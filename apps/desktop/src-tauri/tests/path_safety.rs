mod support;

use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::fs;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use document_studio_lib::path_policy::{
    canonical_directory, canonical_regular_file, ensure_different_files, validate_output_name,
    PathPolicyError,
};
use document_studio_lib::publication::{
    publish_verified_staging, publish_verified_staging_with_observer, PublicationContext,
    PublicationError,
};
use document_studio_lib::workspace::{WorkspaceError, WorkspaceManager};
use tempfile::tempdir;
use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

const JOB_ID: &str = "018f0f17-2f4a-7fb1-a247-101010101010";

fn guard_path(partial_path: &Path) -> PathBuf {
    let mut name = OsString::from(partial_path.file_name().unwrap());
    name.push(".guard");
    partial_path.with_file_name(name)
}

#[test]
fn unsafe_output_names_are_rejected() {
    for name in [
        "",
        ".",
        "..",
        "CON",
        "con.txt",
        "LPT1.pdf",
        "COM9",
        "report. ",
        "report.",
        "..\\escape.txt",
        "folder/file.txt",
        "file.txt:stream",
        "bad?.txt",
        "bad*.txt",
    ] {
        assert!(
            validate_output_name(name).is_err(),
            "{name:?} must be rejected"
        );
    }
    for name in [
        "report-copy.pdf",
        "résumé copy.txt",
        ".document-studio.partial",
    ] {
        validate_output_name(name).unwrap();
    }
}

#[test]
fn relative_traversal_ads_and_device_namespace_paths_are_rejected() {
    assert!(matches!(
        canonical_regular_file(Path::new("..\\outside.txt")),
        Err(PathPolicyError::NotAbsolute)
    ));
    assert!(matches!(
        canonical_regular_file(Path::new("C:\\safe.txt:stream")),
        Err(PathPolicyError::UnsafeNamespace)
    ));
    assert!(matches!(
        canonical_directory(Path::new("\\\\?\\C:\\Windows")),
        Err(PathPolicyError::UnsafeNamespace)
    ));
    assert!(matches!(
        canonical_directory(Path::new("\\\\.\\C:\\Windows")),
        Err(PathPolicyError::UnsafeNamespace)
    ));
}

#[test]
fn junction_escape_is_rejected_without_symlink_privilege() {
    let root = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let junction_path = root.path().join("escape");
    junction::create(outside.path(), &junction_path).unwrap();
    assert!(matches!(
        canonical_directory(&junction_path),
        Err(PathPolicyError::ReparsePoint)
    ));
}

#[test]
fn hard_link_identity_prevents_input_output_collision() {
    let directory = tempdir().unwrap();
    let input = support::write_fixture(directory.path(), "input.bin", b"original");
    let alias = directory.path().join("alias.bin");
    fs::hard_link(&input, &alias).unwrap();
    assert!(matches!(
        ensure_different_files(&input, &alias),
        Err(PathPolicyError::SameFile)
    ));
    assert_eq!(fs::read(input).unwrap(), b"original");
}

#[test]
fn owned_workspace_creation_and_exact_cleanup_succeeds() {
    let app_data = tempdir().unwrap();
    let manager = WorkspaceManager::initialize(app_data.path()).unwrap();
    let workspace = manager.create_job(JOB_ID).unwrap();
    assert!(workspace.inputs.is_dir());
    fs::write(workspace.staging.join("owned.bin"), b"temporary").unwrap();
    assert!(workspace.temporary.is_dir());
    assert!(workspace.audit.is_dir());
    assert!(workspace.root.starts_with(manager.root()));
    manager.cleanup_job(JOB_ID).unwrap();
    assert!(!workspace.root.exists());
}

#[test]
fn workspace_cleanup_refuses_a_tampered_ownership_marker() {
    let app_data = tempdir().unwrap();
    let manager = WorkspaceManager::initialize(app_data.path()).unwrap();
    let workspace = manager.create_job(JOB_ID).unwrap();
    fs::write(
        workspace.root.join(".document-studio-job-v1"),
        b"not-the-job",
    )
    .unwrap();
    assert!(matches!(
        manager.cleanup_job(JOB_ID),
        Err(WorkspaceError::OwnershipMarker)
    ));
    assert!(workspace.root.exists());
}

#[test]
fn publication_uses_deterministic_suffix_and_preserves_existing_file() {
    let source_directory = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source_directory.path(), "input.bin", b"source-data");
    let staging = support::write_fixture(source_directory.path(), "staging.bin", b"source-data");
    fs::write(destination.path().join("report-copy.bin"), b"existing").unwrap();

    let result = publish_verified_staging(
        &staging,
        &input,
        destination.path(),
        "report-copy.bin",
        JOB_ID,
        || false,
        |_completed, _total| {},
    )
    .unwrap();

    assert_eq!(result.resolved_name, "report-copy (1).bin");
    assert_eq!(fs::read(result.final_path).unwrap(), b"source-data");
    assert_eq!(
        fs::read(destination.path().join("report-copy.bin")).unwrap(),
        b"existing"
    );
    assert!(support::partial_files(destination.path()).is_empty());
}

#[test]
fn publication_collision_race_never_replaces_competing_file() {
    let source_directory = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source_directory.path(), "input.bin", b"source-data");
    let staging = support::write_fixture(source_directory.path(), "staging.bin", b"source-data");
    let injected = Cell::new(false);

    let result = publish_verified_staging_with_observer(
        PublicationContext {
            staging_path: &staging,
            input_paths: &[&input],
            destination_directory: destination.path(),
            requested_name: "report-copy.bin",
            job_id: JOB_ID,
        },
        || false,
        |_completed, _total| Ok(()),
        |_candidate, _partial, _resolved_name, _size, _sha256| Ok(()),
        |_partial, _identity| Ok(()),
        |_partial| Ok(()),
        |candidate| {
            if !injected.replace(true) {
                fs::write(candidate, b"competitor")?;
            }
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(
        fs::read(destination.path().join("report-copy.bin")).unwrap(),
        b"competitor"
    );
    assert_eq!(result.resolved_name, "report-copy (1).bin");
    assert_eq!(fs::read(result.final_path).unwrap(), b"source-data");
    assert!(support::partial_files(destination.path()).is_empty());
}

#[test]
fn publication_reserves_the_exact_partial_before_creation() {
    let source_directory = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source_directory.path(), "input.bin", b"journaled");
    let staging = support::write_fixture(source_directory.path(), "staging.bin", b"journaled");
    let reserved = RefCell::new(None);

    let result = publish_verified_staging_with_observer(
        PublicationContext {
            staging_path: &staging,
            input_paths: &[&input],
            destination_directory: destination.path(),
            requested_name: "journaled.bin",
            job_id: JOB_ID,
        },
        || false,
        |_completed, _total| Ok(()),
        |_candidate, partial, _resolved_name, _size, _sha256| {
            assert!(!partial.exists());
            *reserved.borrow_mut() = Some(partial.to_path_buf());
            Ok(())
        },
        |partial, _identity| {
            assert!(!partial.exists());
            Ok(())
        },
        |partial| {
            assert_eq!(reserved.borrow().as_deref(), Some(partial));
            Ok(())
        },
        |_candidate| Ok(()),
    )
    .unwrap();

    assert_eq!(
        reserved.borrow().as_deref(),
        Some(result.owned_partial_path.as_path())
    );
    assert!(!result.owned_partial_path.exists());
}

#[test]
fn generated_partial_that_already_exists_is_preserved() {
    let source_directory = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source_directory.path(), "input.bin", b"source-data");
    let staging = support::write_fixture(source_directory.path(), "staging.bin", b"source-data");
    let preexisting = RefCell::new(None);

    let result = publish_verified_staging_with_observer(
        PublicationContext {
            staging_path: &staging,
            input_paths: &[&input],
            destination_directory: destination.path(),
            requested_name: "existing-partial.bin",
            job_id: JOB_ID,
        },
        || false,
        |_completed, _total| Ok(()),
        |_candidate, partial, _resolved_name, _size, _sha256| {
            if preexisting.borrow().is_none() {
                fs::write(partial, b"pre-existing partial-shaped file")?;
                *preexisting.borrow_mut() = Some(partial.to_path_buf());
            }
            Ok(())
        },
        |_partial, _identity| Ok(()),
        |_partial| Ok(()),
        |_candidate| Ok(()),
    )
    .unwrap();

    let preexisting = preexisting.borrow();
    let preexisting = preexisting.as_ref().unwrap();
    assert_eq!(
        fs::read(preexisting).unwrap(),
        b"pre-existing partial-shaped file"
    );
    assert_eq!(result.resolved_name, "existing-partial (1).bin");
    assert_eq!(fs::read(result.final_path).unwrap(), b"source-data");
}

#[test]
fn guard_create_new_already_exists_is_bounded_and_preserves_the_existing_file() {
    let source_directory = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source_directory.path(), "input.bin", b"guarded");
    let staging = support::write_fixture(source_directory.path(), "staging.bin", b"guarded");
    let preexisting_guard = RefCell::new(None);
    let first_reserved = RefCell::new(None);

    let result = publish_verified_staging_with_observer(
        PublicationContext {
            staging_path: &staging,
            input_paths: &[&input],
            destination_directory: destination.path(),
            requested_name: "guarded.bin",
            job_id: JOB_ID,
        },
        || false,
        |_completed, _total| Ok(()),
        |_candidate, partial, _resolved_name, _size, _sha256| {
            if preexisting_guard.borrow().is_none() {
                let guard = guard_path(partial);
                fs::write(&guard, b"pre-existing guard-shaped file")?;
                *preexisting_guard.borrow_mut() = Some(guard);
                *first_reserved.borrow_mut() = Some(partial.to_path_buf());
            }
            Ok(())
        },
        |partial, _identity| {
            assert_ne!(first_reserved.borrow().as_deref(), Some(partial));
            Ok(())
        },
        |_partial| Ok(()),
        |_candidate| Ok(()),
    )
    .unwrap();

    let guard = preexisting_guard.borrow();
    let guard = guard.as_ref().unwrap();
    assert_eq!(fs::read(guard).unwrap(), b"pre-existing guard-shaped file");
    assert_eq!(result.resolved_name, "guarded (1).bin");
}

#[test]
fn reservation_release_failure_never_deletes_a_preexisting_partial() {
    let source_directory = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source_directory.path(), "input.bin", b"source");
    let staging = support::write_fixture(source_directory.path(), "staging.bin", b"source");
    let preexisting = RefCell::new(None);

    let error = publish_verified_staging_with_observer(
        PublicationContext {
            staging_path: &staging,
            input_paths: &[&input],
            destination_directory: destination.path(),
            requested_name: "release-failure.bin",
            job_id: JOB_ID,
        },
        || false,
        |_completed, _total| Ok(()),
        |_candidate, partial, _resolved_name, _size, _sha256| {
            fs::write(partial, b"must survive")?;
            *preexisting.borrow_mut() = Some(partial.to_path_buf());
            Ok(())
        },
        |_partial, _identity| Ok(()),
        |_partial| {
            Err(PublicationError::Io(std::io::Error::other(
                "injected reservation release failure",
            )))
        },
        |_candidate| Ok(()),
    )
    .unwrap_err();

    assert!(matches!(error, PublicationError::Io(_)));
    let preexisting = preexisting.borrow();
    let preexisting = preexisting.as_ref().unwrap();
    assert_eq!(fs::read(preexisting).unwrap(), b"must survive");
    assert!(!guard_path(preexisting).exists());
}

#[test]
fn failed_activation_removes_the_delete_on_close_guard_before_returning() {
    let source_directory = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source_directory.path(), "input.bin", b"source");
    let staging = support::write_fixture(source_directory.path(), "staging.bin", b"source");
    let reserved = RefCell::new(None);

    let error = publish_verified_staging_with_observer(
        PublicationContext {
            staging_path: &staging,
            input_paths: &[&input],
            destination_directory: destination.path(),
            requested_name: "activation-crash.bin",
            job_id: JOB_ID,
        },
        || false,
        |_completed, _total| Ok(()),
        |_candidate, partial, _resolved_name, _size, _sha256| {
            *reserved.borrow_mut() = Some(partial.to_path_buf());
            Ok(())
        },
        |partial, _identity| {
            assert!(!partial.exists());
            assert!(guard_path(partial).exists());
            Err(PublicationError::Io(std::io::Error::other(
                "simulated crash before durable activation",
            )))
        },
        |_partial| panic!("a crash window does not run reservation release"),
        |_candidate| Ok(()),
    )
    .unwrap_err();

    assert!(matches!(error, PublicationError::Io(_)));
    let reserved = reserved.borrow();
    let reserved = reserved.as_ref().unwrap();
    assert!(!reserved.exists());
    assert!(!guard_path(reserved).exists());
}

#[test]
fn collision_retries_are_bounded_and_exhaust_without_overwrite_or_partial() {
    let source_directory = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source_directory.path(), "input.bin", b"");
    let staging = support::write_fixture(source_directory.path(), "staging.bin", b"");
    let collisions = Cell::new(0_u32);

    let error = publish_verified_staging_with_observer(
        PublicationContext {
            staging_path: &staging,
            input_paths: &[&input],
            destination_directory: destination.path(),
            requested_name: "bounded.bin",
            job_id: JOB_ID,
        },
        || false,
        |_completed, _total| Ok(()),
        |_candidate, _partial, _resolved_name, _size, _sha256| Ok(()),
        |_partial, _identity| Ok(()),
        |_partial| Ok(()),
        |candidate| {
            collisions.set(collisions.get() + 1);
            fs::write(candidate, b"competitor")?;
            Ok(())
        },
    )
    .unwrap_err();

    assert!(matches!(error, PublicationError::CollisionExhausted));
    assert_eq!(collisions.get(), 1000);
    assert!(support::partial_files(destination.path()).is_empty());
    for attempt in 0..1000 {
        let name = document_studio_lib::publication::collision_name("bounded.bin", attempt);
        assert_eq!(
            fs::read(destination.path().join(name)).unwrap(),
            b"competitor"
        );
    }
}

#[test]
fn collision_cleanup_failure_overrides_exhaustion_and_retains_exact_partial() {
    let source_directory = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let input = support::write_fixture(source_directory.path(), "input.bin", b"cleanup");
    let staging = support::write_fixture(source_directory.path(), "staging.bin", b"cleanup");
    let owned = RefCell::new(None);
    let lock = RefCell::new(None);

    let error = publish_verified_staging_with_observer(
        PublicationContext {
            staging_path: &staging,
            input_paths: &[&input],
            destination_directory: destination.path(),
            requested_name: "cleanup.bin",
            job_id: JOB_ID,
        },
        || false,
        |_completed, _total| Ok(()),
        |_candidate, partial, _resolved_name, _size, _sha256| {
            *owned.borrow_mut() = Some(partial.to_path_buf());
            Ok(())
        },
        |_partial, _identity| Ok(()),
        |_partial| panic!("ownership must not clear when deletion fails"),
        |candidate| {
            let partial = owned.borrow();
            let partial = partial.as_ref().unwrap();
            *lock.borrow_mut() = Some(
                fs::OpenOptions::new()
                    .read(true)
                    .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                    .open(partial)?,
            );
            fs::write(candidate, b"competitor")?;
            Ok(())
        },
    )
    .unwrap_err();

    assert!(matches!(error, PublicationError::Cleanup(_)));
    let partial = owned.borrow();
    let partial = partial.as_ref().unwrap();
    assert!(partial.exists());
    assert_eq!(
        fs::read(destination.path().join("cleanup.bin")).unwrap(),
        b"competitor"
    );
    drop(lock.borrow_mut().take());
    fs::remove_file(partial).unwrap();
}

#[test]
fn cancellation_during_destination_copy_removes_the_owned_partial() {
    let source_directory = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let bytes = vec![0x5a; 3 * 1024 * 1024];
    let input = support::write_fixture(source_directory.path(), "input.bin", &bytes);
    let staging = support::write_fixture(source_directory.path(), "staging.bin", &bytes);
    let cancelled = Cell::new(false);

    let result = publish_verified_staging(
        &staging,
        &input,
        destination.path(),
        "report-copy.bin",
        JOB_ID,
        || cancelled.get(),
        |_completed, _total| cancelled.set(true),
    );
    assert!(matches!(result, Err(PublicationError::Cancelled)));
    assert!(!destination.path().join("report-copy.bin").exists());
    assert!(support::partial_files(destination.path()).is_empty());
}
