use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

use crate::contracts::{PDF_MERGE_MAX_INPUTS, PDF_MERGE_MIN_INPUTS};

pub const MERGED_STAGING_RELATIVE_PATH: &str = r"staging\merged.pdf";
pub const COMPRESSED_STAGING_RELATIVE_PATH: &str = r"staging\compressed.pdf";
pub const QPDF_BUNDLE_MANIFEST_JSON: &str =
    include_str!("../resources/qpdf/12.3.2/qpdf-manifest.json");
const QPDF_CACHE_DIRECTORY: &str = "qpdf/12.3.2";
const QPDF_MANIFEST_NAME: &str = "qpdf-manifest.json";
const HASH_BUFFER_SIZE: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct QpdfRuntimeManager {
    bundle_root: PathBuf,
    cache_parent: PathBuf,
    runtime: Arc<Mutex<Option<VerifiedQpdfRuntime>>>,
}

#[derive(Debug, Clone)]
pub struct VerifiedQpdfRuntime {
    pub root: PathBuf,
    pub bin: PathBuf,
    pub executable: PathBuf,
    held_files: Arc<Vec<File>>,
}

impl VerifiedQpdfRuntime {
    pub fn held_file_count(&self) -> usize {
        self.held_files.len()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BundleManifest {
    schema_version: u32,
    dependency: String,
    version: String,
    files: Vec<BundleFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BundleFile {
    path: String,
    size_bytes: u64,
    sha256: String,
}

#[derive(Debug, Error)]
pub enum QpdfDependencyError {
    #[error("the compiled qpdf manifest is invalid")]
    Manifest,
    #[error("the bundled qpdf runtime failed verification")]
    BundleMismatch,
    #[error("the private qpdf runtime cache failed verification")]
    CacheMismatch,
    #[error("the qpdf runtime filesystem operation failed")]
    Io(#[from] io::Error),
}

impl QpdfRuntimeManager {
    pub fn new(bundle_root: PathBuf, cache_parent: PathBuf) -> Self {
        Self {
            bundle_root,
            cache_parent,
            runtime: Arc::new(Mutex::new(None)),
        }
    }

    pub fn get_or_prepare(&self) -> Result<VerifiedQpdfRuntime, QpdfDependencyError> {
        let mut slot = self.runtime.lock().map_err(|_| {
            QpdfDependencyError::Io(io::Error::other("qpdf runtime lock was poisoned"))
        })?;
        if let Some(runtime) = slot.as_ref() {
            return Ok(runtime.clone());
        }
        let runtime = prepare_runtime(&self.bundle_root, &self.cache_parent)?;
        *slot = Some(runtime.clone());
        Ok(runtime)
    }
}

fn prepare_runtime(
    bundle_root: &Path,
    cache_parent: &Path,
) -> Result<VerifiedQpdfRuntime, QpdfDependencyError> {
    let manifest: BundleManifest = serde_json::from_str(QPDF_BUNDLE_MANIFEST_JSON)
        .map_err(|_| QpdfDependencyError::Manifest)?;
    if manifest.schema_version != 1
        || manifest.dependency != "qpdf"
        || manifest.version != crate::contracts::QPDF_VERSION
        || manifest.files.is_empty()
    {
        return Err(QpdfDependencyError::Manifest);
    }
    verify_bundle(bundle_root, &manifest)?;

    fs::create_dir_all(cache_parent)?;
    let cache_root = cache_parent.join(QPDF_CACHE_DIRECTORY);
    if !cache_root.exists() {
        materialize_cache(bundle_root, cache_parent, &cache_root, &manifest)?;
    }
    verify_cache(&cache_root, &manifest)?;

    let bin = cache_root.join("bin");
    let executable = bin.join("qpdf.exe");
    let mut held_files = Vec::new();
    for file in manifest
        .files
        .iter()
        .filter(|file| file.path.starts_with("bin/"))
    {
        held_files.push(
            OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ)
                .open(cache_root.join(native_relative(&file.path)?))?,
        );
    }
    Ok(VerifiedQpdfRuntime {
        root: cache_root,
        bin,
        executable,
        held_files: Arc::new(held_files),
    })
}

fn verify_bundle(bundle_root: &Path, manifest: &BundleManifest) -> Result<(), QpdfDependencyError> {
    if fs::read_to_string(bundle_root.join(QPDF_MANIFEST_NAME))? != QPDF_BUNDLE_MANIFEST_JSON {
        return Err(QpdfDependencyError::BundleMismatch);
    }
    for file in &manifest.files {
        if !matches_manifest_file(&bundle_root.join(native_relative(&file.path)?), file)? {
            return Err(QpdfDependencyError::BundleMismatch);
        }
    }
    let expected = manifest
        .files
        .iter()
        .map(|file| file.path.replace('\\', "/"))
        .chain(std::iter::once(QPDF_MANIFEST_NAME.to_owned()))
        .collect::<HashSet<_>>();
    if collect_relative_files(bundle_root)? != expected {
        return Err(QpdfDependencyError::BundleMismatch);
    }
    Ok(())
}

fn materialize_cache(
    bundle_root: &Path,
    cache_parent: &Path,
    cache_root: &Path,
    manifest: &BundleManifest,
) -> Result<(), QpdfDependencyError> {
    let qpdf_parent = cache_parent.join("qpdf");
    fs::create_dir_all(&qpdf_parent)?;
    let temporary = qpdf_parent.join(format!(".12.3.2-{}.preparing", Uuid::new_v4()));
    fs::create_dir(&temporary)?;
    fs::create_dir(temporary.join("bin"))?;
    let result = (|| {
        for file in manifest
            .files
            .iter()
            .filter(|file| file.path.starts_with("bin/"))
        {
            let relative = native_relative(&file.path)?;
            let mut source = File::open(bundle_root.join(&relative))?;
            let mut destination = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(temporary.join(&relative))?;
            io::copy(&mut source, &mut destination)?;
            destination.sync_all()?;
        }
        let mut manifest_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(temporary.join(QPDF_MANIFEST_NAME))?;
        manifest_file.write_all(QPDF_BUNDLE_MANIFEST_JSON.as_bytes())?;
        manifest_file.sync_all()?;
        drop(manifest_file);
        verify_cache(&temporary, manifest)?;
        fs::rename(&temporary, cache_root)?;
        Ok(())
    })();
    if result.is_err() && temporary.exists() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

fn verify_cache(cache_root: &Path, manifest: &BundleManifest) -> Result<(), QpdfDependencyError> {
    if fs::read_to_string(cache_root.join(QPDF_MANIFEST_NAME))? != QPDF_BUNDLE_MANIFEST_JSON {
        return Err(QpdfDependencyError::CacheMismatch);
    }
    let runtime_files = manifest
        .files
        .iter()
        .filter(|file| file.path.starts_with("bin/"))
        .collect::<Vec<_>>();
    for file in &runtime_files {
        if !matches_manifest_file(&cache_root.join(native_relative(&file.path)?), file)? {
            return Err(QpdfDependencyError::CacheMismatch);
        }
    }
    let expected = runtime_files
        .iter()
        .map(|file| file.path.replace('\\', "/"))
        .chain(std::iter::once(QPDF_MANIFEST_NAME.to_owned()))
        .collect::<HashSet<_>>();
    if collect_relative_files(cache_root)? != expected {
        return Err(QpdfDependencyError::CacheMismatch);
    }
    Ok(())
}

fn matches_manifest_file(path: &Path, expected: &BundleFile) -> Result<bool, io::Error> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() != expected.size_bytes {
        return Ok(false);
    }
    let mut source = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0_u8; HASH_BUFFER_SIZE];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    let digest = hash.finalize();
    let digest = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(digest == expected.sha256)
}

fn collect_relative_files(root: &Path) -> Result<HashSet<String>, io::Error> {
    fn visit(root: &Path, directory: &Path, output: &mut HashSet<String>) -> io::Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                return Err(io::Error::other("qpdf runtime contains a link"));
            }
            if file_type.is_dir() {
                visit(root, &entry.path(), output)?;
            } else if file_type.is_file() {
                let relative = entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|_| io::Error::other("qpdf runtime escaped its root"))?
                    .to_string_lossy()
                    .replace('\\', "/");
                output.insert(relative);
            } else {
                return Err(io::Error::other("qpdf runtime contains a special file"));
            }
        }
        Ok(())
    }
    let mut output = HashSet::new();
    visit(root, root, &mut output)?;
    Ok(output)
}

fn native_relative(relative: &str) -> Result<PathBuf, QpdfDependencyError> {
    if relative.is_empty()
        || relative.contains('\\')
        || relative.split('/').any(|component| {
            component.is_empty() || component == "." || component == ".." || component.contains(':')
        })
    {
        return Err(QpdfDependencyError::Manifest);
    }
    Ok(relative.split('/').collect())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrdinalSnapshot {
    pub ordinal: u32,
    pub relative_path: PathBuf,
}

impl OrdinalSnapshot {
    pub fn for_ordinal(ordinal: u32) -> Self {
        Self {
            ordinal,
            relative_path: snapshot_relative_path(ordinal),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuralCheckOutcome {
    Valid,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionCheckOutcome {
    Encrypted,
    Unencrypted,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum QpdfContractError {
    #[error("PDF Merge requires between 2 and 128 ordinal snapshots")]
    InputCount,
    #[error("snapshot ordinals must be contiguous and ordered")]
    OrdinalOrder,
    #[error("every ordinal must use its exact ASCII snapshot path")]
    SnapshotPath,
    #[error("snapshot paths must be physically distinct")]
    DuplicateSnapshotPath,
    #[error("the staging output path is not the owned operation path")]
    StagingPath,
    #[error("qpdf returned an unexpected status")]
    UnexpectedExit,
}

pub fn snapshot_relative_path(ordinal: u32) -> PathBuf {
    PathBuf::from(format!(r"inputs\source-{ordinal:04}.pdf"))
}

pub fn version_output_is_expected(output: &[u8]) -> bool {
    let Ok(output) = std::str::from_utf8(output) else {
        return false;
    };
    let lines = output.lines().collect::<Vec<_>>();
    matches!(
        lines.as_slice(),
        ["qpdf version 12.3.2"]
            | [
                "qpdf version 12.3.2",
                "Run qpdf --copyright to see copyright and license information."
            ]
    )
}

pub fn build_production_merge_arguments(
    snapshots: &[OrdinalSnapshot],
    staging_relative_path: &Path,
) -> Result<Vec<OsString>, QpdfContractError> {
    if !(PDF_MERGE_MIN_INPUTS..=PDF_MERGE_MAX_INPUTS).contains(&snapshots.len()) {
        return Err(QpdfContractError::InputCount);
    }
    if staging_relative_path != Path::new(MERGED_STAGING_RELATIVE_PATH) {
        return Err(QpdfContractError::StagingPath);
    }

    let mut seen_paths = HashSet::with_capacity(snapshots.len());
    for (index, snapshot) in snapshots.iter().enumerate() {
        let expected_ordinal = u32::try_from(index).map_err(|_| QpdfContractError::OrdinalOrder)?;
        if snapshot.ordinal != expected_ordinal {
            return Err(QpdfContractError::OrdinalOrder);
        }
        if snapshot.relative_path != snapshot_relative_path(expected_ordinal)
            || !snapshot.relative_path.as_os_str().is_ascii()
        {
            return Err(QpdfContractError::SnapshotPath);
        }
        if !seen_paths.insert(snapshot.relative_path.clone()) {
            return Err(QpdfContractError::DuplicateSnapshotPath);
        }
    }

    let mut arguments = [
        "--empty",
        "--suppress-recovery",
        "--stream-data=preserve",
        "--object-streams=preserve",
        "--remove-info",
        "--remove-metadata",
        "--remove-page-labels",
        "--pages",
    ]
    .into_iter()
    .map(OsString::from)
    .collect::<Vec<_>>();
    for snapshot in snapshots {
        let mut argument = OsString::from("--file=");
        argument.push(&snapshot.relative_path);
        arguments.push(argument);
    }
    arguments.push(OsString::from("--"));
    arguments.push(staging_relative_path.as_os_str().to_owned());
    Ok(arguments)
}

pub fn build_lossless_compression_arguments(
    input_relative_path: &Path,
    staging_relative_path: &Path,
) -> Result<Vec<OsString>, QpdfContractError> {
    if input_relative_path != snapshot_relative_path(0)
        || !input_relative_path.as_os_str().is_ascii()
    {
        return Err(QpdfContractError::SnapshotPath);
    }
    if staging_relative_path != Path::new(COMPRESSED_STAGING_RELATIVE_PATH) {
        return Err(QpdfContractError::StagingPath);
    }

    Ok(vec![
        input_relative_path.as_os_str().to_owned(),
        OsString::from("--stream-data=compress"),
        OsString::from("--object-streams=generate"),
        OsString::from("--recompress-flate"),
        OsString::from("--compression-level=9"),
        staging_relative_path.as_os_str().to_owned(),
    ])
}

pub fn interpret_structural_check_exit(
    exit_code: i32,
) -> Result<StructuralCheckOutcome, QpdfContractError> {
    match exit_code {
        0 => Ok(StructuralCheckOutcome::Valid),
        2 | 3 => Ok(StructuralCheckOutcome::Rejected),
        _ => Err(QpdfContractError::UnexpectedExit),
    }
}

pub fn interpret_encryption_check_exit(
    exit_code: i32,
) -> Result<EncryptionCheckOutcome, QpdfContractError> {
    match exit_code {
        0 => Ok(EncryptionCheckOutcome::Encrypted),
        2 => Ok(EncryptionCheckOutcome::Unencrypted),
        _ => Err(QpdfContractError::UnexpectedExit),
    }
}
