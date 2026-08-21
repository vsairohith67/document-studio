mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use document_studio_lib::app_state::{AppState, CancelOutcome};
use document_studio_lib::contracts::{JobState, JobsCreateRequest};
use document_studio_lib::database::Database;
use document_studio_lib::pdf_compression::PdfCompressionService;
use document_studio_lib::pdf_merge::PdfMergeHooks;
use document_studio_lib::qpdf::QpdfRuntimeManager;
use document_studio_lib::workspace::WorkspaceManager;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

fn bundle_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/qpdf/12.3.2")
}

fn qpdf_executable() -> PathBuf {
    bundle_root().join("bin/qpdf.exe")
}

fn service() -> (tempfile::TempDir, AppState, PdfCompressionService) {
    let app_data = tempdir().unwrap();
    let database = Database::open(&app_data.path().join("metadata.sqlite3")).unwrap();
    let workspaces = WorkspaceManager::initialize(app_data.path()).unwrap();
    let state = AppState::new(database, workspaces).with_qpdf(QpdfRuntimeManager::new(
        bundle_root(),
        app_data.path().join("engines"),
    ));
    let service = PdfCompressionService::new(state.clone());
    (app_data, state, service)
}

fn request(input: &Path, destination: &Path, name: &str) -> JobsCreateRequest {
    JobsCreateRequest {
        operation_id: "pdf.compress-lossless".to_owned(),
        input_paths: vec![input.to_string_lossy().into_owned()],
        destination_directory: destination.to_string_lossy().into_owned(),
        requested_output_name: name.to_owned(),
    }
}

fn sha256(path: &Path) -> String {
    Sha256::digest(fs::read(path).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn run_qpdf(arguments: &[String]) -> std::process::Output {
    Command::new(qpdf_executable())
        .args(arguments)
        .output()
        .expect("the accepted qpdf 12.3.2 bundle must run in focused tests")
}

#[test]
fn compressible_pdf_is_verified_published_smaller_and_source_immutable() {
    let (_app_data, state, service) = service();
    let source_directory = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let source =
        support::write_compressible_pdf_fixture(source_directory.path(), "compressible.pdf");
    let source_bytes = fs::read(&source).unwrap();
    let source_hash = sha256(&source);
    let source_size = source_bytes.len() as u64;
    let job = service
        .create_job(request(&source, destination.path(), "compressed.pdf"))
        .unwrap();
    assert_eq!(job.operation_id, "pdf.compress-lossless");
    assert_eq!(job.operation_version, "1.0.0");

    let mut messages = Vec::new();
    let completed = service
        .execute(&job.id, |event| messages.push(event.message_code))
        .unwrap();
    let output = PathBuf::from(completed.outputs[0].final_path.as_ref().unwrap());
    let output_size = fs::metadata(&output).unwrap().len();
    assert_eq!(completed.state, JobState::Completed);
    assert!(messages
        .iter()
        .any(|code| code == "COMPRESSING_PDF_LOSSLESSLY"));
    assert!(output_size < source_size);
    assert_eq!(
        completed.inputs[0].sha256.as_deref(),
        Some(source_hash.as_str())
    );
    assert_eq!(completed.outputs[0].size_bytes, Some(output_size));
    let output_hash = sha256(&output);
    assert_eq!(
        completed.outputs[0].sha256.as_deref(),
        Some(output_hash.as_str())
    );
    assert_eq!(fs::read(&source).unwrap(), source_bytes);
    assert_eq!(sha256(&source), source_hash);
    assert!(!state.workspaces.root().join(&job.id).exists());

    let output_string = output.to_string_lossy().into_owned();
    assert_eq!(
        run_qpdf(&[output_string.clone(), "--check".to_owned()])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        run_qpdf(&[output_string.clone(), "--is-encrypted".to_owned()])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        String::from_utf8(run_qpdf(&[output_string, "--show-npages".to_owned()]).stdout)
            .unwrap()
            .trim(),
        "1"
    );
}

#[test]
fn structural_inventory_metadata_and_signature_field_survive_recompression() {
    let (_app_data, _state, service) = service();
    let source_directory = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let source = support::write_structural_pdf_fixture(source_directory.path(), "structure.pdf");
    let source_hash = sha256(&source);
    let job = service
        .create_job(request(
            &source,
            destination.path(),
            "structure-compressed.pdf",
        ))
        .unwrap();
    let completed = service.execute(&job.id, |_| {}).unwrap();
    let output = PathBuf::from(completed.outputs[0].final_path.as_ref().unwrap());
    assert_eq!(sha256(&source), source_hash);

    let json = run_qpdf(&[
        "--json=2".to_owned(),
        "--json-key=acroform".to_owned(),
        "--json-key=attachments".to_owned(),
        "--json-key=outlines".to_owned(),
        "--json-key=pagelabels".to_owned(),
        output.to_string_lossy().into_owned(),
    ]);
    assert!(
        json.status.success(),
        "{}",
        String::from_utf8_lossy(&json.stderr)
    );
    let inventory: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(inventory["acroform"]["hasacroform"], true);
    assert_eq!(inventory["acroform"]["fields"].as_array().unwrap().len(), 2);
    assert_eq!(inventory["attachments"].as_object().unwrap().len(), 1);
    assert_eq!(inventory["outlines"].as_array().unwrap().len(), 1);
    assert_eq!(inventory["pagelabels"].as_array().unwrap().len(), 1);

    let full_json = run_qpdf(&[
        "--json=2".to_owned(),
        "--json-key=qpdf".to_owned(),
        output.to_string_lossy().into_owned(),
    ]);
    let full_text = String::from_utf8(full_json.stdout).unwrap();
    for preserved in [
        "G04A Structural Fixture",
        "Document Studio",
        "Review note",
        "g04a-note",
        "Approval",
        "note.txt",
    ] {
        assert!(full_text.contains(preserved), "missing {preserved}");
    }
}

#[test]
fn malformed_encrypted_and_changed_sources_fail_closed() {
    let source_directory = tempdir().unwrap();
    let malformed = support::write_fixture(
        source_directory.path(),
        "malformed.pdf",
        b"%PDF-1.7\nbroken",
    );
    let clear = support::write_pdf_fixture(source_directory.path(), "clear.pdf", "CLEAR", 612);
    let encrypted = source_directory.path().join("encrypted.pdf");
    assert!(Command::new(qpdf_executable())
        .arg(&clear)
        .args(["--encrypt", "", "owner", "256", "--"])
        .arg(&encrypted)
        .status()
        .unwrap()
        .success());

    for (input, expected) in [
        (&malformed, "PDF_STRUCTURE_INVALID"),
        (&encrypted, "PDF_ENCRYPTED"),
    ] {
        let (_app_data, _state, service) = service();
        let destination = tempdir().unwrap();
        let job = service
            .create_job(request(input, destination.path(), "rejected.pdf"))
            .unwrap();
        assert_eq!(service.execute(&job.id, |_| {}).unwrap_err().code, expected);
        assert!(!destination.path().join("rejected.pdf").exists());
    }

    let (_app_data, _state, service) = service();
    let destination = tempdir().unwrap();
    let changed = support::write_pdf_fixture(source_directory.path(), "changed.pdf", "OLD", 610);
    let job = service
        .create_job(request(&changed, destination.path(), "changed-output.pdf"))
        .unwrap();
    fs::write(&changed, fs::read(&clear).unwrap()).unwrap();
    assert_eq!(
        service.execute(&job.id, |_| {}).unwrap_err().code,
        "SOURCE_CHANGED"
    );

    let disappeared =
        support::write_pdf_fixture(source_directory.path(), "disappeared.pdf", "GONE", 611);
    let disappeared_job = service
        .create_job(request(
            &disappeared,
            destination.path(),
            "disappeared-output.pdf",
        ))
        .unwrap();
    fs::remove_file(&disappeared).unwrap();
    let disappeared_error = service.execute(&disappeared_job.id, |_| {}).unwrap_err();
    assert!(matches!(
        disappeared_error.code.as_str(),
        "INPUT_UNREADABLE" | "PATH_UNSAFE"
    ));
    assert_eq!(disappeared_error.input_index, Some(0));
    assert!(!destination.path().join("disappeared-output.pdf").exists());
}

#[test]
fn nonzero_timeout_and_output_verification_fail_without_publication() {
    for (name, hooks, expected) in [
        (
            "nonzero.pdf",
            PdfMergeHooks {
                force_qpdf_nonzero_exit: true,
                ..Default::default()
            },
            "QPDF_PROCESS_FAILED",
        ),
        (
            "timeout.pdf",
            PdfMergeHooks {
                qpdf_timeout_override: Some(Duration::ZERO),
                ..Default::default()
            },
            "QPDF_TIMEOUT",
        ),
        (
            "corrupt.pdf",
            PdfMergeHooks {
                corrupt_staging_before_verify: true,
                ..Default::default()
            },
            "OUTPUT_SIZE_INVALID",
        ),
        (
            "publication-failure.pdf",
            PdfMergeHooks {
                fail_before_publication_commit: true,
                ..Default::default()
            },
            "PUBLICATION_FAILED",
        ),
    ] {
        let (_app_data, state, service) = service();
        let source_directory = tempdir().unwrap();
        let destination = tempdir().unwrap();
        let source =
            support::write_pdf_fixture(source_directory.path(), "source.pdf", "SOURCE", 612);
        let job = service
            .create_job(request(&source, destination.path(), name))
            .unwrap();
        assert_eq!(
            service
                .execute_with_hooks(&job.id, |_| {}, hooks)
                .unwrap_err()
                .code,
            expected
        );
        assert!(!destination.path().join(name).exists());
        assert!(!state.workspaces.root().join(&job.id).exists());
    }
}

#[test]
fn cancellation_collision_larger_output_and_recovery_are_truthful_and_safe() {
    let (_app_data, state, service) = service();
    let source_directory = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let source = support::write_pdf_fixture(source_directory.path(), "small.pdf", "SMALL", 612);
    let original = fs::read(&source).unwrap();

    let efficient = source_directory.path().join("efficient.pdf");
    assert!(Command::new(qpdf_executable())
        .arg(&source)
        .args([
            "--stream-data=compress",
            "--object-streams=generate",
            "--recompress-flate",
            "--compression-level=9",
        ])
        .arg(&efficient)
        .status()
        .unwrap()
        .success());
    let efficient_job = service
        .create_job(request(
            &efficient,
            destination.path(),
            "efficient-output.pdf",
        ))
        .unwrap();
    assert_eq!(
        service.execute(&efficient_job.id, |_| {}).unwrap().state,
        JobState::Completed
    );

    let cancel_job = service
        .create_job(request(&source, destination.path(), "cancelled.pdf"))
        .unwrap();
    let worker_service = service.clone();
    let worker_id = cancel_job.id.clone();
    let worker = std::thread::spawn(move || {
        worker_service.execute_with_hooks(
            &worker_id,
            |_| {},
            PdfMergeHooks {
                pause_before_merge: Some(Duration::from_secs(2)),
                ..Default::default()
            },
        )
    });
    let started = Instant::now();
    while state
        .database()
        .get_job(&cancel_job.id)
        .unwrap()
        .unwrap()
        .state
        != JobState::Running
    {
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        state.cancellations.request(&cancel_job.id),
        CancelOutcome::Requested
    );
    state
        .database()
        .request_cancellation(&cancel_job.id)
        .unwrap();
    assert_eq!(worker.join().unwrap().unwrap().state, JobState::Cancelled);
    assert!(!destination.path().join("cancelled.pdf").exists());

    fs::write(destination.path().join("small.pdf"), b"existing user file").unwrap();
    let collision_job = service
        .create_job(request(&source, destination.path(), "small.pdf"))
        .unwrap();
    let collision = service.execute(&collision_job.id, |_| {}).unwrap();
    assert_eq!(
        fs::read(destination.path().join("small.pdf")).unwrap(),
        b"existing user file"
    );
    assert_eq!(
        collision.outputs[0].resolved_name.as_deref(),
        Some("small (1).pdf")
    );
    assert!(collision.outputs[0].size_bytes.unwrap() > original.len() as u64);

    let recovery_job = service
        .create_job(request(&source, destination.path(), "recovery.pdf"))
        .unwrap();
    assert_eq!(
        service
            .execute_with_hooks(
                &recovery_job.id,
                |_| {},
                PdfMergeHooks {
                    fail_cleanup: true,
                    ..Default::default()
                },
            )
            .unwrap_err()
            .code,
        "CLEANUP_FAILED"
    );
    let published = destination.path().join("recovery.pdf");
    let published_hash = sha256(&published);
    assert_eq!(
        document_studio_lib::recovery::reconcile_startup(&state)
            .unwrap()
            .completed_publications,
        1
    );
    assert_eq!(sha256(&published), published_hash);
    assert_eq!(
        state
            .database()
            .get_job(&recovery_job.id)
            .unwrap()
            .unwrap()
            .state,
        JobState::Completed
    );
    assert_eq!(fs::read(&source).unwrap(), original);
}
