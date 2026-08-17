mod support;

use std::fs;
use std::io::Write;
use std::mem::size_of;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use document_studio_lib::app_state::AppState;
use document_studio_lib::contracts::JobInput;
use document_studio_lib::contracts::{FilesInspectRequest, JobState, JobsCreateRequest};
use document_studio_lib::database::Database;
use document_studio_lib::pdf_merge::{
    plan_inputs, PdfMergeHooks, PdfMergeService, PREFLIGHT_MAX_CONCURRENCY,
};
use document_studio_lib::process_sandbox::{
    authorize_qpdf_paths, ensure_production_profile, run_sandboxed_capture, SandboxLaunchSpec,
};
use document_studio_lib::qpdf::QpdfRuntimeManager;
use document_studio_lib::workspace::WorkspaceManager;
use tempfile::tempdir;
use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

fn input(ordinal: u32, identity: &str) -> JobInput {
    JobInput {
        ordinal,
        display_name: format!("source-{ordinal}.pdf"),
        source_path: format!(r"C:\source\source-{ordinal}.pdf"),
        canonical_path: format!(r"C:\source\source-{ordinal}.pdf"),
        file_identity: identity.to_owned(),
        size_bytes: 100,
        modified_at: "2026-08-17T00:00:00Z".to_owned(),
        mime_type: "application/pdf".to_owned(),
        sha256: None,
        password_reference: None,
    }
}

#[test]
fn duplicate_identity_shares_preflight_but_never_snapshot_or_ordinal() {
    let inputs = vec![input(0, "same"), input(1, "different"), input(2, "same")];
    let plan = plan_inputs(&inputs).unwrap();

    assert_eq!(plan.unique_sources.len(), 2);
    assert_eq!(plan.unique_sources[0].ordinals, [0, 2]);
    assert_eq!(plan.unique_sources[1].ordinals, [1]);
    assert_eq!(plan.snapshots.len(), inputs.len());
    assert_eq!(plan.snapshots[0].ordinal, 0);
    assert_eq!(plan.snapshots[1].ordinal, 1);
    assert_eq!(plan.snapshots[2].ordinal, 2);
    assert_ne!(
        plan.snapshots[0].relative_path,
        plan.snapshots[2].relative_path
    );
}

#[test]
fn unique_source_preflight_concurrency_is_bounded() {
    let inputs = (0..16)
        .map(|ordinal| input(ordinal, &format!("identity-{ordinal}")))
        .collect::<Vec<_>>();
    let plan = plan_inputs(&inputs).unwrap();
    assert!((1..=PREFLIGHT_MAX_CONCURRENCY).contains(&plan.preflight_concurrency));
    assert!(plan.preflight_concurrency <= plan.unique_sources.len());
}

fn bundle_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/qpdf/12.3.2")
}

fn service() -> (tempfile::TempDir, AppState, PdfMergeService) {
    let app_data = tempdir().unwrap();
    let database = Database::open(&app_data.path().join("metadata.sqlite3")).unwrap();
    let workspaces = WorkspaceManager::initialize(app_data.path()).unwrap();
    let state = AppState::new(database, workspaces).with_qpdf(QpdfRuntimeManager::new(
        bundle_root(),
        app_data.path().join("engines"),
    ));
    let service = PdfMergeService::new(state.clone());
    (app_data, state, service)
}

fn request(inputs: &[PathBuf], destination: &Path, name: &str) -> JobsCreateRequest {
    JobsCreateRequest {
        operation_id: "pdf.merge".to_owned(),
        input_paths: inputs
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        destination_directory: destination.to_string_lossy().into_owned(),
        requested_output_name: name.to_owned(),
    }
}

#[test]
fn production_merge_preserves_exact_order_and_cleans_every_snapshot() {
    let (_app_data, state, service) = service();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let first = support::write_pdf_fixture(source.path(), "first.pdf", "ORDER-FIRST", 401);
    let second = support::write_pdf_fixture(source.path(), "second.pdf", "ORDER-SECOND", 402);
    let third = support::write_pdf_fixture(source.path(), "third.pdf", "ORDER-THIRD", 403);
    let job = service
        .create_job(request(
            &[third.clone(), first.clone(), second.clone()],
            destination.path(),
            "merged.pdf",
        ))
        .unwrap();

    let completed = service.execute(&job.id, |_| {}).unwrap();

    assert_eq!(completed.state, JobState::Completed);
    assert_eq!(
        completed
            .inputs
            .iter()
            .map(|input| input.ordinal)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    let output = fs::read(completed.outputs[0].final_path.as_ref().unwrap()).unwrap();
    let positions = ["ORDER-THIRD", "ORDER-FIRST", "ORDER-SECOND"].map(|marker| {
        output
            .windows(marker.len())
            .position(|window| window == marker.as_bytes())
            .expect("merged output marker")
    });
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(!state.workspaces.root().join(&job.id).exists());
}

#[test]
fn repeated_input_and_hard_link_get_distinct_ordinals_and_contribute_pages() {
    let (_app_data, _state, service) = service();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let original = support::write_pdf_fixture(source.path(), "original.pdf", "REPEATED", 410);
    let alias = source.path().join("alias.pdf");
    fs::hard_link(&original, &alias).unwrap();
    let other = support::write_pdf_fixture(source.path(), "other.pdf", "OTHER", 420);
    let job = service
        .create_job(request(
            &[original.clone(), original, alias, other],
            destination.path(),
            "duplicates.pdf",
        ))
        .unwrap();

    let completed = service.execute(&job.id, |_| {}).unwrap();

    assert_eq!(completed.state, JobState::Completed);
    assert_eq!(completed.inputs.len(), 4);
    assert_eq!(
        completed.inputs[0].file_identity,
        completed.inputs[1].file_identity
    );
    assert_eq!(
        completed.inputs[0].file_identity,
        completed.inputs[2].file_identity
    );
    assert!(completed.inputs.iter().all(|input| input.sha256.is_some()));
}

#[test]
fn malformed_and_zero_page_inputs_fail_without_publication() {
    let (_app_data, state, service) = service();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let valid = support::write_pdf_fixture(source.path(), "valid.pdf", "VALID", 430);
    let malformed = support::write_fixture(source.path(), "malformed.pdf", b"%PDF-1.7\nbroken");
    let malformed_job = service
        .create_job(request(
            &[valid.clone(), malformed],
            destination.path(),
            "malformed-output.pdf",
        ))
        .unwrap();
    assert_eq!(
        service.execute(&malformed_job.id, |_| {}).unwrap_err().code,
        "PDF_STRUCTURE_INVALID"
    );
    assert_eq!(
        state
            .database()
            .get_job(&malformed_job.id)
            .unwrap()
            .unwrap()
            .state,
        JobState::Failed
    );
    let zero = support::write_zero_page_pdf(source.path(), "zero.pdf");
    let zero_job = service
        .create_job(request(
            &[valid, zero],
            destination.path(),
            "zero-output.pdf",
        ))
        .unwrap();
    assert_eq!(
        service.execute(&zero_job.id, |_| {}).unwrap_err().code,
        "PDF_ZERO_PAGES"
    );
    assert!(!destination.path().join("malformed-output.pdf").exists());
    assert!(!destination.path().join("zero-output.pdf").exists());
}

#[test]
fn source_change_and_destination_input_are_rejected_without_source_mutation() {
    let (_app_data, state, service) = service();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let first = support::write_pdf_fixture(source.path(), "first.pdf", "FIRST", 440);
    let second = support::write_pdf_fixture(source.path(), "second.pdf", "SECOND", 441);
    let original_first = fs::read(&first).unwrap();
    let job = service
        .create_job(request(
            &[first.clone(), second.clone()],
            destination.path(),
            "changed.pdf",
        ))
        .unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(&second)
        .unwrap()
        .write_all(b"changed")
        .unwrap();
    assert_eq!(
        service.execute(&job.id, |_| {}).unwrap_err().code,
        "SOURCE_CHANGED"
    );
    assert_eq!(fs::read(&first).unwrap(), original_first);
    assert_eq!(
        state.database().get_job(&job.id).unwrap().unwrap().state,
        JobState::Failed
    );

    let error = service
        .create_job(request(
            &[first.clone(), second],
            source.path(),
            "first.pdf",
        ))
        .unwrap_err();
    assert_eq!(error.code, "DESTINATION_IS_INPUT");
}

#[test]
fn publication_never_overwrites_an_existing_pdf() {
    let (_app_data, _state, service) = service();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let first = support::write_pdf_fixture(source.path(), "first.pdf", "FIRST", 450);
    let second = support::write_pdf_fixture(source.path(), "second.pdf", "SECOND", 451);
    fs::write(destination.path().join("merged.pdf"), b"existing").unwrap();
    let job = service
        .create_job(request(&[first, second], destination.path(), "merged.pdf"))
        .unwrap();
    let completed = service.execute(&job.id, |_| {}).unwrap();
    assert_eq!(
        fs::read(destination.path().join("merged.pdf")).unwrap(),
        b"existing"
    );
    assert_eq!(
        completed.outputs[0].resolved_name.as_deref(),
        Some("merged (1).pdf")
    );
}

#[test]
fn bundled_runtime_launches_through_the_production_boundary() {
    let (app_data, state, _service) = service();
    let runtime = state
        .qpdf
        .as_ref()
        .unwrap()
        .get_or_prepare()
        .expect("verify and materialize qpdf runtime");
    let profile = ensure_production_profile().expect("fixed production profile");
    let workspace = state
        .workspaces
        .create_job(&uuid::Uuid::new_v4().hyphenated().to_string())
        .unwrap();
    authorize_qpdf_paths(&profile, &runtime.bin, &workspace).expect("native AppContainer ACLs");
    let arguments = [std::ffi::OsString::from("--version")];
    let execution = run_sandboxed_capture(
        &profile,
        &SandboxLaunchSpec {
            executable: &runtime.executable,
            arguments: &arguments,
            working_directory: &workspace.root,
            temporary_directory: &workspace.temporary,
        },
        std::time::Duration::from_secs(10),
    )
    .expect("sandbox qpdf version probe");
    assert_eq!(execution.exit_code, 0);
    assert!(document_studio_lib::qpdf::version_output_is_expected(
        &execution.stdout
    ));
    support::write_pdf_fixture(&workspace.inputs, "source-0000.pdf", "BOUNDARY", 500);
    let check_arguments = [
        std::ffi::OsString::from(r"inputs\source-0000.pdf"),
        std::ffi::OsString::from("--suppress-recovery"),
        std::ffi::OsString::from("--check"),
    ];
    let checked = run_sandboxed_capture(
        &profile,
        &SandboxLaunchSpec {
            executable: &runtime.executable,
            arguments: &check_arguments,
            working_directory: &workspace.root,
            temporary_directory: &workspace.temporary,
        },
        std::time::Duration::from_secs(10),
    )
    .expect("sandbox qpdf structural check");
    assert_eq!(checked.exit_code, 0);
    let page_arguments = [
        std::ffi::OsString::from(r"inputs\source-0000.pdf"),
        std::ffi::OsString::from("--suppress-recovery"),
        std::ffi::OsString::from("--show-npages"),
    ];
    let pages = run_sandboxed_capture(
        &profile,
        &SandboxLaunchSpec {
            executable: &runtime.executable,
            arguments: &page_arguments,
            working_directory: &workspace.root,
            temporary_directory: &workspace.temporary,
        },
        std::time::Duration::from_secs(10),
    )
    .expect("sandbox qpdf page count");
    assert_eq!(pages.exit_code, 0);
    drop(app_data);
}

#[test]
fn encrypted_and_recovery_required_pdfs_are_rejected_without_repair() {
    let (_app_data, _state, service) = service();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let valid = support::write_pdf_fixture(source.path(), "valid.pdf", "VALID", 510);
    let encrypted = source.path().join("encrypted.pdf");
    let status = Command::new(bundle_root().join("bin/qpdf.exe"))
        .arg(&valid)
        .args(["--encrypt", "", "test-owner", "256", "--"])
        .arg(&encrypted)
        .status()
        .expect("generate an encrypted local fixture with the approved qpdf");
    assert!(status.success());
    let encrypted_job = service
        .create_job(request(
            &[valid.clone(), encrypted],
            destination.path(),
            "encrypted-output.pdf",
        ))
        .unwrap();
    assert_eq!(
        service.execute(&encrypted_job.id, |_| {}).unwrap_err().code,
        "PDF_ENCRYPTED"
    );

    let recoverable = source.path().join("recoverable.pdf");
    let mut bytes = fs::read(&valid).unwrap();
    let marker = b"startxref\n";
    let position = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap()
        + marker.len();
    let end = bytes[position..]
        .iter()
        .position(|byte| *byte == b'\n')
        .unwrap()
        + position;
    bytes.splice(position..end, b"1".iter().copied());
    fs::write(&recoverable, bytes).unwrap();
    let recoverable_job = service
        .create_job(request(
            &[valid, recoverable],
            destination.path(),
            "recoverable-output.pdf",
        ))
        .unwrap();
    assert_eq!(
        service
            .execute(&recoverable_job.id, |_| {})
            .unwrap_err()
            .code,
        "PDF_STRUCTURE_INVALID"
    );
}

#[test]
fn busy_input_is_reported_by_ordinal_and_unicode_paths_merge() {
    let (_app_data, _state, service) = service();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let first = support::write_pdf_fixture(
        source.path(),
        "\u{6587}\u{6863}-one.pdf",
        "UNICODE-ONE",
        520,
    );
    let second = support::write_pdf_fixture(
        source.path(),
        "r\u{00e9}sum\u{00e9}-two.pdf",
        "UNICODE-TWO",
        521,
    );
    let busy_job = service
        .create_job(request(
            &[first.clone(), second.clone()],
            destination.path(),
            "busy.pdf",
        ))
        .unwrap();
    let lock = fs::OpenOptions::new()
        .write(true)
        .share_mode(0)
        .open(&second)
        .unwrap();
    let busy = service.execute(&busy_job.id, |_| {}).unwrap_err();
    assert_eq!(busy.code, "SOURCE_BUSY");
    assert_eq!(busy.input_index, Some(1));
    drop(lock);

    let unicode_job = service
        .create_job(request(
            &[second, first],
            destination.path(),
            "\u{7d50}\u{5408}.pdf",
        ))
        .unwrap();
    assert_eq!(
        service.execute(&unicode_job.id, |_| {}).unwrap().state,
        JobState::Completed
    );
}

#[test]
fn long_local_paths_are_snapshotted_through_short_ascii_ordinals() {
    let (_app_data, _state, service) = service();
    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let mut long_directory = source.path().to_path_buf();
    for index in 0..7 {
        long_directory.push(format!("segment-{index}-abcdefghijklmnopqrstuvwxyz"));
        fs::create_dir(&long_directory).unwrap();
    }
    assert!(
        long_directory
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .count()
            > 260
    );
    let first = support::write_pdf_fixture(&long_directory, "long-first.pdf", "LONG-FIRST", 525);
    let second = support::write_pdf_fixture(&long_directory, "long-second.pdf", "LONG-SECOND", 526);
    let job = service
        .create_job(request(
            &[first, second],
            destination.path(),
            "long-output.pdf",
        ))
        .unwrap();
    assert_eq!(
        service.execute(&job.id, |_| {}).unwrap().state,
        JobState::Completed
    );
}

#[test]
fn cancellation_during_qpdf_and_near_publication_leave_no_final_file() {
    for (name, hooks, wait_for_partial) in [
        (
            "during-qpdf.pdf",
            PdfMergeHooks {
                pause_after_merge_process_start: Some(Duration::from_secs(2)),
                ..Default::default()
            },
            false,
        ),
        (
            "near-publication.pdf",
            PdfMergeHooks {
                pause_before_publication_commit: Some(Duration::from_secs(2)),
                ..Default::default()
            },
            true,
        ),
    ] {
        let (_app_data, state, service) = service();
        let source = tempdir().unwrap();
        let destination = tempdir().unwrap();
        let first = support::write_pdf_fixture(source.path(), "first.pdf", "FIRST", 530);
        let second = support::write_pdf_fixture(source.path(), "second.pdf", "SECOND", 531);
        let job = service
            .create_job(request(&[first, second], destination.path(), name))
            .unwrap();
        let worker_service = service.clone();
        let worker_id = job.id.clone();
        let worker = std::thread::spawn(move || {
            worker_service.execute_with_hooks(&worker_id, |_| {}, hooks)
        });
        let started = Instant::now();
        loop {
            let current = state.database().get_job(&job.id).unwrap().unwrap();
            let ready = if wait_for_partial {
                current.outputs[0].partial_path.is_some()
            } else {
                current.state == JobState::Running
            };
            if ready {
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            state.cancellations.request(&job.id),
            document_studio_lib::app_state::CancelOutcome::Requested
        );
        state.database().request_cancellation(&job.id).unwrap();
        let cancelled = worker.join().unwrap().unwrap();
        assert_eq!(cancelled.state, JobState::Cancelled);
        assert!(!destination.path().join(name).exists());
        assert!(support::partial_files(destination.path()).is_empty());
    }
}

#[test]
fn verification_low_space_and_cleanup_failures_remain_safe_and_recoverable() {
    let source = tempdir().unwrap();
    let first = support::write_pdf_fixture(source.path(), "first.pdf", "FIRST", 540);
    let second = support::write_pdf_fixture(source.path(), "second.pdf", "SECOND", 541);

    let (_app_data, _state, low_space_service) = service();
    let destination = tempdir().unwrap();
    let low_space_job = low_space_service
        .create_job(request(
            &[first.clone(), second.clone()],
            destination.path(),
            "low-space.pdf",
        ))
        .unwrap();
    let low_space = low_space_service
        .execute_with_hooks(
            &low_space_job.id,
            |_| {},
            PdfMergeHooks {
                available_space_override: Some(0),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert_eq!(low_space.code, "INSUFFICIENT_SPACE");
    assert!(!destination.path().join("low-space.pdf").exists());

    let (_app_data, _state, corrupt_service) = service();
    let destination = tempdir().unwrap();
    let corrupt_job = corrupt_service
        .create_job(request(
            &[first.clone(), second.clone()],
            destination.path(),
            "corrupt.pdf",
        ))
        .unwrap();
    assert!(corrupt_service
        .execute_with_hooks(
            &corrupt_job.id,
            |_| {},
            PdfMergeHooks {
                corrupt_staging_before_verify: true,
                ..Default::default()
            },
        )
        .is_err());
    assert!(!destination.path().join("corrupt.pdf").exists());

    let (_app_data, state, cleanup_service) = service();
    let destination = tempdir().unwrap();
    let cleanup_job = cleanup_service
        .create_job(request(&[first, second], destination.path(), "cleanup.pdf"))
        .unwrap();
    assert_eq!(
        cleanup_service
            .execute_with_hooks(
                &cleanup_job.id,
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
    assert_eq!(
        state
            .database()
            .get_job(&cleanup_job.id)
            .unwrap()
            .unwrap()
            .state,
        JobState::Interrupted
    );
    let report = document_studio_lib::recovery::reconcile_startup(&state).unwrap();
    assert_eq!(report.completed_publications, 1);
    assert_eq!(
        state
            .database()
            .get_job(&cleanup_job.id)
            .unwrap()
            .unwrap()
            .state,
        JobState::Completed
    );
}

fn percentile_95(values: &[Duration]) -> Duration {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len().saturating_sub(1)]
}

fn working_set_bytes() -> usize {
    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    // SAFETY: the pseudo-handle is valid for this process and counters has the declared size.
    let read = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };
    assert_ne!(read, 0);
    counters.WorkingSetSize
}

#[test]
#[ignore = "manual bounded G02 performance evidence"]
fn measure_g02_performance_on_reference_machine() {
    let manager_samples = (0..10)
        .map(|_| {
            let root = tempdir().unwrap();
            let started = Instant::now();
            let manager = QpdfRuntimeManager::new(bundle_root(), root.path().join("engines"));
            let elapsed = started.elapsed();
            drop(manager);
            elapsed
        })
        .collect::<Vec<_>>();

    let source = tempdir().unwrap();
    let destination = tempdir().unwrap();
    let small_inputs = (0..10)
        .map(|index| {
            support::write_multi_page_pdf_fixture(
                source.path(),
                &format!("small-{index}.pdf"),
                &format!("SMALL-{index}"),
                10,
            )
        })
        .collect::<Vec<_>>();
    let inspect_request = FilesInspectRequest {
        paths: small_inputs[..2]
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
    };
    let inspect_samples = (0..10)
        .map(|_| {
            let started = Instant::now();
            assert_eq!(
                document_studio_lib::ipc::files_inspect(inspect_request.clone())
                    .unwrap()
                    .len(),
                2
            );
            started.elapsed()
        })
        .collect::<Vec<_>>();

    let (_app_data, state, service) = service();
    let baseline_working_set = working_set_bytes();
    let stop_sampling = Arc::new(AtomicBool::new(false));
    let peak_parent = Arc::new(std::sync::atomic::AtomicUsize::new(baseline_working_set));
    let sampler_stop = stop_sampling.clone();
    let sampler_peak = peak_parent.clone();
    let sampler = std::thread::spawn(move || {
        while !sampler_stop.load(Ordering::Acquire) {
            sampler_peak.fetch_max(working_set_bytes(), Ordering::AcqRel);
            std::thread::sleep(Duration::from_millis(5));
        }
    });

    let process_peaks = Arc::new(Mutex::new(Vec::new()));
    let mut preflight_samples = Vec::new();
    let mut small_merge_samples = Vec::new();
    for iteration in 0..5 {
        let job = service
            .create_job(request(
                &small_inputs,
                destination.path(),
                &format!("small-merge-{iteration}.pdf"),
            ))
            .unwrap();
        let started = Instant::now();
        let mut merge_stage_started = None;
        let completed = service
            .execute_with_hooks(
                &job.id,
                |event| {
                    if event.message_code == "MERGING_PDFS" && merge_stage_started.is_none() {
                        merge_stage_started = Some(started.elapsed());
                    }
                },
                PdfMergeHooks {
                    merge_process_peak_memory: Some(process_peaks.clone()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(completed.state, JobState::Completed);
        preflight_samples.push(merge_stage_started.expect("merge stage event"));
        small_merge_samples.push(started.elapsed());
    }

    let representative_inputs = (0..20)
        .map(|index| {
            support::write_multi_page_pdf_fixture(
                source.path(),
                &format!("representative-{index}.pdf"),
                &format!("REPRESENTATIVE-{index}"),
                50,
            )
        })
        .collect::<Vec<_>>();
    let mut representative_samples = Vec::new();
    for iteration in 0..5 {
        let job = service
            .create_job(request(
                &representative_inputs,
                destination.path(),
                &format!("representative-merge-{iteration}.pdf"),
            ))
            .unwrap();
        let started = Instant::now();
        assert_eq!(
            service
                .execute_with_hooks(
                    &job.id,
                    |_| {},
                    PdfMergeHooks {
                        merge_process_peak_memory: Some(process_peaks.clone()),
                        ..Default::default()
                    },
                )
                .unwrap()
                .state,
            JobState::Completed
        );
        representative_samples.push(started.elapsed());
    }

    let mut cancellation_ack = Vec::new();
    let mut cancellation_terminal = Vec::new();
    for iteration in 0..5 {
        let job = service
            .create_job(request(
                &small_inputs[..2],
                destination.path(),
                &format!("cancel-{iteration}.pdf"),
            ))
            .unwrap();
        let process_started = Arc::new(AtomicBool::new(false));
        let worker_service = service.clone();
        let worker_id = job.id.clone();
        let worker_signal = process_started.clone();
        let worker = std::thread::spawn(move || {
            worker_service.execute_with_hooks(
                &worker_id,
                |_| {},
                PdfMergeHooks {
                    pause_after_merge_process_start: Some(Duration::from_secs(5)),
                    merge_process_started: Some(worker_signal),
                    ..Default::default()
                },
            )
        });
        let wait_started = Instant::now();
        while !process_started.load(Ordering::Acquire) {
            assert!(wait_started.elapsed() < Duration::from_secs(30));
            std::thread::sleep(Duration::from_millis(1));
        }
        let cancel_started = Instant::now();
        assert_eq!(
            state.cancellations.request(&job.id),
            document_studio_lib::app_state::CancelOutcome::Requested
        );
        state.database().request_cancellation(&job.id).unwrap();
        cancellation_ack.push(cancel_started.elapsed());
        let terminal_started = Instant::now();
        assert_eq!(worker.join().unwrap().unwrap().state, JobState::Cancelled);
        cancellation_terminal.push(terminal_started.elapsed());
    }

    stop_sampling.store(true, Ordering::Release);
    sampler.join().unwrap();
    let parent_growth = peak_parent
        .load(Ordering::Acquire)
        .saturating_sub(baseline_working_set);
    let qpdf_peak = process_peaks
        .lock()
        .unwrap()
        .iter()
        .copied()
        .max()
        .unwrap_or(0);
    let source_bytes = representative_inputs
        .iter()
        .map(|path| fs::metadata(path).unwrap().len())
        .sum::<u64>();

    println!(
        "G02_PERF manager_p95_ms={} inspect_2_p95_ms={} preflight_10x10_p95_ms={} small_merge_10x10_p95_ms={} representative_20x50_1000_pages_source_bytes={} representative_p95_ms={} parent_growth_bytes={} qpdf_peak_bytes={} cancel_ack_p95_ms={} cancel_terminal_p95_ms={}",
        percentile_95(&manager_samples).as_millis(),
        percentile_95(&inspect_samples).as_millis(),
        percentile_95(&preflight_samples).as_millis(),
        percentile_95(&small_merge_samples).as_millis(),
        source_bytes,
        percentile_95(&representative_samples).as_millis(),
        parent_growth,
        qpdf_peak,
        percentile_95(&cancellation_ack).as_millis(),
        percentile_95(&cancellation_terminal).as_millis(),
    );
}
