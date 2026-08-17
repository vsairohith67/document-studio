use std::ffi::OsString;
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
use crate::windows_security::{
    available_bytes, create_delete_on_close, delete_open_file, identity_from_file,
    is_collision_error, move_no_replace, open_for_identity_and_delete, FileIdentity,
};

const COPY_BUFFER_SIZE: usize = 1024 * 1024;
pub const MAX_COLLISION_ATTEMPTS: u32 = 1000;
const PARTIAL_OWNERSHIP_RESULT_PREFIX: &str = "DESTINATION_PARTIAL_OWNED:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationResult {
    pub final_path: PathBuf,
    pub owned_partial_path: PathBuf,
    pub resolved_name: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[doc(hidden)]
pub struct PublicationContext<'a> {
    pub staging_path: &'a Path,
    pub input_paths: &'a [&'a Path],
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
    #[error("the exact owned destination partial could not be reconciled")]
    Cleanup(io::Error),
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
            input_paths: &[input_path],
            destination_directory,
            requested_name,
            job_id,
        },
        &mut is_cancelled,
        |completed, total| {
            on_progress(completed, total);
            Ok(())
        },
        |_, _, _, _, _| Ok(()),
        |_, _| Ok(()),
        |_| Ok(()),
        |_| Ok(()),
    )
}

#[doc(hidden)]
pub fn publish_verified_staging_with_observer<C, P, R, A, L, O>(
    context: PublicationContext<'_>,
    mut is_cancelled: C,
    mut on_progress: P,
    mut reserve_attempt: R,
    mut activate_attempt: A,
    mut release_attempt: L,
    mut before_commit: O,
) -> Result<PublicationResult, PublicationError>
where
    C: FnMut() -> bool,
    P: FnMut(u64, u64) -> io::Result<()>,
    R: FnMut(&Path, &Path, &str, u64, &str) -> Result<(), PublicationError>,
    A: FnMut(&Path, FileIdentity) -> Result<(), PublicationError>,
    L: FnMut(&Path) -> Result<(), PublicationError>,
    O: FnMut(&Path) -> Result<(), PublicationError>,
{
    let PublicationContext {
        staging_path,
        input_paths,
        destination_directory,
        requested_name,
        job_id,
    } = context;
    Uuid::parse_str(job_id)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid job identifier"))?;
    validate_output_name(requested_name)?;
    let destination_directory = canonical_directory(destination_directory)?;
    let (staging_path, _) = canonical_regular_file(staging_path)?;
    let input_paths = input_paths
        .iter()
        .map(|path| canonical_regular_file(path).map(|(path, _)| path))
        .collect::<Result<Vec<_>, _>>()?;
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
        for input_path in &input_paths {
            ensure_different_files(input_path, &final_path)?;
        }
        if final_path.exists() {
            continue;
        }

        let partial_name = format!(
            ".document-studio-{}-{}.partial",
            job_id,
            Uuid::new_v4().hyphenated()
        );
        let partial_path = destination_directory.join(partial_name);
        reserve_attempt(
            &final_path,
            &partial_path,
            &resolved_name,
            expected_size,
            &expected_hash,
        )?;
        let guard_path = partial_guard_path(&partial_path);
        let guard_file = match create_delete_on_close(&guard_path) {
            Ok(file) => file,
            Err(error) => {
                release_attempt(&partial_path)?;
                if error.kind() == io::ErrorKind::AlreadyExists {
                    continue;
                }
                return Err(error.into());
            }
        };
        let partial_identity = match identity_from_file(&guard_file) {
            Ok(identity) => identity,
            Err(error) => {
                drop(guard_file);
                release_attempt(&partial_path)?;
                return Err(error.into());
            }
        };
        if let Err(error) = activate_attempt(&partial_path, partial_identity) {
            drop(guard_file);
            return Err(error);
        }
        if let Err(error) = fs::hard_link(&guard_path, &partial_path) {
            drop(guard_file);
            release_attempt(&partial_path)?;
            if error.kind() == io::ErrorKind::AlreadyExists || is_collision_error(&error) {
                continue;
            }
            return Err(error.into());
        }
        let partial_file = match OpenOptions::new().write(true).open(&partial_path) {
            Ok(file) => match identity_from_file(&file) {
                Ok(identity) if identity == partial_identity => file,
                Ok(_) | Err(_) => {
                    drop(file);
                    drop(guard_file);
                    reconcile_owned_partial(&partial_path, partial_identity, &mut release_attempt)?;
                    return Err(io::Error::other("destination partial identity changed").into());
                }
            },
            Err(error) => {
                drop(guard_file);
                reconcile_owned_partial(&partial_path, partial_identity, &mut release_attempt)?;
                return Err(error.into());
            }
        };
        drop(guard_file);

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
                reconcile_owned_partial(&partial_path, partial_identity, &mut release_attempt)?;
                return Err(error);
            }
        };
        if copied_size != expected_size || copied_hash != expected_hash {
            reconcile_owned_partial(&partial_path, partial_identity, &mut release_attempt)?;
            return Err(PublicationError::VerificationMismatch);
        }
        let (reopened_size, reopened_hash) = match hash_file(&partial_path) {
            Ok(value) => value,
            Err(error) => {
                reconcile_owned_partial(&partial_path, partial_identity, &mut release_attempt)?;
                return Err(error.into());
            }
        };
        if reopened_size != expected_size || reopened_hash != expected_hash {
            reconcile_owned_partial(&partial_path, partial_identity, &mut release_attempt)?;
            return Err(PublicationError::VerificationMismatch);
        }

        if is_cancelled() {
            reconcile_owned_partial(&partial_path, partial_identity, &mut release_attempt)?;
            return Err(PublicationError::Cancelled);
        }
        if let Err(error) = before_commit(&final_path) {
            reconcile_owned_partial(&partial_path, partial_identity, &mut release_attempt)?;
            return Err(error);
        }
        match move_no_replace(&partial_path, &final_path) {
            Ok(()) => {
                let (published_size, published_hash) = match hash_file(&final_path) {
                    Ok(value) => value,
                    Err(error) => {
                        reconcile_owned_partial(
                            &partial_path,
                            partial_identity,
                            &mut release_attempt,
                        )?;
                        return Err(error.into());
                    }
                };
                if published_size != expected_size || published_hash != expected_hash {
                    reconcile_owned_partial(&partial_path, partial_identity, &mut release_attempt)?;
                    return Err(PublicationError::VerificationMismatch);
                }
                return Ok(PublicationResult {
                    final_path,
                    owned_partial_path: partial_path,
                    resolved_name,
                    size_bytes: expected_size,
                    sha256: expected_hash,
                });
            }
            Err(error) if is_collision_error(&error) => {
                reconcile_owned_partial(&partial_path, partial_identity, &mut release_attempt)?;
            }
            Err(error) => {
                reconcile_owned_partial(&partial_path, partial_identity, &mut release_attempt)?;
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

pub fn is_exact_owned_partial_path(
    destination_directory: &Path,
    job_id: &str,
    partial_path: &Path,
) -> bool {
    if Uuid::parse_str(job_id).is_err() || partial_path.parent() != Some(destination_directory) {
        return false;
    }
    let Some(name) = partial_path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let prefix = format!(".document-studio-{job_id}-");
    let Some(identifier) = name
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix(".partial"))
    else {
        return false;
    };
    Uuid::parse_str(identifier).is_ok()
}

pub fn partial_ownership_result_code(
    destination_directory: &Path,
    job_id: &str,
    partial_path: &Path,
    identity: FileIdentity,
) -> Option<String> {
    if Uuid::parse_str(job_id).is_err() || partial_path.parent() != Some(destination_directory) {
        return None;
    }
    let name = partial_path.file_name()?.to_str()?;
    let prefix = format!(".document-studio-{job_id}-");
    let identifier = name.strip_prefix(&prefix)?.strip_suffix(".partial")?;
    let identifier = Uuid::parse_str(identifier).ok()?;
    Some(format!(
        "{PARTIAL_OWNERSHIP_RESULT_PREFIX}{}:{identity}",
        identifier.hyphenated(),
    ))
}

fn partial_guard_path(partial_path: &Path) -> PathBuf {
    let mut name = OsString::from(partial_path.file_name().unwrap_or_default());
    name.push(".guard");
    partial_path.with_file_name(name)
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

fn reconcile_owned_partial<L>(
    path: &Path,
    expected_identity: FileIdentity,
    release_attempt: &mut L,
) -> Result<(), PublicationError>
where
    L: FnMut(&Path) -> Result<(), PublicationError>,
{
    match open_for_identity_and_delete(path) {
        Ok(file) => {
            let actual_identity = identity_from_file(&file).map_err(PublicationError::Cleanup)?;
            if actual_identity == expected_identity {
                delete_open_file(file).map_err(PublicationError::Cleanup)?;
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(PublicationError::Cleanup(error)),
    }
    release_attempt(path)
}
