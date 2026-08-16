mod support;

use std::cell::Cell;
use std::fs;
use std::path::Path;

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

const JOB_ID: &str = "018f0f17-2f4a-7fb1-a247-101010101010";

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
    fs::write(workspace.staging.join("owned.bin"), b"temporary").unwrap();
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
            input_path: &input,
            destination_directory: destination.path(),
            requested_name: "report-copy.bin",
            job_id: JOB_ID,
        },
        || false,
        |_completed, _total| Ok(()),
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
