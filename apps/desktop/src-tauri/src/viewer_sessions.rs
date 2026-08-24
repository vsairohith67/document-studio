use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::windows::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, SecondsFormat, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::app_state::CancellationToken;
use crate::contracts::{
    DestinationGrant, OperationError, OperationStage, ViewerDocumentMetadata, ViewerRangeRequest,
    ViewerSessionRequest,
};
use crate::path_policy::{canonical_directory, canonical_regular_file};
use crate::windows_security::{identity_from_file, open_viewer_readonly, FileIdentity};

pub const VIEWER_RANGE_CHUNK_BYTES: u64 = 256 * 1024;
pub const VIEWER_MAX_RANGE_BYTES: u64 = 1024 * 1024;
pub const VIEWER_MAX_READS_IN_FLIGHT: usize = 4;
pub const VIEWER_MAX_SESSIONS: usize = 2;
pub const VIEWER_SESSION_IDLE_EXPIRY: Duration = Duration::from_secs(2 * 60 * 60);
pub const VIEWER_SESSION_MAX_LIFETIME: Duration = Duration::from_secs(8 * 60 * 60);
const SNAPSHOT_BUFFER_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
pub struct ViewerSessionManager {
    sessions: Arc<Mutex<HashMap<String, Arc<ViewerSession>>>>,
    destination_grants: Arc<Mutex<HashMap<String, DestinationGrantRecord>>>,
    next_generation: Arc<AtomicU64>,
    drop_enabled: Arc<AtomicBool>,
}

struct ViewerSession {
    id: String,
    generation: u64,
    source_path: PathBuf,
    file: Arc<File>,
    identity: FileIdentity,
    size_bytes: u64,
    modified_at: String,
    display_name: String,
    created_at: Instant,
    last_access: Mutex<Instant>,
    active: Arc<AtomicBool>,
    reads_in_flight: AtomicUsize,
}

#[derive(Debug, Clone)]
struct DestinationGrantRecord {
    path: PathBuf,
    created_at: Instant,
}

#[derive(Clone)]
pub struct ViewerJobSource {
    pub path: PathBuf,
    pub display_name: String,
    pub size_bytes: u64,
    pub modified_at: String,
    pub file_identity: String,
    session: Arc<ViewerSession>,
}

struct ReadPermit<'a> {
    count: &'a AtomicUsize,
}

impl Drop for ReadPermit<'_> {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Default for ViewerSessionManager {
    fn default() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            destination_grants: Arc::new(Mutex::new(HashMap::new())),
            next_generation: Arc::new(AtomicU64::new(1)),
            drop_enabled: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl ViewerSessionManager {
    pub fn set_drop_enabled(&self, enabled: bool) {
        self.drop_enabled.store(enabled, Ordering::Release);
    }

    pub fn drop_enabled(&self) -> bool {
        self.drop_enabled.load(Ordering::Acquire)
    }

    pub fn open_pdf(&self, selected_path: &Path) -> Result<ViewerDocumentMetadata, OperationError> {
        let (canonical_path, expected_identity) =
            canonical_regular_file(selected_path).map_err(|_| unsafe_source())?;
        let display_name = canonical_path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .ok_or_else(unsafe_source)?
            .to_owned();
        if !canonical_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
        {
            return Err(unsupported_source());
        }
        let file = open_viewer_readonly(&canonical_path).map_err(map_open_error)?;
        let identity = identity_from_file(&file).map_err(|_| unsafe_source())?;
        if identity != expected_identity {
            return Err(source_changed());
        }
        let metadata = file.metadata().map_err(|_| unsafe_source())?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(unsupported_source());
        }
        let mut header = vec![0_u8; usize::try_from(metadata.len().min(1024)).unwrap_or(1024)];
        read_exact_at(&file, &mut header, 0).map_err(|_| unsupported_source())?;
        if !header.windows(5).any(|window| window == b"%PDF-") {
            return Err(unsupported_source());
        }
        let modified_at = modified_timestamp(&metadata).map_err(|_| unsafe_source())?;

        let mut sessions = self
            .sessions
            .lock()
            .expect("viewer session registry mutex poisoned");
        sessions.retain(|_, session| !session.is_expired());
        if sessions.len() >= VIEWER_MAX_SESSIONS {
            return Err(OperationError::safe(
                "VIEWER_SESSION_LIMIT",
                "Too many PDFs are open",
                "Close an open document before opening another PDF.",
                OperationStage::Inspect,
                true,
            ));
        }
        let id = Uuid::new_v4().to_string();
        let generation = self.next_generation.fetch_add(1, Ordering::AcqRel);
        let session = Arc::new(ViewerSession {
            id: id.clone(),
            generation,
            source_path: canonical_path,
            file: Arc::new(file),
            identity,
            size_bytes: metadata.len(),
            modified_at: modified_at.clone(),
            display_name: display_name.clone(),
            created_at: Instant::now(),
            last_access: Mutex::new(Instant::now()),
            active: Arc::new(AtomicBool::new(true)),
            reads_in_flight: AtomicUsize::new(0),
        });
        sessions.insert(id.clone(), session);
        Ok(ViewerDocumentMetadata {
            session_id: id,
            generation,
            display_name,
            size_bytes: metadata.len(),
            modified_at,
            mime_type: "application/pdf".to_owned(),
            file_identity: identity.to_string(),
        })
    }

    pub fn read_range(&self, request: &ViewerRangeRequest) -> Result<Vec<u8>, OperationError> {
        let session = self.get(&request.session_id, request.generation)?;
        if request.end <= request.begin
            || request.end > session.size_bytes
            || request.end.saturating_sub(request.begin) > VIEWER_MAX_RANGE_BYTES
        {
            return Err(invalid_range());
        }
        let _permit = session.acquire_read()?;
        if !session.active.load(Ordering::Acquire) {
            return Err(session_expired());
        }
        session.validate_unchanged()?;
        let length = usize::try_from(request.end - request.begin).map_err(|_| invalid_range())?;
        let mut bytes = vec![0_u8; length];
        read_exact_at(&session.file, &mut bytes, request.begin).map_err(|_| range_read_failed())?;
        if !session.active.load(Ordering::Acquire)
            || !self.contains_generation(&session.id, session.generation)
        {
            return Err(session_expired());
        }
        Ok(bytes)
    }

    pub fn close(&self, request: &ViewerSessionRequest) -> Result<(), OperationError> {
        let mut sessions = self
            .sessions
            .lock()
            .expect("viewer session registry mutex poisoned");
        let session = sessions
            .get(&request.session_id)
            .filter(|session| session.generation == request.generation)
            .cloned()
            .ok_or_else(session_expired)?;
        session.active.store(false, Ordering::Release);
        sessions.remove(&request.session_id);
        Ok(())
    }

    pub fn close_all(&self) {
        let mut sessions = self
            .sessions
            .lock()
            .expect("viewer session registry mutex poisoned");
        for session in sessions.values() {
            session.active.store(false, Ordering::Release);
        }
        sessions.clear();
        self.destination_grants
            .lock()
            .expect("destination grant registry mutex poisoned")
            .clear();
    }

    pub fn source_for_job(
        &self,
        session_id: &str,
        generation: u64,
    ) -> Result<ViewerJobSource, OperationError> {
        let session = self.get(session_id, generation)?;
        session.validate_unchanged()?;
        Ok(ViewerJobSource {
            path: session.source_path.clone(),
            display_name: session.display_name.clone(),
            size_bytes: session.size_bytes,
            modified_at: session.modified_at.clone(),
            file_identity: session.identity.to_string(),
            session,
        })
    }

    pub fn grant_destination(
        &self,
        selected_path: &Path,
    ) -> Result<DestinationGrant, OperationError> {
        let path = canonical_directory(selected_path).map_err(|_| unsafe_destination())?;
        let display_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("Local folder")
            .to_owned();
        let grant_id = Uuid::new_v4().to_string();
        self.destination_grants
            .lock()
            .expect("destination grant registry mutex poisoned")
            .insert(
                grant_id.clone(),
                DestinationGrantRecord {
                    path,
                    created_at: Instant::now(),
                },
            );
        Ok(DestinationGrant {
            grant_id,
            display_name,
        })
    }

    pub fn resolve_destination(&self, grant_id: &str) -> Result<PathBuf, OperationError> {
        let mut grants = self
            .destination_grants
            .lock()
            .expect("destination grant registry mutex poisoned");
        grants.retain(|_, grant| grant.created_at.elapsed() <= VIEWER_SESSION_IDLE_EXPIRY);
        let path = grants
            .get(grant_id)
            .map(|grant| grant.path.clone())
            .ok_or_else(|| {
                OperationError::safe(
                    "DESTINATION_GRANT_EXPIRED",
                    "The destination selection expired",
                    "Choose the destination folder again.",
                    OperationStage::Preflight,
                    true,
                )
            })?;
        canonical_directory(&path).map_err(|_| unsafe_destination())
    }

    pub fn revoke_destination(&self, grant_id: &str) {
        self.destination_grants
            .lock()
            .expect("destination grant registry mutex poisoned")
            .remove(grant_id);
    }

    fn get(&self, id: &str, generation: u64) -> Result<Arc<ViewerSession>, OperationError> {
        let mut sessions = self
            .sessions
            .lock()
            .expect("viewer session registry mutex poisoned");
        let expired = sessions.get(id).is_some_and(|session| session.is_expired());
        if expired {
            if let Some(session) = sessions.remove(id) {
                session.active.store(false, Ordering::Release);
            }
        }
        let session = sessions
            .get(id)
            .filter(|session| session.generation == generation)
            .cloned()
            .ok_or_else(session_expired)?;
        session.touch();
        Ok(session)
    }

    fn contains_generation(&self, id: &str, generation: u64) -> bool {
        self.sessions
            .lock()
            .expect("viewer session registry mutex poisoned")
            .get(id)
            .is_some_and(|session| session.generation == generation)
    }
}

impl ViewerSession {
    fn is_expired(&self) -> bool {
        !self.active.load(Ordering::Acquire)
            || self.created_at.elapsed() > VIEWER_SESSION_MAX_LIFETIME
            || self
                .last_access
                .lock()
                .expect("viewer access time mutex poisoned")
                .elapsed()
                > VIEWER_SESSION_IDLE_EXPIRY
    }

    fn touch(&self) {
        *self
            .last_access
            .lock()
            .expect("viewer access time mutex poisoned") = Instant::now();
    }

    fn acquire_read(&self) -> Result<ReadPermit<'_>, OperationError> {
        let acquired =
            self.reads_in_flight
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                    (current < VIEWER_MAX_READS_IN_FLIGHT).then_some(current + 1)
                });
        if acquired.is_err() {
            return Err(OperationError::safe(
                "VIEWER_RANGE_BUSY",
                "The viewer is already loading several ranges",
                "Wait for the visible pages to finish loading and try again.",
                OperationStage::Inspect,
                true,
            ));
        }
        Ok(ReadPermit {
            count: &self.reads_in_flight,
        })
    }

    fn validate_unchanged(&self) -> Result<(), OperationError> {
        let metadata = self.file.metadata().map_err(|_| source_changed())?;
        let modified_at = modified_timestamp(&metadata).map_err(|_| source_changed())?;
        let identity = identity_from_file(&self.file).map_err(|_| source_changed())?;
        let (current_path, current_identity) =
            canonical_regular_file(&self.source_path).map_err(|_| source_changed())?;
        if metadata.len() != self.size_bytes
            || modified_at != self.modified_at
            || identity != self.identity
            || current_identity != self.identity
            || current_path != self.source_path
        {
            return Err(source_changed());
        }
        Ok(())
    }
}

impl ViewerJobSource {
    pub fn copy_snapshot(
        &self,
        destination: &Path,
        token: &CancellationToken,
    ) -> Result<(u64, String), OperationError> {
        self.session.validate_unchanged()?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|_| snapshot_failed())?;
        let mut offset = 0_u64;
        let mut digest = Sha256::new();
        let mut buffer = vec![0_u8; SNAPSHOT_BUFFER_BYTES];
        while offset < self.size_bytes {
            if token.is_cancelled() {
                return Err(OperationError::safe(
                    "CANCELLED",
                    "The PDF operation was cancelled",
                    "Private snapshots and unpublished outputs will be removed safely.",
                    OperationStage::Execute,
                    false,
                ));
            }
            let remaining = usize::try_from((self.size_bytes - offset).min(buffer.len() as u64))
                .map_err(|_| snapshot_failed())?;
            read_exact_at(&self.session.file, &mut buffer[..remaining], offset)
                .map_err(|_| source_changed())?;
            output
                .write_all(&buffer[..remaining])
                .map_err(|_| snapshot_failed())?;
            digest.update(&buffer[..remaining]);
            offset += u64::try_from(remaining).map_err(|_| snapshot_failed())?;
        }
        output.sync_all().map_err(|_| snapshot_failed())?;
        self.session.validate_unchanged()?;
        Ok((offset, hex_digest(&digest.finalize())))
    }

    pub fn verify_unchanged_hash(
        &self,
        expected_sha256: &str,
        token: &CancellationToken,
    ) -> Result<(), OperationError> {
        self.session.validate_unchanged()?;
        let mut offset = 0_u64;
        let mut digest = Sha256::new();
        let mut buffer = vec![0_u8; SNAPSHOT_BUFFER_BYTES];
        while offset < self.size_bytes {
            if token.is_cancelled() {
                return Err(OperationError::safe(
                    "CANCELLED",
                    "The PDF operation was cancelled",
                    "Private snapshots and unpublished outputs will be removed safely.",
                    OperationStage::Verify,
                    false,
                ));
            }
            let remaining = usize::try_from((self.size_bytes - offset).min(buffer.len() as u64))
                .map_err(|_| source_changed())?;
            read_exact_at(&self.session.file, &mut buffer[..remaining], offset)
                .map_err(|_| source_changed())?;
            digest.update(&buffer[..remaining]);
            offset += u64::try_from(remaining).map_err(|_| source_changed())?;
        }
        self.session.validate_unchanged()?;
        if hex_digest(&digest.finalize()) != expected_sha256 {
            return Err(source_changed());
        }
        Ok(())
    }
}

fn read_exact_at(file: &File, mut buffer: &mut [u8], mut offset: u64) -> io::Result<()> {
    while !buffer.is_empty() {
        let read = file.seek_read(buffer, offset)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "range ended early",
            ));
        }
        offset = offset
            .checked_add(u64::try_from(read).map_err(|_| io::Error::other("range overflow"))?)
            .ok_or_else(|| io::Error::other("range overflow"))?;
        buffer = &mut buffer[read..];
    }
    Ok(())
}

fn modified_timestamp(metadata: &std::fs::Metadata) -> Result<String, io::Error> {
    let modified: DateTime<Utc> = metadata.modified()?.into();
    Ok(modified.to_rfc3339_opts(SecondsFormat::Nanos, true))
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn map_open_error(error: io::Error) -> OperationError {
    if matches!(error.raw_os_error(), Some(32 | 33)) {
        OperationError::safe(
            "SOURCE_BUSY",
            "The PDF is open for editing",
            "Close the program changing this file, then open it again.",
            OperationStage::Inspect,
            true,
        )
    } else {
        unsafe_source()
    }
}

fn unsafe_source() -> OperationError {
    OperationError::safe(
        "PATH_UNSAFE",
        "The selected PDF path is not safe",
        "Choose a regular local PDF without links or special path syntax.",
        OperationStage::Inspect,
        false,
    )
}

fn unsupported_source() -> OperationError {
    OperationError::safe(
        "PDF_UNSUPPORTED",
        "The selected file is not a supported PDF",
        "Choose a non-empty local file with a PDF header and .pdf extension.",
        OperationStage::Inspect,
        false,
    )
}

fn source_changed() -> OperationError {
    OperationError::safe(
        "SOURCE_CHANGED",
        "The source PDF changed",
        "Close this document and open it again before continuing.",
        OperationStage::Inspect,
        true,
    )
}

fn session_expired() -> OperationError {
    OperationError::safe(
        "VIEWER_SESSION_EXPIRED",
        "The PDF viewer session expired",
        "Open the PDF again to continue.",
        OperationStage::Inspect,
        true,
    )
}

fn invalid_range() -> OperationError {
    OperationError::safe(
        "VIEWER_RANGE_INVALID",
        "The requested PDF byte range is invalid",
        "Reload the document; no bytes were returned.",
        OperationStage::Inspect,
        false,
    )
}

fn range_read_failed() -> OperationError {
    OperationError::safe(
        "VIEWER_RANGE_READ_FAILED",
        "The PDF bytes could not be read",
        "Reload the document or open it again.",
        OperationStage::Inspect,
        true,
    )
}

fn unsafe_destination() -> OperationError {
    OperationError::safe(
        "DESTINATION_UNSAFE",
        "The destination folder is not safe",
        "Choose an existing local folder without links or special path syntax.",
        OperationStage::Preflight,
        false,
    )
}

fn snapshot_failed() -> OperationError {
    OperationError::safe(
        "SNAPSHOT_WRITE_FAILED",
        "The private PDF snapshot could not be created",
        "Check available disk space and try again.",
        OperationStage::Execute,
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ViewerSessionManager, VIEWER_MAX_RANGE_BYTES, VIEWER_MAX_SESSIONS, VIEWER_RANGE_CHUNK_BYTES,
    };
    use crate::contracts::{ViewerRangeRequest, ViewerSessionRequest};
    use std::fs::OpenOptions;
    use std::io::Write;
    use tempfile::tempdir;

    fn minimal_pdf() -> Vec<u8> {
        b"%PDF-1.4\n1 0 obj<</Type/Catalog>>endobj\n%%EOF\n".to_vec()
    }

    #[test]
    fn ranges_are_binary_bounded_and_close_invalidates_generation() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("viewer.pdf");
        std::fs::write(&path, minimal_pdf()).unwrap();
        let manager = ViewerSessionManager::default();
        let metadata = manager.open_pdf(&path).unwrap();
        let end = metadata.size_bytes.min(VIEWER_RANGE_CHUNK_BYTES);
        let bytes = manager
            .read_range(&ViewerRangeRequest {
                session_id: metadata.session_id.clone(),
                generation: metadata.generation,
                begin: 0,
                end,
            })
            .unwrap();
        assert_eq!(&bytes[..5], b"%PDF-");
        assert_eq!(VIEWER_MAX_RANGE_BYTES, 1024 * 1024);
        manager
            .close(&ViewerSessionRequest {
                session_id: metadata.session_id.clone(),
                generation: metadata.generation,
            })
            .unwrap();
        assert_eq!(
            manager
                .read_range(&ViewerRangeRequest {
                    session_id: metadata.session_id,
                    generation: metadata.generation,
                    begin: 0,
                    end,
                })
                .unwrap_err()
                .code,
            "VIEWER_SESSION_EXPIRED"
        );
    }

    #[test]
    fn invalid_and_oversized_ranges_are_rejected() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("viewer.pdf");
        let mut bytes = minimal_pdf();
        bytes.resize(VIEWER_MAX_RANGE_BYTES as usize + 32, 0);
        std::fs::write(&path, bytes).unwrap();
        let manager = ViewerSessionManager::default();
        let metadata = manager.open_pdf(&path).unwrap();
        let request = ViewerRangeRequest {
            session_id: metadata.session_id,
            generation: metadata.generation,
            begin: 0,
            end: VIEWER_MAX_RANGE_BYTES + 1,
        };
        assert_eq!(
            manager.read_range(&request).unwrap_err().code,
            "VIEWER_RANGE_INVALID"
        );
    }

    #[test]
    fn repeated_and_overlapping_positioned_reads_are_exact() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("viewer.pdf");
        let mut source = minimal_pdf();
        source.extend((0..=255).cycle().take(600_000));
        std::fs::write(&path, &source).unwrap();
        let manager = ViewerSessionManager::default();
        let metadata = manager.open_pdf(&path).unwrap();
        let read = |begin, end| {
            manager
                .read_range(&ViewerRangeRequest {
                    session_id: metadata.session_id.clone(),
                    generation: metadata.generation,
                    begin,
                    end,
                })
                .unwrap()
        };
        assert_eq!(read(100, 300), source[100..300]);
        assert_eq!(read(100, 300), source[100..300]);
        assert_eq!(read(200, 500), source[200..500]);
        assert_eq!(read(0, metadata.size_bytes), source);
    }

    #[test]
    fn retained_handle_blocks_write_delete_and_hard_link_alias_mutation() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("viewer.pdf");
        let alias = directory.path().join("alias.pdf");
        std::fs::write(&path, minimal_pdf()).unwrap();
        std::fs::hard_link(&path, &alias).unwrap();
        let manager = ViewerSessionManager::default();
        let metadata = manager.open_pdf(&path).unwrap();

        assert!(OpenOptions::new()
            .write(true)
            .open(&alias)
            .and_then(|mut file| file.write_all(b"changed"))
            .is_err());
        assert!(std::fs::remove_file(&path).is_err());

        manager
            .close(&ViewerSessionRequest {
                session_id: metadata.session_id,
                generation: metadata.generation,
            })
            .unwrap();
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn session_limit_generation_and_shutdown_fail_closed_without_path_metadata() {
        let directory = tempdir().unwrap();
        let manager = ViewerSessionManager::default();
        let mut sessions = Vec::new();
        for index in 0..=VIEWER_MAX_SESSIONS {
            let path = directory.path().join(format!("viewer-{index}.pdf"));
            std::fs::write(&path, minimal_pdf()).unwrap();
            if index < VIEWER_MAX_SESSIONS {
                let metadata = manager.open_pdf(&path).unwrap();
                let serialized = serde_json::to_string(&metadata).unwrap();
                assert!(!serialized.contains(directory.path().to_string_lossy().as_ref()));
                sessions.push(metadata);
            } else {
                assert_eq!(
                    manager.open_pdf(&path).unwrap_err().code,
                    "VIEWER_SESSION_LIMIT"
                );
            }
        }
        let stale = ViewerRangeRequest {
            session_id: sessions[0].session_id.clone(),
            generation: sessions[0].generation + 1,
            begin: 0,
            end: 5,
        };
        assert_eq!(
            manager.read_range(&stale).unwrap_err().code,
            "VIEWER_SESSION_EXPIRED"
        );
        manager.close_all();
        let request = ViewerRangeRequest {
            session_id: sessions[1].session_id.clone(),
            generation: sessions[1].generation,
            begin: 0,
            end: 5,
        };
        assert_eq!(
            manager.read_range(&request).unwrap_err().code,
            "VIEWER_SESSION_EXPIRED"
        );
    }
}
