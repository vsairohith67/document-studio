mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use std::os::windows::fs::OpenOptionsExt;

use document_studio_lib::app_state::AppState;
use document_studio_lib::contracts::{
    CorePdfJobCreateRequest, CorePdfPlanPayload, ExtractPagesPlan, JobRecord, JobState,
    OperationError, OperationPlanEnvelope, OperationStage, OutputRotation, PageRotation,
    RemovePagesPlan, ReorderPagesPlan, RotatePagesPlan, SplitOutputRange, SplitPlan,
    ViewerSessionRequest, PDF_EXTRACT_OPERATION_ID, PDF_REMOVE_OPERATION_ID,
    PDF_REORDER_OPERATION_ID, PDF_ROTATE_OPERATION_ID, PDF_SPLIT_OPERATION_ID,
};
use document_studio_lib::database::Database;
use document_studio_lib::pdf_operations::{PdfPageOperationHooks, PdfPageOperationService};
use document_studio_lib::qpdf::QpdfRuntimeManager;
use document_studio_lib::workspace::WorkspaceManager;
use sha2::{Digest, Sha256};
use tempfile::tempdir;
use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

fn bundle_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/qpdf/12.3.2")
}

fn qpdf_executable() -> PathBuf {
    bundle_root().join("bin/qpdf.exe")
}

fn service() -> (tempfile::TempDir, AppState, PdfPageOperationService) {
    let app_data = tempdir().unwrap();
    let database = Database::open(&app_data.path().join("metadata.sqlite3")).unwrap();
    let workspaces = WorkspaceManager::initialize(app_data.path()).unwrap();
    let state = AppState::new(database, workspaces).with_qpdf(QpdfRuntimeManager::new(
        bundle_root(),
        app_data.path().join("engines"),
    ));
    state
        .qpdf
        .as_ref()
        .unwrap()
        .get_or_prepare()
        .expect("the accepted qpdf bundle must prepare for core-operation tests");
    let service = PdfPageOperationService::new(state.clone());
    (app_data, state, service)
}

fn execute(
    state: &AppState,
    service: &PdfPageOperationService,
    source: &Path,
    destination: &Path,
    operation_id: &str,
    payload: CorePdfPlanPayload,
    page_count: u32,
) -> JobRecord {
    let session = state.viewer_sessions.open_pdf(source).unwrap();
    let grant = state
        .viewer_sessions
        .grant_destination(destination)
        .unwrap();
    let (job, source) = service
        .create_job(CorePdfJobCreateRequest {
            viewer_session_id: session.session_id.clone(),
            viewer_generation: session.generation,
            destination_grant_id: grant.grant_id,
            plan: OperationPlanEnvelope {
                schema_version: 1,
                operation_id: operation_id.to_owned(),
                source_page_count: page_count,
                payload,
            },
        })
        .unwrap();
    let token = state.cancellations.register(&job.id);
    let completed = service
        .execute_with_registered_token(&job.id, source, token, |_| {})
        .unwrap();
    state
        .viewer_sessions
        .close(&ViewerSessionRequest {
            session_id: session.session_id,
            generation: session.generation,
        })
        .unwrap();
    completed
}

fn qpdf_output(arguments: &[String]) -> Vec<u8> {
    let output = Command::new(qpdf_executable())
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    output.stdout
}

fn page_records(path: &Path) -> Vec<((u32, u32), (u32, u32))> {
    let output = qpdf_output(&[
        path.to_string_lossy().into_owned(),
        "--suppress-recovery".to_owned(),
        "--show-pages".to_owned(),
    ]);
    let text = String::from_utf8(output).unwrap();
    let lines = text.lines().collect::<Vec<_>>();
    let mut records = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let page = lines[index].strip_prefix("page ").unwrap();
        let (_, reference) = page.split_once(": ").unwrap();
        let page_reference = parse_reference(reference);
        assert_eq!(lines[index + 1], "  content:");
        let content_reference = parse_reference(lines[index + 2].trim());
        records.push((page_reference, content_reference));
        index += 3;
    }
    records
}

fn parse_reference(value: &str) -> (u32, u32) {
    let fields = value.split_whitespace().collect::<Vec<_>>();
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[2], "R");
    (fields[0].parse().unwrap(), fields[1].parse().unwrap())
}

fn semantic_markers(path: &Path) -> Vec<String> {
    page_records(path)
        .into_iter()
        .map(|(_, (object, generation))| {
            let output = qpdf_output(&[
                path.to_string_lossy().into_owned(),
                "--suppress-recovery".to_owned(),
                format!("--show-object={object},{generation}"),
                "--filtered-stream-data".to_owned(),
            ]);
            String::from_utf8(output)
                .unwrap()
                .lines()
                .find_map(|line| line.strip_prefix("% DS-G02-MARKER:"))
                .unwrap()
                .to_owned()
        })
        .collect()
}

fn output_path(job: &JobRecord, ordinal: usize) -> &Path {
    Path::new(job.outputs[ordinal].final_path.as_ref().unwrap())
}

#[test]
fn five_core_operations_preserve_adversarial_page_semantics() {
    let (_app_data, state, service) = service();
    let source_directory = tempdir().unwrap();
    let source = support::write_semantic_pdf_fixture(
        source_directory.path(),
        "adversarial.pdf",
        &["PAGE-A", "PAGE-B", "PAGE-C", "PAGE-D"],
    );
    let source_before = std::fs::read(&source).unwrap();

    let extract_destination = tempdir().unwrap();
    let extract_started = Instant::now();
    let extract = execute(
        &state,
        &service,
        &source,
        extract_destination.path(),
        PDF_EXTRACT_OPERATION_ID,
        CorePdfPlanPayload::Extract(ExtractPagesPlan {
            selected_page_indexes: vec![2, 0],
            output_name: "extract.pdf".to_owned(),
        }),
        4,
    );
    let extract_ms = extract_started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        semantic_markers(output_path(&extract, 0)),
        ["PAGE-C", "PAGE-A"]
    );

    let remove_destination = tempdir().unwrap();
    let remove_started = Instant::now();
    let remove = execute(
        &state,
        &service,
        &source,
        remove_destination.path(),
        PDF_REMOVE_OPERATION_ID,
        CorePdfPlanPayload::Remove(RemovePagesPlan {
            removed_page_indexes: vec![1, 3],
            output_name: "remove.pdf".to_owned(),
        }),
        4,
    );
    let remove_ms = remove_started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        semantic_markers(output_path(&remove, 0)),
        ["PAGE-A", "PAGE-C"]
    );

    let reorder_destination = tempdir().unwrap();
    let reorder_started = Instant::now();
    let reorder = execute(
        &state,
        &service,
        &source,
        reorder_destination.path(),
        PDF_REORDER_OPERATION_ID,
        CorePdfPlanPayload::Reorder(ReorderPagesPlan {
            ordered_page_indexes: vec![3, 1, 0, 2],
            output_name: "reorder.pdf".to_owned(),
        }),
        4,
    );
    let reorder_ms = reorder_started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        semantic_markers(output_path(&reorder, 0)),
        ["PAGE-D", "PAGE-B", "PAGE-A", "PAGE-C"]
    );

    let rotate_destination = tempdir().unwrap();
    let rotate_started = Instant::now();
    let rotate = execute(
        &state,
        &service,
        &source,
        rotate_destination.path(),
        PDF_ROTATE_OPERATION_ID,
        CorePdfPlanPayload::Rotate(RotatePagesPlan {
            rotations: vec![
                PageRotation {
                    page_index: 0,
                    clockwise_degrees: OutputRotation::Clockwise90,
                },
                PageRotation {
                    page_index: 2,
                    clockwise_degrees: OutputRotation::Clockwise270,
                },
            ],
            output_name: "rotate.pdf".to_owned(),
        }),
        4,
    );
    let rotate_ms = rotate_started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        semantic_markers(output_path(&rotate, 0)),
        ["PAGE-A", "PAGE-B", "PAGE-C", "PAGE-D"]
    );
    let records = page_records(output_path(&rotate, 0));
    for (page_index, expected) in [(0_usize, 90_u16), (2, 270)] {
        let (object, generation) = records[page_index].0;
        let page_object = String::from_utf8(qpdf_output(&[
            output_path(&rotate, 0).to_string_lossy().into_owned(),
            format!("--show-object={object},{generation}"),
        ]))
        .unwrap();
        assert!(page_object.contains(&format!("/Rotate {expected}")));
    }

    let split_destination = tempdir().unwrap();
    let split_started = Instant::now();
    let split = execute(
        &state,
        &service,
        &source,
        split_destination.path(),
        PDF_SPLIT_OPERATION_ID,
        CorePdfPlanPayload::Split(SplitPlan {
            ranges: vec![
                SplitOutputRange {
                    start_page_index: 0,
                    end_page_index: 1,
                    output_name: "part-001.pdf".to_owned(),
                },
                SplitOutputRange {
                    start_page_index: 2,
                    end_page_index: 2,
                    output_name: "part-002.pdf".to_owned(),
                },
                SplitOutputRange {
                    start_page_index: 3,
                    end_page_index: 3,
                    output_name: "part-003.pdf".to_owned(),
                },
            ],
        }),
        4,
    );
    let split_ms = split_started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(split.state, JobState::Completed);
    assert_eq!(split.outputs.len(), 3);
    assert_eq!(
        semantic_markers(output_path(&split, 0)),
        ["PAGE-A", "PAGE-B"]
    );
    assert_eq!(semantic_markers(output_path(&split, 1)), ["PAGE-C"]);
    assert_eq!(semantic_markers(output_path(&split, 2)), ["PAGE-D"]);
    assert!(split.outputs.iter().all(|output| output.sha256.is_some()
        && output.staging_path.is_none()
        && output.partial_path.is_none()));
    assert_eq!(std::fs::read(&source).unwrap(), source_before);
    eprintln!(
        "G03_QPDF_PERF={{\"fixturePages\":4,\"fixtureBytes\":{},\"extractMs\":{extract_ms:.2},\"removeMs\":{remove_ms:.2},\"reorderMs\":{reorder_ms:.2},\"rotateMs\":{rotate_ms:.2},\"splitThreeOutputsMs\":{split_ms:.2}}}",
        source_before.len()
    );
}

#[test]
fn persisted_source_page_count_tampering_is_caught_before_output_execution() {
    let (_app_data, state, service) = service();
    let source_directory = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let source = support::write_semantic_pdf_fixture(
        source_directory.path(),
        "count.pdf",
        &["ONE", "TWO", "THREE"],
    );
    let source_before = std::fs::read(&source).unwrap();
    let session = state.viewer_sessions.open_pdf(&source).unwrap();
    let grant = state
        .viewer_sessions
        .grant_destination(destination.path())
        .unwrap();
    let (job, pinned) = service
        .create_job(CorePdfJobCreateRequest {
            viewer_session_id: session.session_id.clone(),
            viewer_generation: session.generation,
            destination_grant_id: grant.grant_id,
            plan: OperationPlanEnvelope {
                schema_version: 1,
                operation_id: PDF_EXTRACT_OPERATION_ID.to_owned(),
                source_page_count: 3,
                payload: CorePdfPlanPayload::Extract(ExtractPagesPlan {
                    selected_page_indexes: vec![0],
                    output_name: "wrong-count.pdf".to_owned(),
                }),
            },
        })
        .unwrap();
    let mut tampered = state
        .database()
        .get_operation_plan(&job.id)
        .unwrap()
        .unwrap()
        .envelope;
    tampered.source_page_count = 2;
    let tampered_json = serde_json::to_string(&tampered).unwrap();
    let tampered_hash = Sha256::digest(tampered_json.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    state
        .database()
        .connection()
        .execute(
            "UPDATE job_operation_plans
             SET source_page_count = 2, plan_json = ?1, plan_sha256 = ?2
             WHERE job_id = ?3",
            (&tampered_json, &tampered_hash, &job.id),
        )
        .unwrap();
    let token = state.cancellations.register(&job.id);
    let error = service
        .execute_with_registered_token(&job.id, pinned, token, |_| {})
        .unwrap_err();
    assert_eq!(error.code, "SOURCE_PAGE_COUNT_CHANGED");
    let failed = state.database().get_job(&job.id).unwrap().unwrap();
    assert_eq!(failed.state, JobState::Failed);
    assert!(failed
        .outputs
        .iter()
        .all(|output| output.final_path.is_none() && output.staging_path.is_none()));
    let output_execution_runs: i64 = state
        .database()
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM job_stage_runs WHERE job_id = ?1 AND stage = 'execute'",
            [&job.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(output_execution_runs, 0);
    assert!(std::fs::read_dir(destination.path())
        .unwrap()
        .next()
        .is_none());
    assert!(!state.workspaces.root().join(&job.id).exists());
    assert_eq!(std::fs::read(&source).unwrap(), source_before);

    let locked_destination = tempdir().unwrap();
    let locked_session = state.viewer_sessions.open_pdf(&source).unwrap();
    let locked_grant = state
        .viewer_sessions
        .grant_destination(locked_destination.path())
        .unwrap();
    let (locked_job, locked_source) = service
        .create_job(CorePdfJobCreateRequest {
            viewer_session_id: locked_session.session_id.clone(),
            viewer_generation: locked_session.generation,
            destination_grant_id: locked_grant.grant_id,
            plan: OperationPlanEnvelope {
                schema_version: 1,
                operation_id: PDF_EXTRACT_OPERATION_ID.to_owned(),
                source_page_count: 3,
                payload: CorePdfPlanPayload::Extract(ExtractPagesPlan {
                    selected_page_indexes: vec![0],
                    output_name: "cleanup-interrupted.pdf".to_owned(),
                }),
            },
        })
        .unwrap();
    let mut locked_tampered = state
        .database()
        .get_operation_plan(&locked_job.id)
        .unwrap()
        .unwrap()
        .envelope;
    locked_tampered.source_page_count = 2;
    let locked_json = serde_json::to_string(&locked_tampered).unwrap();
    let locked_hash = Sha256::digest(locked_json.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    state
        .database()
        .connection()
        .execute(
            "UPDATE job_operation_plans
             SET source_page_count = 2, plan_json = ?1, plan_sha256 = ?2
             WHERE job_id = ?3",
            (&locked_json, &locked_hash, &locked_job.id),
        )
        .unwrap();
    let locked_workspace = state.workspaces.root().join(&locked_job.id);
    let mut cleanup_lock = None;
    let locked_token = state.cancellations.register(&locked_job.id);
    let locked_error = service
        .execute_with_registered_token(&locked_job.id, locked_source, locked_token, |event| {
            if event.message_code == "SNAPSHOTTING_SOURCE" && cleanup_lock.is_none() {
                cleanup_lock = Some(
                    std::fs::OpenOptions::new()
                        .read(true)
                        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                        .open(locked_workspace.join(".document-studio-job-v1"))
                        .unwrap(),
                );
            }
        })
        .unwrap_err();
    assert_eq!(locked_error.code, "SOURCE_PAGE_COUNT_CHANGED");
    let interrupted = state.database().get_job(&locked_job.id).unwrap().unwrap();
    assert_eq!(interrupted.state, JobState::Interrupted);
    assert!(interrupted
        .errors
        .iter()
        .any(|error| error.code == "CLEANUP_FAILED"));
    assert!(locked_workspace.exists());
    assert!(std::fs::read_dir(locked_destination.path())
        .unwrap()
        .next()
        .is_none());
    assert_eq!(std::fs::read(&source).unwrap(), source_before);
    drop(cleanup_lock);
    state.workspaces.cleanup_job(&locked_job.id).unwrap();
    assert!(!locked_workspace.exists());
    state
        .viewer_sessions
        .close(&ViewerSessionRequest {
            session_id: session.session_id,
            generation: session.generation,
        })
        .unwrap();
    state
        .viewer_sessions
        .close(&ViewerSessionRequest {
            session_id: locked_session.session_id,
            generation: locked_session.generation,
        })
        .unwrap();
}

#[test]
fn split_partial_publication_is_failed_truthful_and_preserves_published_files() {
    let (_app_data, state, _service) = service();
    let service = PdfPageOperationService::new(state.clone()).with_hooks(PdfPageOperationHooks {
        after_output_published: Arc::new(|ordinal| {
            if ordinal == 0 {
                Err(OperationError::safe(
                    "TEST_AFTER_FIRST_OUTPUT",
                    "Injected failure after first output",
                    "Used only to prove partial-publication behavior.",
                    OperationStage::Publish,
                    false,
                ))
            } else {
                Ok(())
            }
        }),
    });
    let source_directory = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let source = support::write_semantic_pdf_fixture(
        source_directory.path(),
        "partial.pdf",
        &["FIRST", "SECOND"],
    );
    let session = state.viewer_sessions.open_pdf(&source).unwrap();
    let grant = state
        .viewer_sessions
        .grant_destination(destination.path())
        .unwrap();
    let (job, pinned) = service
        .create_job(CorePdfJobCreateRequest {
            viewer_session_id: session.session_id,
            viewer_generation: session.generation,
            destination_grant_id: grant.grant_id,
            plan: OperationPlanEnvelope {
                schema_version: 1,
                operation_id: PDF_SPLIT_OPERATION_ID.to_owned(),
                source_page_count: 2,
                payload: CorePdfPlanPayload::Split(SplitPlan {
                    ranges: vec![
                        SplitOutputRange {
                            start_page_index: 0,
                            end_page_index: 0,
                            output_name: "part-001.pdf".to_owned(),
                        },
                        SplitOutputRange {
                            start_page_index: 1,
                            end_page_index: 1,
                            output_name: "part-002.pdf".to_owned(),
                        },
                    ],
                }),
            },
        })
        .unwrap();
    let token = state.cancellations.register(&job.id);
    assert!(service
        .execute_with_registered_token(&job.id, pinned, token, |_| {})
        .is_err());
    let failed = state.database().get_job(&job.id).unwrap().unwrap();
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(
        failed
            .outputs
            .iter()
            .filter(|output| output.status.as_str() == "published")
            .count(),
        1
    );
    assert!(failed
        .errors
        .iter()
        .any(|error| error.code == "PARTIAL_PUBLICATION"));
    assert!(output_path(&failed, 0).is_file());
    assert_eq!(semantic_markers(output_path(&failed, 0)), ["FIRST"]);
    assert!(!state.workspaces.root().join(&failed.id).exists());
    assert!(failed
        .outputs
        .iter()
        .all(|output| output.staging_path.is_none() && output.partial_path.is_none()));
}

#[test]
fn encrypted_source_is_rejected_without_a_password_transport_or_output() {
    let (_app_data, state, service) = service();
    let source_directory = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let clear =
        support::write_semantic_pdf_fixture(source_directory.path(), "clear.pdf", &["PRIVATE"]);
    let encrypted = source_directory.path().join("encrypted.pdf");
    let status = Command::new(qpdf_executable())
        .arg(&clear)
        .args(["--encrypt", "", "test-owner", "256", "--"])
        .arg(&encrypted)
        .status()
        .expect("generate the encrypted test fixture with the accepted qpdf");
    assert!(status.success());

    let session = state.viewer_sessions.open_pdf(&encrypted).unwrap();
    let grant = state
        .viewer_sessions
        .grant_destination(destination.path())
        .unwrap();
    let (job, pinned) = service
        .create_job(CorePdfJobCreateRequest {
            viewer_session_id: session.session_id,
            viewer_generation: session.generation,
            destination_grant_id: grant.grant_id,
            plan: OperationPlanEnvelope {
                schema_version: 1,
                operation_id: PDF_EXTRACT_OPERATION_ID.to_owned(),
                source_page_count: 1,
                payload: CorePdfPlanPayload::Extract(ExtractPagesPlan {
                    selected_page_indexes: vec![0],
                    output_name: "must-not-exist.pdf".to_owned(),
                }),
            },
        })
        .unwrap();
    let token = state.cancellations.register(&job.id);
    let error = service
        .execute_with_registered_token(&job.id, pinned, token, |_| {})
        .unwrap_err();
    assert_eq!(error.code, "PDF_ENCRYPTED");
    assert_eq!(
        state.database().get_job(&job.id).unwrap().unwrap().state,
        JobState::Failed
    );
    assert!(std::fs::read_dir(destination.path())
        .unwrap()
        .next()
        .is_none());
}

#[test]
fn cancellation_before_snapshot_prevents_processing_and_publication() {
    let (_app_data, state, service) = service();
    let source_directory = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let source = support::write_semantic_pdf_fixture(
        source_directory.path(),
        "cancel.pdf",
        &["FIRST", "SECOND"],
    );
    let session = state.viewer_sessions.open_pdf(&source).unwrap();
    let grant = state
        .viewer_sessions
        .grant_destination(destination.path())
        .unwrap();
    let (job, pinned) = service
        .create_job(CorePdfJobCreateRequest {
            viewer_session_id: session.session_id,
            viewer_generation: session.generation,
            destination_grant_id: grant.grant_id,
            plan: OperationPlanEnvelope {
                schema_version: 1,
                operation_id: PDF_REMOVE_OPERATION_ID.to_owned(),
                source_page_count: 2,
                payload: CorePdfPlanPayload::Remove(RemovePagesPlan {
                    removed_page_indexes: vec![1],
                    output_name: "cancelled.pdf".to_owned(),
                }),
            },
        })
        .unwrap();
    let token = state.cancellations.register(&job.id);
    let cancelled_started = Instant::now();
    state.cancellations.request(&job.id);
    let error = service
        .execute_with_registered_token(&job.id, pinned, token, |_| {})
        .unwrap_err();
    assert_eq!(error.code, "CANCELLED");
    let cancellation_ms = cancelled_started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        state.database().get_job(&job.id).unwrap().unwrap().state,
        JobState::Cancelled
    );
    assert!(!state.workspaces.root().join(&job.id).exists());
    assert!(std::fs::read_dir(destination.path())
        .unwrap()
        .next()
        .is_none());
    eprintln!("G03_CANCEL_PERF={{\"preExecutionCancellationMs\":{cancellation_ms:.2}}}");
}

#[test]
fn records_five_operation_preflight_samples() {
    let (_app_data, state, service) = service();
    let source_directory = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let source = support::write_semantic_pdf_fixture(
        source_directory.path(),
        "preflight.pdf",
        &["ONE", "TWO", "THREE", "FOUR"],
    );
    for run in 0..5 {
        let session = state.viewer_sessions.open_pdf(&source).unwrap();
        let close_request = ViewerSessionRequest {
            session_id: session.session_id.clone(),
            generation: session.generation,
        };
        let grant = state
            .viewer_sessions
            .grant_destination(destination.path())
            .unwrap();
        let (job, pinned) = service
            .create_job(CorePdfJobCreateRequest {
                viewer_session_id: session.session_id,
                viewer_generation: session.generation,
                destination_grant_id: grant.grant_id,
                plan: OperationPlanEnvelope {
                    schema_version: 1,
                    operation_id: PDF_EXTRACT_OPERATION_ID.to_owned(),
                    source_page_count: 4,
                    payload: CorePdfPlanPayload::Extract(ExtractPagesPlan {
                        selected_page_indexes: vec![0, 2],
                        output_name: format!("preflight-{run}.pdf"),
                    }),
                },
            })
            .unwrap();
        let token = state.cancellations.register(&job.id);
        let mut preflight_started = None;
        let mut execute_started = None;
        service
            .execute_with_registered_token(&job.id, pinned, token, |event| {
                if event.stage == OperationStage::Preflight && preflight_started.is_none() {
                    preflight_started = Some(Instant::now());
                }
                if event.stage == OperationStage::Execute && execute_started.is_none() {
                    execute_started = Some(Instant::now());
                }
            })
            .unwrap();
        state.viewer_sessions.close(&close_request).unwrap();
        let preflight_ms = execute_started
            .unwrap()
            .duration_since(preflight_started.unwrap())
            .as_secs_f64()
            * 1000.0;
        eprintln!(
            "G03_PREFLIGHT_PERF={{\"run\":{},\"fixturePages\":4,\"fixtureBytes\":{},\"preflightMs\":{preflight_ms:.2}}}",
            run + 1,
            std::fs::metadata(&source).unwrap().len()
        );
    }
}
