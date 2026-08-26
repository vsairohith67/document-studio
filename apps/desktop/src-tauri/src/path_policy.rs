use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::{Component, Path, PathBuf, Prefix};

use thiserror::Error;
use windows_sys::Win32::Globalization::{CompareStringOrdinal, CSTR_EQUAL};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use crate::windows_security::{file_identity, FileIdentity};

#[derive(Debug, Error)]
pub enum PathPolicyError {
    #[error("the selected path is not absolute")]
    NotAbsolute,
    #[error("device, namespace, UNC, and alternate-stream paths are not allowed")]
    UnsafeNamespace,
    #[error("the selected path does not exist")]
    Missing,
    #[error("the selected input must be a regular file")]
    NotRegularFile,
    #[error("the selected destination must be a directory")]
    NotDirectory,
    #[error("reparse points are not allowed in selected paths")]
    ReparsePoint,
    #[error("the output file name is not safe on Windows")]
    UnsafeFileName,
    #[error("the input and output resolve to the same file")]
    SameFile,
    #[error("filesystem validation failed")]
    Io(#[from] std::io::Error),
}

pub fn canonical_regular_file(path: &Path) -> Result<(PathBuf, FileIdentity), PathPolicyError> {
    validate_existing_path(path)?;
    let metadata = fs::metadata(path).map_err(map_missing)?;
    if !metadata.is_file() {
        return Err(PathPolicyError::NotRegularFile);
    }
    let canonical = canonicalize_local(path)?;
    reject_unsafe_syntax(&canonical)?;
    reject_reparse_components(&canonical)?;
    Ok((canonical.clone(), file_identity(&canonical)?))
}

pub fn canonical_directory(path: &Path) -> Result<PathBuf, PathPolicyError> {
    validate_existing_path(path)?;
    let metadata = fs::metadata(path).map_err(map_missing)?;
    if !metadata.is_dir() {
        return Err(PathPolicyError::NotDirectory);
    }
    let canonical = canonicalize_local(path)?;
    reject_unsafe_syntax(&canonical)?;
    reject_reparse_components(&canonical)?;
    Ok(canonical)
}

pub fn validate_output_name(name: &str) -> Result<(), PathPolicyError> {
    if name.is_empty()
        || name.encode_utf16().count() > 255
        || name == "."
        || name == ".."
        || name.ends_with(['.', ' '])
        || name.chars().any(|character| {
            character < ' '
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
        })
    {
        return Err(PathPolicyError::UnsafeFileName);
    }

    let device_stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    let reserved_number = |suffix: Option<&str>| {
        matches!(
            suffix,
            Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        )
    };
    if matches!(device_stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || reserved_number(device_stem.strip_prefix("COM"))
        || reserved_number(device_stem.strip_prefix("LPT"))
    {
        return Err(PathPolicyError::UnsafeFileName);
    }
    Ok(())
}

pub fn windows_file_names_equal(left: &str, right: &str) -> bool {
    let left: Vec<u16> = left.encode_utf16().collect();
    let right: Vec<u16> = right.encode_utf16().collect();
    // SAFETY: both slices remain alive for the call and their explicit lengths exclude
    // terminators, which is the contract required by CompareStringOrdinal.
    unsafe {
        CompareStringOrdinal(
            left.as_ptr(),
            i32::try_from(left.len()).unwrap_or(i32::MAX),
            right.as_ptr(),
            i32::try_from(right.len()).unwrap_or(i32::MAX),
            1,
        ) == CSTR_EQUAL
    }
}

pub fn ensure_different_files(input: &Path, output: &Path) -> Result<(), PathPolicyError> {
    if !output.exists() {
        return Ok(());
    }
    if file_identity(input)? == file_identity(output)? {
        return Err(PathPolicyError::SameFile);
    }
    Ok(())
}

pub fn reject_reparse_components(path: &Path) -> Result<(), PathPolicyError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_) | Component::RootDir) {
            continue;
        }
        let metadata = fs::symlink_metadata(&current).map_err(map_missing)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(PathPolicyError::ReparsePoint);
        }
    }
    Ok(())
}

fn validate_existing_path(path: &Path) -> Result<(), PathPolicyError> {
    reject_unsafe_syntax(path)?;
    reject_reparse_components(path)
}

fn reject_unsafe_syntax(path: &Path) -> Result<(), PathPolicyError> {
    if !path.is_absolute() {
        return Err(PathPolicyError::NotAbsolute);
    }
    let mut components = path.components();
    match components.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(_) => {}
            Prefix::Verbatim(_)
            | Prefix::VerbatimUNC(_, _)
            | Prefix::VerbatimDisk(_)
            | Prefix::DeviceNS(_)
            | Prefix::UNC(_, _) => return Err(PathPolicyError::UnsafeNamespace),
        },
        _ => return Err(PathPolicyError::NotAbsolute),
    }
    for component in components {
        if let Component::Normal(name) = component {
            let text = name.to_string_lossy();
            if text.contains(':') {
                return Err(PathPolicyError::UnsafeNamespace);
            }
        }
        if matches!(component, Component::ParentDir) {
            return Err(PathPolicyError::UnsafeNamespace);
        }
    }
    Ok(())
}

fn map_missing(error: std::io::Error) -> PathPolicyError {
    if error.kind() == std::io::ErrorKind::NotFound {
        PathPolicyError::Missing
    } else {
        PathPolicyError::Io(error)
    }
}

fn canonicalize_local(path: &Path) -> Result<PathBuf, PathPolicyError> {
    let canonical = fs::canonicalize(path)?;
    let text = canonical.as_os_str().to_string_lossy();
    if let Some(local) = text.strip_prefix(r"\\?\") {
        if local.starts_with("UNC\\") {
            return Err(PathPolicyError::UnsafeNamespace);
        }
        return Ok(PathBuf::from(local));
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::{validate_output_name, windows_file_names_equal};

    #[test]
    fn windows_ordinal_ignore_case_handles_ascii_and_unicode_without_locale_folding() {
        assert!(windows_file_names_equal("REPORT.pdf", "report.PDF"));
        assert!(windows_file_names_equal("ЖУРНАЛ.pdf", "журнал.PDF"));
        assert!(!windows_file_names_equal("I.pdf", "ı.pdf"));
        assert!(!windows_file_names_equal("resume.pdf", "résumé.pdf"));
    }

    #[test]
    fn output_component_limit_is_exact_utf16_units() {
        assert!(validate_output_name(&format!("{}.pdf", "a".repeat(251))).is_ok());
        assert!(validate_output_name(&format!("{}.pdf", "a".repeat(252))).is_err());
        assert!(validate_output_name(&format!("{}a.pdf", "😀".repeat(125))).is_ok());
        assert!(validate_output_name(&format!("{}aa.pdf", "😀".repeat(125))).is_err());
    }
}
