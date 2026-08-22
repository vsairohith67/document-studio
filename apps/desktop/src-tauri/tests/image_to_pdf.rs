use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use document_studio_lib::app_state::AppState;
use document_studio_lib::contracts::{JobState, JobsCreateRequest, OperationStage};
use document_studio_lib::database::Database;
use document_studio_lib::image_to_pdf::ImageToPdfService;
use document_studio_lib::qpdf::QpdfRuntimeManager;
use document_studio_lib::recovery::reconcile_startup;
use document_studio_lib::workspace::WorkspaceManager;
use flate2::read::ZlibDecoder;
use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder, ImageFormat, Rgba, RgbaImage};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use tempfile::tempdir;

fn bundle_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/qpdf/12.3.2")
}

fn qpdf_executable() -> PathBuf {
    bundle_root().join("bin/qpdf.exe")
}

fn service() -> (tempfile::TempDir, AppState, ImageToPdfService) {
    let app_data = tempdir().unwrap();
    let database = Database::open(&app_data.path().join("metadata.sqlite3")).unwrap();
    let workspaces = WorkspaceManager::initialize(app_data.path()).unwrap();
    let state = AppState::new(database, workspaces).with_qpdf(QpdfRuntimeManager::new(
        bundle_root(),
        app_data.path().join("engines"),
    ));
    let service = ImageToPdfService::new(state.clone());
    (app_data, state, service)
}

fn write_image(
    directory: &Path,
    name: &str,
    format: ImageFormat,
    width: u32,
    height: u32,
    rgba: [u8; 4],
) -> PathBuf {
    let path = directory.join(name);
    let image = RgbaImage::from_pixel(width, height, Rgba(rgba));
    if format == ImageFormat::Jpeg {
        image::DynamicImage::ImageRgba8(image)
            .to_rgb8()
            .save_with_format(&path, format)
            .unwrap();
    } else {
        image.save_with_format(&path, format).unwrap();
    }
    path
}

fn write_oriented_icc_jpeg(directory: &Path, name: &str, width: u32, height: u32) -> PathBuf {
    let path = directory.join(name);
    let mut bytes = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut bytes, 90);
    // Little-endian TIFF IFD with one SHORT orientation entry set to 6 (rotate 90°).
    encoder
        .set_exif_metadata(vec![
            0x49, 0x49, 0x2a, 0x00, 0x08, 0x00, 0x00, 0x00, 0x01, 0x00, 0x12, 0x01, 0x03, 0x00,
            0x01, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ])
        .unwrap();
    encoder
        .set_icc_profile(b"document-studio-test-profile".to_vec())
        .unwrap();
    encoder
        .write_image(
            &vec![64_u8; width as usize * height as usize * 3],
            width,
            height,
            ExtendedColorType::Rgb8,
        )
        .unwrap();
    fs::write(&path, bytes).unwrap();
    path
}

fn request(inputs: &[PathBuf], destination: &Path, name: &str) -> JobsCreateRequest {
    JobsCreateRequest {
        operation_id: "image.to-pdf".to_owned(),
        input_paths: inputs
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
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

fn decoded_flate_streams(pdf: &[u8]) -> Vec<Vec<u8>> {
    let mut decoded = Vec::new();
    let mut offset = 0;
    while let Some(relative_start) = pdf[offset..]
        .windows(7)
        .position(|part| part == b"stream\n")
    {
        let start = offset + relative_start + 7;
        let Some(relative_end) = pdf[start..]
            .windows(10)
            .position(|part| part == b"\nendstream")
        else {
            break;
        };
        let end = start + relative_end;
        let mut output = Vec::new();
        if ZlibDecoder::new(&pdf[start..end])
            .read_to_end(&mut output)
            .is_ok()
        {
            decoded.push(output);
        }
        offset = end + 10;
    }
    decoded
}

fn advance_to_running(state: &AppState, job_id: &str) {
    for (expected, next, stage) in [
        (
            JobState::Queued,
            JobState::Inspecting,
            OperationStage::Inspect,
        ),
        (
            JobState::Inspecting,
            JobState::Preflight,
            OperationStage::Preflight,
        ),
        (JobState::Preflight, JobState::Ready, OperationStage::Plan),
        (JobState::Ready, JobState::Running, OperationStage::Execute),
    ] {
        let current = state.database().get_job(job_id).unwrap().unwrap();
        state
            .database()
            .transition_job(job_id, expected, current.version, next, Some(stage))
            .unwrap();
    }
}

#[test]
fn jpeg_png_and_webp_publish_one_verified_page_each_in_selected_order() {
    let (_app_data, state, service) = service();
    let sources = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let png_named_jpg = write_image(
        sources.path(),
        "content-not-extension.jpg",
        ImageFormat::Png,
        20,
        10,
        [255, 0, 0, 255],
    );
    let webp = write_image(
        sources.path(),
        "second.webp",
        ImageFormat::WebP,
        7,
        13,
        [0, 255, 0, 128],
    );
    let jpeg = write_oriented_icc_jpeg(sources.path(), "third.jpeg", 11, 9);
    let ordered = vec![png_named_jpg, webp, jpeg];
    let source_hashes = ordered.iter().map(|path| sha256(path)).collect::<Vec<_>>();
    let job = service
        .create_job(request(&ordered, destination.path(), "images.pdf"))
        .unwrap();
    let spec = state
        .database()
        .get_operation_spec(&job.id)
        .unwrap()
        .unwrap();
    assert_eq!(spec.envelope.operation_id, "image.to-pdf");
    assert_eq!(
        spec.envelope.settings["pageSizing"],
        "one-point-per-oriented-pixel"
    );

    let mut messages = Vec::new();
    let completed = service
        .execute(&job.id, |event| messages.push(event.message_code))
        .unwrap();
    assert_eq!(completed.state, JobState::Completed);
    assert!(messages.iter().any(|code| code == "WRITING_IMAGE_PAGES"));
    let output = PathBuf::from(completed.outputs[0].final_path.as_ref().unwrap());
    if let Some(evidence_directory) = std::env::var_os("DOCUMENT_STUDIO_G04B_VISUAL_EVIDENCE_DIR") {
        let evidence_directory = PathBuf::from(evidence_directory);
        fs::create_dir_all(&evidence_directory).unwrap();
        fs::copy(&ordered[0], evidence_directory.join("source.png")).unwrap();
        fs::copy(&output, evidence_directory.join("output.pdf")).unwrap();
    }
    let bytes = fs::read(&output).unwrap();
    assert!(bytes.starts_with(b"%PDF-1.4"));
    assert!(bytes.windows(6).any(|window| window == b"/SMask"));
    let text = String::from_utf8_lossy(&bytes);
    let first = text.find("/MediaBox [0 0 20 10]").unwrap();
    let second = text.find("/MediaBox [0 0 7 13]").unwrap();
    let third = text.find("/MediaBox [0 0 9 11]").unwrap();
    assert!(first < second && second < third);
    let decoded_streams = decoded_flate_streams(&bytes);
    assert!(decoded_streams.contains(&[255_u8, 0, 0].repeat(20 * 10)));
    assert!(decoded_streams.contains(&vec![128_u8; 7 * 13]));
    assert!(decoded_streams
        .iter()
        .any(|stream| stream.len() == 9 * 11 * 3));
    assert_eq!(
        completed
            .inputs
            .iter()
            .map(|input| input.sha256.clone().unwrap())
            .collect::<Vec<_>>(),
        source_hashes
    );
    assert!(ordered
        .iter()
        .zip(source_hashes.iter())
        .all(|(path, expected)| sha256(path) == *expected));
    assert!(!state.workspaces.root().join(&job.id).exists());
    let warnings = state.database().list_warnings(&job.id).unwrap();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].code, "ICC_PROFILE_NOT_RETAINED");
    assert_eq!(warnings[0].input_index, Some(2));
    assert!(!warnings[0].sanitized_detail.contains("third.jpeg"));

    let qpdf = Command::new(qpdf_executable())
        .arg(&output)
        .arg("--check")
        .output()
        .unwrap();
    assert!(
        qpdf.status.success(),
        "{}",
        String::from_utf8_lossy(&qpdf.stderr)
    );
    let pages = Command::new(qpdf_executable())
        .arg(&output)
        .arg("--show-npages")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(pages.stdout).unwrap().trim(), "3");
}

#[test]
fn malformed_content_cancellation_and_collision_fail_or_publish_truthfully() {
    let sources = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let malformed = sources.path().join("malformed.png");
    fs::write(&malformed, b"not an image").unwrap();
    let (_app_data, _state, malformed_service) = service();
    assert_eq!(
        malformed_service
            .create_job(request(&[malformed], destination.path(), "bad.pdf"))
            .unwrap_err()
            .code,
        "UNSUPPORTED_IMAGE"
    );
    let oversized = write_image(
        sources.path(),
        "oversized.png",
        ImageFormat::Png,
        8193,
        1,
        [1, 2, 3, 255],
    );
    assert_eq!(
        malformed_service
            .create_job(request(&[oversized], destination.path(), "oversized.pdf"))
            .unwrap_err()
            .code,
        "IMAGE_RESOURCE_LIMIT"
    );

    let source = write_image(
        sources.path(),
        "source.png",
        ImageFormat::Png,
        8,
        8,
        [10, 20, 30, 255],
    );
    let (_app_data, state, service) = service();
    let cancelled = service
        .create_job(request(
            std::slice::from_ref(&source),
            destination.path(),
            "cancelled.pdf",
        ))
        .unwrap();
    let token = state.cancellations.register(&cancelled.id);
    assert!(matches!(
        state.cancellations.request(&cancelled.id),
        document_studio_lib::app_state::CancelOutcome::Requested
    ));
    let result = service
        .execute_with_registered_token(&cancelled.id, token, |_| {})
        .unwrap();
    assert_eq!(result.state, JobState::Cancelled);
    assert!(!destination.path().join("cancelled.pdf").exists());

    let late_cancelled = service
        .create_job(request(
            std::slice::from_ref(&source),
            destination.path(),
            "late-cancelled.pdf",
        ))
        .unwrap();
    let mut cancellation_requested = false;
    let late_result = service
        .execute(&late_cancelled.id, |event| {
            if !cancellation_requested
                && event.message_code == "WRITING_IMAGE_PAGES"
                && event.completed_units == 1
            {
                assert!(matches!(
                    state.cancellations.request(&late_cancelled.id),
                    document_studio_lib::app_state::CancelOutcome::Requested
                ));
                cancellation_requested = true;
            }
        })
        .unwrap();
    assert!(cancellation_requested);
    assert_eq!(late_result.state, JobState::Cancelled);
    assert!(!destination.path().join("late-cancelled.pdf").exists());
    assert!(!state.workspaces.root().join(&late_cancelled.id).exists());

    fs::write(destination.path().join("images.pdf"), b"existing").unwrap();
    let collision = service
        .create_job(request(&[source], destination.path(), "images.pdf"))
        .unwrap();
    let completed = service.execute(&collision.id, |_| {}).unwrap();
    assert_eq!(completed.state, JobState::Completed);
    assert_eq!(
        completed.outputs[0].resolved_name.as_deref(),
        Some("images (1).pdf")
    );
    assert_eq!(
        fs::read(destination.path().join("images.pdf")).unwrap(),
        b"existing"
    );
}

#[test]
fn altered_persisted_settings_fail_closed_before_conversion() {
    let (app_data, state, service) = service();
    let sources = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let source = write_image(
        sources.path(),
        "source.png",
        ImageFormat::Png,
        4,
        4,
        [10, 20, 30, 255],
    );
    let job = service
        .create_job(request(&[source], destination.path(), "altered.pdf"))
        .unwrap();
    let altered = serde_json::to_string(&serde_json::json!({
        "schemaVersion": 1,
        "operationId": "image.to-pdf",
        "settings": {
            "alphaPolicy": "discard",
            "colorProfilePolicy": "discard-profile-use-decoded-device-rgb-with-warning",
            "compression": "flate-lossless",
            "pageSizing": "one-point-per-oriented-pixel",
            "sourceOrder": "selected-order"
        }
    }))
    .unwrap();
    let altered_sha = Sha256::digest(altered.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let connection = Connection::open(app_data.path().join("metadata.sqlite3")).unwrap();
    connection
        .execute(
            "UPDATE job_operation_specs
             SET settings_json = ?1, settings_sha256 = ?2
             WHERE job_id = ?3",
            params![altered, altered_sha, job.id],
        )
        .unwrap();

    let error = service.execute(&job.id, |_| {}).unwrap_err();
    assert_eq!(error.code, "METADATA_WRITE_FAILED");
    assert_eq!(
        state.database().get_job(&job.id).unwrap().unwrap().state,
        JobState::Failed
    );
    assert!(!destination.path().join("altered.pdf").exists());
}

#[test]
fn count_boundary_publication_failure_and_startup_recovery_are_truthful() {
    let sources = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let source = write_image(
        sources.path(),
        "boundary.png",
        ImageFormat::Png,
        2,
        2,
        [12, 34, 56, 255],
    );
    let (_app_data, state, service) = service();
    let too_many = vec![source.clone(); 129];
    assert_eq!(
        service
            .create_job(request(&too_many, destination.path(), "too-many.pdf"))
            .unwrap_err()
            .code,
        "INVALID_INPUT_COUNT"
    );

    let maximum = vec![source.clone(); 128];
    let maximum_job = service
        .create_job(request(&maximum, destination.path(), "maximum.pdf"))
        .unwrap();
    let maximum_result = service.execute(&maximum_job.id, |_| {}).unwrap();
    assert_eq!(maximum_result.state, JobState::Completed);
    let maximum_output = PathBuf::from(maximum_result.outputs[0].final_path.as_ref().unwrap());
    let pages = Command::new(qpdf_executable())
        .arg(&maximum_output)
        .arg("--show-npages")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(pages.stdout).unwrap().trim(), "128");
    assert_eq!(fs::read_dir(destination.path()).unwrap().count(), 1);

    let publication_destination = tempdir().unwrap();
    let publication_path = publication_destination.path().to_path_buf();
    let publication_job = service
        .create_job(request(
            std::slice::from_ref(&source),
            &publication_path,
            "publication-failure.pdf",
        ))
        .unwrap();
    let mut removed = false;
    let error = service
        .execute(&publication_job.id, |event| {
            if !removed && event.message_code == "PREPARING_IMAGE_PDF_PUBLICATION" {
                fs::remove_dir(&publication_path).unwrap();
                removed = true;
            }
        })
        .unwrap_err();
    assert_eq!(error.code, "IMAGE_PDF_PUBLICATION_FAILED");
    let failed = state
        .database()
        .get_job(&publication_job.id)
        .unwrap()
        .unwrap();
    assert_eq!(failed.state, JobState::Failed);
    assert!(!state.workspaces.root().join(&publication_job.id).exists());

    let recovery_destination = tempdir().unwrap();
    let recovery_job = service
        .create_job(request(
            std::slice::from_ref(&source),
            recovery_destination.path(),
            "recovery.pdf",
        ))
        .unwrap();
    let workspace = state.workspaces.create_job(&recovery_job.id).unwrap();
    fs::write(workspace.staging.join("abandoned.pdf"), b"temporary").unwrap();
    advance_to_running(&state, &recovery_job.id);
    let report = reconcile_startup(&state).unwrap();
    assert_eq!(report.failed, 1);
    assert!(!workspace.root.exists());
    let recovered = state.database().get_job(&recovery_job.id).unwrap().unwrap();
    assert_eq!(recovered.state, JobState::Failed);
    assert!(!recovery_destination.path().join("recovery.pdf").exists());
}
