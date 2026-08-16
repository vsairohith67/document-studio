use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::path_policy::{
    canonical_directory, canonical_regular_file, ensure_different_files, validate_output_name,
    PathPolicyError,
};
use crate::windows_security::{available_bytes, is_collision_error, move_no_replace};

const COPY_BUFFER_SIZE: usize = 1024 * 1024;
const MAX_COLLISION_ATTEMPTS: u32 = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationResult {
    pub final_path: PathBuf,
    pub resolved_name: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[doc(hidden)]
pub struct PublicationContext<'a> {
    pub staging_path: &'a Path,
    pub input_path: &'a Path,
    pub destination_directory: &'a Path,
    pub requested_name: &'a str,
    pub job_id: &'a str,
}

#[derive(Debug, Error)]
pub enum PublicationError {
    #[error("publication path policy failed")]
    Path(#[from] PathPolicyError),
    #[error("publication filesystem operation failed")]
    Io(#[from] io::Error),
    #[error("the operation was cancelled before publication")]
    Cancelled,
    #[error("the destination does not have enough available space")]
    InsufficientSpace,
    #[error("the destination partial did not match the verified staging file")]
    VerificationMismatch,
    #[error("no collision-free output name was available")]
    CollisionExhausted,
}

pub fn publish_verified_staging<C, P>(
    staging_path: &Path,
    input_path: &Path,
    destination_directory: &Path,
    requested_name: &str,
    job_id: &str,
    mut is_cancelled: C,
    mut on_progress: P,
) -> Result<PublicationResult, PublicationError>
where
    C: FnMut() -> bool,
    P: FnMut(u64, u64),
{
    publish_verified_staging_with_observer(
        PublicationContext {
            staging_path,
            input_path,
            destination_directory,
            requested_name,
            job_id,
        },
        &mut is_cancelled,
        |completed, total| {
            on_progress(completed, total);
            Ok(())
        },
        |_| Ok(()),
    )
}

#[doc(hidden)]
pub fn publish_verified_staging_with_observer<C, P, O>(
    context: PublicationContext<'_>,
    mut is_cancelled: C,
    mut on_progress: P,
    mut before_commit: O,
) -> Result<PublicationResult, PublicationError>
where
    C: FnMut() -> bool,
    P: FnMut(u64, u64) -> io::Result<()>,
    O: FnMut(&Path) -> Result<(), PublicationError>,
{
    let PublicationContext {
        staging_path,
        input_path,
        destination_directory,
        requested_name,
        job_id,
    } = context;
    Uuid::parse_str(job_id)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid job identifier"))?;
    validate_output_name(requested_name)?;
    let destination_directory = canonical_directory(destination_directory)?;
    let (staging_path, _) = canonical_regular_file(staging_path)?;
    let (input_path, _) = canonical_regular_file(input_path)?;
    let (expected_size, expected_hash) = hash_file(&staging_path)?;
    if available_bytes(&destination_directory)?
        < expected_size.saturating_add(COPY_BUFFER_SIZE as u64)
    {
        return Err(PublicationError::InsufficientSpace);
    }

    for attempt in 0..MAX_COLLISION_ATTEMPTS {
        if is_cancelled() {
            return Err(PublicationError::Cancelled);
        }
        let resolved_name = collision_name(requested_name, attempt);
        validate_output_name(&resolved_name)?;
        let final_path = destination_directory.join(&resolved_name);
        ensure_different_files(&input_path, &final_path)?;
        if final_path.exists() {
            continue;
        }

        let partial_name = format!(
            ".document-studio-{}-{}.partial",
            job_id,
            Uuid::new_v4().hyphenated()
        );
        let partial_path = destination_directory.join(partial_name);
        let partial_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial_path)?;

        let copied = copy_to_partial(
            &staging_path,
            partial_file,
            expected_size,
            &mut is_cancelled,
            &mut on_progress,
        );
        let (copied_size, copied_hash) = match copied {
            Ok(value) => value,
            Err(error) => {
                remove_owned_partial(&partial_path);
                return Err(error);
            }
        };
        if copied_size != expected_size || copied_hash != expected_hash {
            remove_owned_partial(&partial_path);
            return Err(PublicationError::VerificationMismatch);
        }
        let (reopened_size, reopened_hash) = hash_file(&partial_path)?;
        if reopened_size != expected_size || reopened_hash != expected_hash {
            remove_owned_partial(&partial_path);
            return Err(PublicationError::VerificationMismatch);
        }

        if is_cancelled() {
            remove_owned_partial(&partial_path);
            return Err(PublicationError::Cancelled);
        }
        if let Err(error) = before_commit(&final_path) {
            remove_owned_partial(&partial_path);
            return Err(error);
        }
        match move_no_replace(&partial_path, &final_path) {
            Ok(()) => {
                return Ok(PublicationResult {
                    final_path,
                    resolved_name,
                    size_bytes: expected_size,
                    sha256: expected_hash,
                });
            }
            Err(error) if is_collision_error(&error) => {
                remove_owned_partial(&partial_path);
            }
            Err(error) => {
                remove_owned_partial(&partial_path);
                return Err(error.into());
            }
        }
    }

    Err(PublicationError::CollisionExhausted)
}

pub fn collision_name(requested_name: &str, attempt: u32) -> String {
    if attempt == 0 {
        return requested_name.to_owned();
    }
    let path = Path::new(requested_name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(requested_name);
    match path.extension().and_then(|value| value.to_str()) {
        Some(extension) if !extension.is_empty() => format!("{stem} ({attempt}).{extension}"),
        _ => format!("{stem} ({attempt})"),
    }
}

pub fn hash_file(path: &Path) -> Result<(u64, String), io::Error> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; COPY_BUFFER_SIZE];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total = total.saturating_add(read as u64);
    }
    Ok((total, digest_hex(hasher.finalize().as_slice())))
}

fn copy_to_partial<C, P>(
    staging_path: &Path,
    mut destination: File,
    total: u64,
    is_cancelled: &mut C,
    on_progress: &mut P,
) -> Result<(u64, String), PublicationError>
where
    C: FnMut() -> bool,
    P: FnMut(u64, u64) -> io::Result<()>,
{
    let mut source = File::open(staging_path)?;
    let mut hasher = Sha256::new();
    let mut completed = 0_u64;
    let mut buffer = vec![0_u8; COPY_BUFFER_SIZE];
    loop {
        if is_cancelled() {
            return Err(PublicationError::Cancelled);
        }
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        destination.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
        completed = completed.saturating_add(read as u64);
        on_progress(completed, total)?;
    }
    destination.sync_all()?;
    drop(destination);
    if is_cancelled() {
        return Err(PublicationError::Cancelled);
    }
    Ok((completed, digest_hex(hasher.finalize().as_slice())))
}

fn digest_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn remove_owned_partial(path: &Path) {
    if let Err(error) = fs::remove_file(path) {
        if error.kind() != io::ErrorKind::NotFound {
            // The caller retains the primary failure. Startup recovery also reconciles recorded partials.
        }
    }
}
