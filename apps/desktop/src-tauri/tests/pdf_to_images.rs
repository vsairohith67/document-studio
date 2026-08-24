mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Barrier};

use document_studio_lib::app_state::{AppState, CancelOutcome};
use document_studio_lib::contracts::{
    JobState, OperationError, OperationStage, PdfImageFormat, PdfToImagesJobCreateRequest,
    PdfToImagesPagePlan, ViewerSessionRequest,
};
use document_studio_lib::database::Database;
use document_studio_lib::pdf_to_images::{
    encode_and_verify_pixels, PdfToImagesHooks, PdfToImagesService, PixelUploadMetadata,
};
use document_studio_lib::qpdf::QpdfRuntimeManager;
use document_studio_lib::recovery::reconcile_startup;
use document_studio_lib::workspace::WorkspaceManager;
use image::ImageFormat;
use tempfile::tempdir;

fn bundle_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/qpdf/12.3.2")
}

fn service() -> (tempfile::TempDir, AppState, PdfToImagesService) {
    let app_data = tempdir().unwrap();
    let database = Database::open(&app_data.path().join("metadata.sqlite3")).unwrap();
    let workspaces = WorkspaceManager::initialize(app_data.path()).unwrap();
    let state = AppState::new(database, workspaces).with_qpdf(QpdfRuntimeManager::new(
        bundle_root(),
        app_data.path().join("engines"),
    ));
    let service = PdfToImagesService::new(state.clone());
    (app_data, state, service)
}

fn request(
    state: &AppState,
    source: &Path,
    destination: &Path,
    page_count: u32,
    pages: Vec<PdfToImagesPagePlan>,
    format: PdfImageFormat,
    dpi: u16,
) -> PdfToImagesJobCreateRequest {
    let viewer = state.viewer_sessions.open_pdf(source).unwrap();
    let destination = state
        .viewer_sessions
        .grant_destination(destination)
        .unwrap();
    PdfToImagesJobCreateRequest {
        viewer_session_id: viewer.session_id,
        viewer_generation: viewer.generation,
        destination_grant_id: destination.grant_id,
        source_page_count: page_count,
        pages,
        format,
        dpi,
        output_stem: "report".to_owned(),
    }
}

fn upload(
    service: &PdfToImagesService,
    session: &document_studio_lib::contracts::PdfToImagesJobSession,
    ordinal: usize,
    rgba: Vec<u8>,
) -> Result<document_studio_lib::contracts::JobRecord, document_studio_lib::contracts::OperationError>
{
    let ticket = &session.pages[ordinal];
    service.submit_page(
        PixelUploadMetadata {
            job_id: session.job.id.clone(),
            render_session_id: session.render_session_id.clone(),
            page_ordinal: ticket.page_ordinal,
            nonce: ticket.nonce.clone(),
            expected_width: ticket.expected_width,
            expected_height: ticket.expected_height,
        },
        rgba,
        |_| {},
    )
}

#[test]
fn png_jpeg_and_lossless_webp_are_encoded_verified_and_published() {
    for (format, expected) in [
        (PdfImageFormat::Png, ImageFormat::Png),
        (PdfImageFormat::Jpeg, ImageFormat::Jpeg),
        (PdfImageFormat::Webp, ImageFormat::WebP),
    ] {
        let (_app_data, state, service) = service();
        let source_dir = tempdir().unwrap();
        let destination = tempdir().unwrap();
        let source = support::write_pdf_fixture(source_dir.path(), "input.pdf", "G04B2", 612);
        let request = request(
            &state,
            &source,
            destination.path(),
            1,
            vec![PdfToImagesPagePlan {
                source_page_index: 0,
                width: 2,
                height: 2,
            }],
            format,
            150,
        );
        let session = service.create_job(request).unwrap();
        let completed = upload(&service, &session, 0, [40, 90, 140, 255].repeat(4)).unwrap();
        assert_eq!(completed.state, JobState::Completed);
        assert_eq!(completed.outputs.len(), 1);
        assert_eq!(completed.outputs[0].status.as_str(), "published");
        let output = completed.outputs[0].final_path.as_ref().unwrap();
        let bytes = std::fs::read(output).unwrap();
        assert_eq!(image::guess_format(&bytes).unwrap(), expected);
        let decoded = image::open(output).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (2, 2));
        assert!(!source.metadata().unwrap().permissions().readonly());
    }
}

#[test]
fn png_and_jpeg_record_each_supported_density() {
    for dpi in [72_u16, 150, 300] {
        for format in [PdfImageFormat::Png, PdfImageFormat::Jpeg] {
            let output = tempdir().unwrap();
            let path = output
                .path()
                .join(format!("density-{dpi}.{}", format.extension()));
            encode_and_verify_pixels(
                &path,
                &[40, 90, 140, 200, 120, 60, 30, 210, 100, 180, 80, 220],
                2,
                2,
                format,
                dpi,
            )
            .unwrap();
            let bytes = std::fs::read(path).unwrap();
            if format == PdfImageFormat::Png {
                let expected = (f64::from(dpi) / 0.0254).round() as u32;
                let marker = bytes
                    .windows(4)
                    .position(|window| window == b"pHYs")
                    .unwrap();
                assert_eq!(
                    u32::from_be_bytes(bytes[marker + 4..marker + 8].try_into().unwrap()),
                    expected
                );
                assert_eq!(
                    u32::from_be_bytes(bytes[marker + 8..marker + 12].try_into().unwrap()),
                    expected
                );
                assert_eq!(bytes[marker + 12], 1);
            } else {
                assert_eq!(bytes[13], 1);
                assert_eq!(u16::from_be_bytes([bytes[14], bytes[15]]), dpi);
                assert_eq!(u16::from_be_bytes([bytes[16], bytes[17]]), dpi);
            }
        }
    }
}

#[test]
fn selected_subset_order_drives_deterministic_output_rows() {
    let (_app_data, state, service) = service();
    let source_dir = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let source =
        support::write_multi_page_pdf_fixture(source_dir.path(), "ordered.pdf", "G04B2", 3);
    let session = service
        .create_job(request(
            &state,
            &source,
            destination.path(),
            3,
            vec![
                PdfToImagesPagePlan {
                    source_page_index: 2,
                    width: 1,
                    height: 1,
                },
                PdfToImagesPagePlan {
                    source_page_index: 0,
                    width: 1,
                    height: 1,
                },
            ],
            PdfImageFormat::Png,
            72,
        ))
        .unwrap();
    let running = upload(&service, &session, 0, vec![255, 0, 0, 255]).unwrap();
    assert_eq!(running.state, JobState::Running);
    let completed = upload(&service, &session, 1, vec![0, 0, 255, 255]).unwrap();
    assert_eq!(completed.state, JobState::Completed);
    assert_eq!(completed.outputs[0].requested_name, "report-page-0001.png");
    assert_eq!(completed.outputs[1].requested_name, "report-page-0002.png");
    assert_eq!(
        image::open(completed.outputs[0].final_path.as_ref().unwrap())
            .unwrap()
            .to_rgb8()[(0, 0)],
        image::Rgb([255, 0, 0])
    );
    assert_eq!(
        image::open(completed.outputs[1].final_path.as_ref().unwrap())
            .unwrap()
            .to_rgb8()[(0, 0)],
        image::Rgb([0, 0, 255])
    );
}

#[test]
fn stale_wrong_page_replay_and_payload_mismatch_fail_closed() {
    for mode in ["wrong-page", "payload", "alpha"] {
        let (_app_data, state, service) = service();
        let source_dir = tempdir().unwrap();
        let destination = tempdir().unwrap();
        let source = support::write_pdf_fixture(source_dir.path(), "input.pdf", "G04B2", 612);
        let session = service
            .create_job(request(
                &state,
                &source,
                destination.path(),
                1,
                vec![PdfToImagesPagePlan {
                    source_page_index: 0,
                    width: 1,
                    height: 1,
                }],
                PdfImageFormat::Png,
                72,
            ))
            .unwrap();
        let ticket = &session.pages[0];
        let metadata = PixelUploadMetadata {
            job_id: session.job.id.clone(),
            render_session_id: session.render_session_id.clone(),
            page_ordinal: if mode == "wrong-page" { 1 } else { 0 },
            nonce: ticket.nonce.clone(),
            expected_width: 1,
            expected_height: 1,
        };
        let payload = match mode {
            "payload" => vec![1, 2, 3],
            "alpha" => vec![0, 0, 0, 254],
            _ => vec![0, 0, 0, 255],
        };
        let error = service.submit_page(metadata, payload, |_| {}).unwrap_err();
        assert_eq!(
            error.code,
            match mode {
                "payload" => "PIXEL_PAYLOAD_MISMATCH",
                "alpha" => "PIXEL_ALPHA_INVALID",
                _ => "PIXEL_TRANSFER_REJECTED",
            }
        );
        let failed = state.database().get_job(&session.job.id).unwrap().unwrap();
        assert_eq!(failed.state, JobState::Failed);
        assert!(failed
            .outputs
            .iter()
            .all(|output| output.staging_path.is_none()));
        assert!(std::fs::read_dir(destination.path())
            .unwrap()
            .next()
            .is_none());
        let replay = upload(&service, &session, 0, vec![0, 0, 0, 255]).unwrap_err();
        assert_eq!(replay.code, "PIXEL_TRANSFER_REJECTED");
    }
}

#[test]
fn exact_128_output_boundary_is_accepted_and_129_is_rejected() {
    let (_app_data, state, service) = service();
    let source_dir = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let source = support::write_multi_page_pdf_fixture(source_dir.path(), "many.pdf", "G04B2", 128);
    let plans = (0..128)
        .map(|source_page_index| PdfToImagesPagePlan {
            source_page_index,
            width: 1,
            height: 1,
        })
        .collect::<Vec<_>>();
    let session = service
        .create_job(request(
            &state,
            &source,
            destination.path(),
            128,
            plans,
            PdfImageFormat::Webp,
            300,
        ))
        .unwrap();
    assert_eq!(session.pages.len(), 128);
    assert_eq!(
        state.cancellations.request(&session.job.id),
        CancelOutcome::Requested
    );
    assert!(service.cancel_if_idle(&session.job.id, |_| {}).unwrap());
    assert_eq!(
        state
            .database()
            .get_job(&session.job.id)
            .unwrap()
            .unwrap()
            .state,
        JobState::Cancelled
    );

    let invalid = PdfToImagesJobCreateRequest {
        viewer_session_id: uuid::Uuid::new_v4().to_string(),
        viewer_generation: 1,
        destination_grant_id: uuid::Uuid::new_v4().to_string(),
        source_page_count: 129,
        pages: (0..129)
            .map(|source_page_index| PdfToImagesPagePlan {
                source_page_index,
                width: 1,
                height: 1,
            })
            .collect(),
        format: PdfImageFormat::Png,
        dpi: 72,
        output_stem: "report".to_owned(),
    };
    assert_eq!(
        service.create_job(invalid).unwrap_err().code,
        "INVALID_REQUEST"
    );
}

#[test]
fn dimension_pixel_and_aggregate_caps_are_enforced_before_session_access() {
    let (_app_data, _state, service) = service();
    let base = || PdfToImagesJobCreateRequest {
        viewer_session_id: uuid::Uuid::new_v4().to_string(),
        viewer_generation: 1,
        destination_grant_id: uuid::Uuid::new_v4().to_string(),
        source_page_count: 5,
        pages: vec![PdfToImagesPagePlan {
            source_page_index: 0,
            width: 1,
            height: 1,
        }],
        format: PdfImageFormat::Png,
        dpi: 72,
        output_stem: "report".to_owned(),
    };
    let mut width = base();
    width.pages[0].width = 8193;
    assert_eq!(
        service.create_job(width).unwrap_err().code,
        "IMAGE_DIMENSION_LIMIT"
    );
    let mut height = base();
    height.pages[0].height = 8193;
    assert_eq!(
        service.create_job(height).unwrap_err().code,
        "IMAGE_DIMENSION_LIMIT"
    );
    let mut pixels = base();
    pixels.pages[0] = PdfToImagesPagePlan {
        source_page_index: 0,
        width: 8192,
        height: 8192,
    };
    assert_eq!(
        service.create_job(pixels).unwrap_err().code,
        "IMAGE_PIXEL_LIMIT"
    );
    let mut aggregate = base();
    aggregate.pages = (0..5)
        .map(|source_page_index| PdfToImagesPagePlan {
            source_page_index,
            width: 4096,
            height: 4096,
        })
        .collect();
    assert_eq!(
        service.create_job(aggregate).unwrap_err().code,
        "AGGREGATE_PIXEL_LIMIT"
    );
}

#[test]
fn destination_collision_never_overwrites_the_racing_user_file() {
    let (_app_data, state, service) = service();
    let source_dir = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let source = support::write_pdf_fixture(source_dir.path(), "input.pdf", "G04B2", 612);
    let session = service
        .create_job(request(
            &state,
            &source,
            destination.path(),
            1,
            vec![PdfToImagesPagePlan {
                source_page_index: 0,
                width: 1,
                height: 1,
            }],
            PdfImageFormat::Png,
            72,
        ))
        .unwrap();
    let raced = destination.path().join("report-page-0001.png");
    std::fs::write(&raced, b"user-owned").unwrap();
    let completed = upload(&service, &session, 0, vec![255, 255, 255, 255]).unwrap();
    assert_eq!(std::fs::read(&raced).unwrap(), b"user-owned");
    assert_ne!(
        completed.outputs[0].final_path.as_deref(),
        Some(raced.to_string_lossy().as_ref())
    );
    assert_eq!(
        completed.outputs[0].resolved_name.as_deref(),
        Some("report-page-0001 (1).png")
    );
}

#[test]
fn verified_staging_hash_drift_fails_before_any_publication() {
    let (_app_data, state, service) = service();
    let source_dir = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let source = support::write_multi_page_pdf_fixture(source_dir.path(), "input.pdf", "G04B2", 2);
    let session = service
        .create_job(request(
            &state,
            &source,
            destination.path(),
            2,
            vec![
                PdfToImagesPagePlan {
                    source_page_index: 0,
                    width: 1,
                    height: 1,
                },
                PdfToImagesPagePlan {
                    source_page_index: 1,
                    width: 1,
                    height: 1,
                },
            ],
            PdfImageFormat::Png,
            72,
        ))
        .unwrap();
    let running = upload(&service, &session, 0, vec![255, 0, 0, 255]).unwrap();
    let first_staging = running.outputs[0].staging_path.as_ref().unwrap();
    std::fs::write(first_staging, b"staging changed after verification").unwrap();

    let error = upload(&service, &session, 1, vec![0, 0, 255, 255]).unwrap_err();
    assert_eq!(error.code, "IMAGE_VERIFY_FAILED");
    let failed = state.database().get_job(&session.job.id).unwrap().unwrap();
    assert_eq!(failed.state, JobState::Failed);
    assert!(failed
        .outputs
        .iter()
        .all(|output| output.final_path.is_none()));
    assert!(std::fs::read_dir(destination.path())
        .unwrap()
        .next()
        .is_none());
}

#[test]
fn malformed_and_encrypted_pdfs_are_rejected_before_rendering() {
    let (_app_data, state, service) = service();
    let source_dir = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let malformed = support::write_fixture(
        source_dir.path(),
        "malformed.pdf",
        b"%PDF-1.7\nnot a valid body",
    );
    let malformed_error = service
        .create_job(request(
            &state,
            &malformed,
            destination.path(),
            1,
            vec![PdfToImagesPagePlan {
                source_page_index: 0,
                width: 1,
                height: 1,
            }],
            PdfImageFormat::Png,
            72,
        ))
        .unwrap_err();
    assert_eq!(malformed_error.code, "PDF_MALFORMED");

    let plain = support::write_pdf_fixture(source_dir.path(), "plain.pdf", "G04B2", 612);
    let encrypted = source_dir.path().join("encrypted.pdf");
    let status = Command::new(bundle_root().join("bin/qpdf.exe"))
        .args(["--encrypt", "", "test-owner", "256", "--"])
        .arg(&plain)
        .arg(&encrypted)
        .status()
        .unwrap();
    assert!(status.success());
    let encrypted_error = service
        .create_job(request(
            &state,
            &encrypted,
            destination.path(),
            1,
            vec![PdfToImagesPagePlan {
                source_page_index: 0,
                width: 1,
                height: 1,
            }],
            PdfImageFormat::Png,
            72,
        ))
        .unwrap_err();
    assert_eq!(encrypted_error.code, "PDF_ENCRYPTED");
    assert!(std::fs::read_dir(destination.path())
        .unwrap()
        .next()
        .is_none());
}

#[test]
fn cancellation_cleans_owned_staging_and_source_remains_immutable() {
    let (_app_data, state, service) = service();
    let source_dir = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let source = support::write_pdf_fixture(source_dir.path(), "input.pdf", "G04B2", 612);
    let original = std::fs::read(&source).unwrap();
    let create_request = request(
        &state,
        &source,
        destination.path(),
        1,
        vec![PdfToImagesPagePlan {
            source_page_index: 0,
            width: 4096,
            height: 4096,
        }],
        PdfImageFormat::Png,
        300,
    );
    let viewer_request = ViewerSessionRequest {
        session_id: create_request.viewer_session_id.clone(),
        generation: create_request.viewer_generation,
    };
    let session = service.create_job(create_request).unwrap();
    let job_id = session.job.id.clone();
    let unrelated_user_file = destination.path().join("other-operation.png");
    std::fs::write(&unrelated_user_file, b"user-owned").unwrap();
    assert!(std::fs::write(&source, b"mutation denied while opaque session owns source").is_err());
    let started = std::time::Instant::now();
    assert_eq!(
        state.cancellations.request(&job_id),
        CancelOutcome::Requested
    );
    let mut events = Vec::new();
    assert!(service
        .cancel_if_idle(&job_id, |event| events.push(event))
        .unwrap());
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    let cancelled = state.database().get_job(&job_id).unwrap().unwrap();
    assert_eq!(cancelled.state, JobState::Cancelled);
    assert!(cancelled
        .outputs
        .iter()
        .all(|output| output.staging_path.is_none()));
    assert!(!state.workspaces.root().join(&job_id).exists());
    assert_eq!(
        state.cancellations.request(&job_id),
        CancelOutcome::NotRunning
    );
    let stale_render_session = upload(&service, &session, 0, vec![0; 4]).unwrap_err();
    assert_eq!(stale_render_session.code, "PIXEL_TRANSFER_REJECTED");
    assert_eq!(events.len(), 1);
    let event_json = serde_json::to_string(&events).unwrap();
    assert!(!event_json.contains(source.to_string_lossy().as_ref()));
    assert!(!event_json.contains(destination.path().to_string_lossy().as_ref()));
    assert_eq!(std::fs::read(&unrelated_user_file).unwrap(), b"user-owned");
    assert_eq!(std::fs::read(&source).unwrap(), original);
    state.viewer_sessions.close(&viewer_request).unwrap();
    std::fs::write(&source, &original)
        .expect("all source handles close after viewer reconciliation");
    assert_eq!(std::fs::read_dir(destination.path()).unwrap().count(), 1);
}

#[test]
fn partial_publication_preserves_the_first_user_file_and_reconciles_the_rest() {
    let (_app_data, state, _) = service();
    let service = PdfToImagesService::with_hooks(
        state.clone(),
        PdfToImagesHooks {
            after_output_published: Arc::new(|ordinal| {
                if ordinal == 0 {
                    Err(OperationError::safe(
                        "INJECTED_AFTER_FIRST_PUBLICATION",
                        "Injected publication interruption",
                        "The acceptance test stops after the first published image.",
                        OperationStage::Publish,
                        false,
                    ))
                } else {
                    Ok(())
                }
            }),
            ..PdfToImagesHooks::default()
        },
    );
    let source_dir = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let source = support::write_multi_page_pdf_fixture(source_dir.path(), "input.pdf", "G04B2", 2);
    let session = service
        .create_job(request(
            &state,
            &source,
            destination.path(),
            2,
            vec![
                PdfToImagesPagePlan {
                    source_page_index: 0,
                    width: 1,
                    height: 1,
                },
                PdfToImagesPagePlan {
                    source_page_index: 1,
                    width: 1,
                    height: 1,
                },
            ],
            PdfImageFormat::Png,
            72,
        ))
        .unwrap();
    upload(&service, &session, 0, vec![255, 0, 0, 255]).unwrap();
    let error = upload(&service, &session, 1, vec![0, 0, 255, 255]).unwrap_err();
    assert_eq!(error.code, "INJECTED_AFTER_FIRST_PUBLICATION");
    let failed = state.database().get_job(&session.job.id).unwrap().unwrap();
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(failed.outputs[0].status.as_str(), "published");
    assert!(failed.outputs[0]
        .final_path
        .as_ref()
        .is_some_and(|path| Path::new(path).exists()));
    assert_ne!(failed.outputs[1].status.as_str(), "published");
    assert!(failed.outputs[1].staging_path.is_none());
    assert!(failed
        .errors
        .iter()
        .any(|item| item.code == "PARTIAL_PUBLICATION"));
    assert_eq!(std::fs::read_dir(destination.path()).unwrap().count(), 1);
}

#[test]
#[ignore = "manual maximum-page CPU cancellation response evidence"]
fn measure_maximum_page_encode_cancellation_response() {
    let (_app_data, state, _) = service();
    let encode_ready = Arc::new(Barrier::new(2));
    let encode_release = Arc::new(Barrier::new(2));
    let ready_for_hook = encode_ready.clone();
    let release_for_hook = encode_release.clone();
    let service = PdfToImagesService::with_hooks(
        state.clone(),
        PdfToImagesHooks {
            before_encode: Arc::new(move || {
                ready_for_hook.wait();
                release_for_hook.wait();
            }),
            ..PdfToImagesHooks::default()
        },
    );
    let source_dir = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let source = support::write_pdf_fixture(source_dir.path(), "input.pdf", "G04B2", 612);
    let session = service
        .create_job(request(
            &state,
            &source,
            destination.path(),
            1,
            vec![PdfToImagesPagePlan {
                source_page_index: 0,
                width: 8192,
                height: 2048,
            }],
            PdfImageFormat::Png,
            300,
        ))
        .unwrap();
    let job_id = session.job.id.clone();
    let worker = std::thread::spawn(move || {
        upload(
            &service,
            &session,
            0,
            [235_u8, 235, 235, 255].repeat(16_777_216),
        )
    });
    encode_ready.wait();
    assert_eq!(
        state.cancellations.request(&job_id),
        CancelOutcome::Requested
    );
    let started = std::time::Instant::now();
    encode_release.wait();
    let error = worker.join().unwrap().unwrap_err();
    let elapsed = started.elapsed();
    eprintln!(
        "G04B2 maximum-page PNG encode cancellation response: {:.3}s",
        elapsed.as_secs_f64()
    );
    assert_eq!(error.code, "CANCELLED");
    assert!(elapsed < std::time::Duration::from_secs(60));
    assert!(std::fs::read_dir(destination.path())
        .unwrap()
        .next()
        .is_none());
}

#[test]
fn unexpected_staging_membership_fails_before_any_publication() {
    let (_app_data, state, service) = service();
    let source_dir = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let source = support::write_multi_page_pdf_fixture(source_dir.path(), "input.pdf", "G04B2", 2);
    let session = service
        .create_job(request(
            &state,
            &source,
            destination.path(),
            2,
            vec![
                PdfToImagesPagePlan {
                    source_page_index: 0,
                    width: 1,
                    height: 1,
                },
                PdfToImagesPagePlan {
                    source_page_index: 1,
                    width: 1,
                    height: 1,
                },
            ],
            PdfImageFormat::Png,
            72,
        ))
        .unwrap();
    upload(&service, &session, 0, vec![255, 0, 0, 255]).unwrap();
    std::fs::write(
        state
            .workspaces
            .root()
            .join(&session.job.id)
            .join("staging")
            .join("unexpected.bin"),
        b"not an expected output",
    )
    .unwrap();
    let error = upload(&service, &session, 1, vec![0, 0, 255, 255]).unwrap_err();
    assert_eq!(error.code, "UNEXPECTED_STAGING_OUTPUT");
    let failed = state.database().get_job(&session.job.id).unwrap().unwrap();
    assert_eq!(failed.state, JobState::Failed);
    assert!(failed
        .outputs
        .iter()
        .all(|output| output.staging_path.is_none()));
    assert!(std::fs::read_dir(destination.path())
        .unwrap()
        .next()
        .is_none());
}

#[test]
fn startup_recovery_fails_an_interrupted_render_job_and_removes_private_workspace() {
    let (app_data, state, service) = service();
    let source_dir = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let source = support::write_pdf_fixture(source_dir.path(), "input.pdf", "G04B2", 612);
    let session = service
        .create_job(request(
            &state,
            &source,
            destination.path(),
            1,
            vec![PdfToImagesPagePlan {
                source_page_index: 0,
                width: 1,
                height: 1,
            }],
            PdfImageFormat::Png,
            72,
        ))
        .unwrap();
    let job_id = session.job.id.clone();
    drop(service);
    drop(state);
    let database = Database::open(&app_data.path().join("metadata.sqlite3")).unwrap();
    let workspaces = WorkspaceManager::initialize(app_data.path()).unwrap();
    let recovered_state = AppState::new(database, workspaces);
    let report = reconcile_startup(&recovered_state).unwrap();
    assert_eq!(report.failed, 1);
    let recovered = recovered_state
        .database()
        .get_job(&job_id)
        .unwrap()
        .unwrap();
    assert_eq!(recovered.state, JobState::Failed);
    assert!(recovered
        .errors
        .iter()
        .any(|error| error.code == "JOB_INTERRUPTED_BY_RESTART"));
    assert!(!recovered_state.workspaces.root().join(&job_id).exists());
    assert!(std::fs::read_dir(destination.path())
        .unwrap()
        .next()
        .is_none());
}
