use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{SecondsFormat, Utc};

use crate::app_state::{AppState, CancellationToken};
use crate::contracts::{
    CorePdfJobCreateRequest, JobInput, JobOutput, JobProgress, JobRecord, JobState, OperationError,
    OperationStage, OutputStatus, ProgressEvent, ProgressUnit, CORE_PDF_OPERATION_VERSION,
    PDF_ROTATE_OPERATION_ID,
};
use crate::database::DatabaseError;
use crate::page_plan::{validate_plan, PlannedOutput, ValidatedPagePlan};
use crate::path_policy::{canonical_directory, canonical_regular_file};
use crate::pdf_merge::{qpdf_page_count, run_qpdf, verify_qpdf_version};
use crate::process_sandbox::{
    authorize_qpdf_paths, ensure_production_profile, validate_command_line_budget,
    QPDF_PROCESS_TIMEOUT,
};
use crate::publication::{
    hash_file, is_exact_owned_partial_path, partial_ownership_result_code,
    publish_verified_staging_with_observer, PublicationContext, PublicationError,
};
use crate::qpdf::{
    interpret_encryption_check_exit, interpret_structural_check_exit, EncryptionCheckOutcome,
    StructuralCheckOutcome, VerifiedQpdfRuntime,
};
use crate::viewer_sessions::ViewerJobSource;
use crate::windows_security::{delete_open_file, identity_from_file, open_for_identity_and_delete};
use crate::workspace::JobWorkspace;

const SNAPSHOT_RELATIVE_PATH: &str = r"inputs\source-0000.pdf";
const NORMALIZED_RELATIVE_PATH: &str = r"temp\rotation-normalized.pdf";
const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MIN_PDF_SIZE: u64 = 8;

#[derive(Clone)]
pub struct PdfPageOperationService {
    state: AppState,
    hooks: PdfPageOperationHooks,
}

#[derive(Clone)]
pub struct PdfPageOperationHooks {
    pub after_output_published: Arc<dyn Fn(usize) -> Result<(), OperationError> + Send + Sync>,
}

impl Default for PdfPageOperationHooks {
    fn default() -> Self {
        Self {
            after_output_published: Arc::new(|_| Ok(())),
        }
    }
}

impl PdfPageOperationService {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            hooks: PdfPageOperationHooks::default(),
        }
    }

    pub fn with_hooks(mut self, hooks: PdfPageOperationHooks) -> Self {
        self.hooks = hooks;
        self
    }

    pub fn create_job(
        &self,
        request: CorePdfJobCreateRequest,
    ) -> Result<(JobRecord, ViewerJobSource), OperationError> {
        let source = self
            .state
            .viewer_sessions
            .source_for_job(&request.viewer_session_id, request.viewer_generation)?;
        let destination = self
            .state
            .viewer_sessions
            .resolve_destination(&request.destination_grant_id)?;
        let validated = validate_plan(request.plan)?;
        let timestamp = timestamp();
        let id = uuid::Uuid::new_v4().to_string();
        let outputs = validated
            .outputs
            .iter()
            .enumerate()
            .map(|(ordinal, output)| JobOutput {
                ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
                requested_name: output.output_name.clone(),
                resolved_name: None,
                staging_path: None,
                partial_path: None,
                final_path: None,
                size_bytes: None,
                mime_type: "application/pdf".to_owned(),
                sha256: None,
                status: OutputStatus::Planned,
                verified_at: None,
                published_at: None,
            })
            .collect::<Vec<_>>();
        let requested_output_name = outputs
            .first()
            .map(|output| output.requested_name.clone())
            .ok_or_else(invalid_plan)?;
        let job = JobRecord {
            id,
            operation_id: validated.stored.envelope.operation_id.clone(),
            operation_version: CORE_PDF_OPERATION_VERSION.to_owned(),
            state: JobState::Queued,
            stage: None,
            sequence: 0,
            progress: JobProgress {
                completed_units: 0,
                total_units: 0,
                unit: ProgressUnit::Steps,
            },
            destination_directory: destination.to_string_lossy().into_owned(),
            requested_output_name,
            resolved_output_name: None,
            cancellation_requested_at: None,
            created_at: timestamp.clone(),
            updated_at: timestamp,
            finished_at: None,
            version: 0,
            inputs: vec![JobInput {
                ordinal: 0,
                display_name: source.display_name.clone(),
                source_path: source.path.to_string_lossy().into_owned(),
                canonical_path: source.path.to_string_lossy().into_owned(),
                file_identity: source.file_identity.clone(),
                size_bytes: source.size_bytes,
                modified_at: source.modified_at.clone(),
                mime_type: "application/pdf".to_owned(),
                sha256: None,
                password_reference: None,
            }],
            outputs,
            errors: Vec::new(),
        };
        self.state
            .database()
            .create_job_with_plan(&job, &validated.stored)
            .map_err(|_| metadata_error())?;
        Ok((job, source))
    }

    pub fn execute_with_registered_token<F>(
        &self,
        job_id: &str,
        source: ViewerJobSource,
        token: CancellationToken,
        mut on_event: F,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let result = self.execute_inner(job_id, &source, &token, &mut on_event);
        let finish_result = match &result {
            Err(error) => self.finish_unsuccessful(job_id, error, &mut on_event),
            Ok(_) => Ok(()),
        };
        self.state.cancellations.unregister(job_id);
        finish_result?;
        result
    }

    fn execute_inner<F>(
        &self,
        job_id: &str,
        source: &ViewerJobSource,
        token: &CancellationToken,
        on_event: &mut F,
    ) -> Result<JobRecord, OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        self.transition(
            job_id,
            JobState::Queued,
            JobState::Inspecting,
            OperationStage::Inspect,
        )?;
        self.progress(
            job_id,
            JobState::Inspecting,
            OperationStage::Inspect,
            0,
            1,
            "INSPECTING_SOURCE",
            "Revalidating the retained PDF source handle",
            true,
            on_event,
        )?;
        check_cancelled(token, OperationStage::Inspect)?;
        let stored = self
            .state
            .database()
            .get_operation_plan(job_id)
            .map_err(operation_plan_load_error)?
            .ok_or_else(metadata_error)?;
        let validated = validate_plan(stored.envelope.clone())?;
        if validated.stored.canonical_json != stored.canonical_json
            || validated.stored.sha256 != stored.sha256
        {
            return Err(invalid_plan());
        }
        validate_output_command_lines(&validated)?;
        let workspace = self
            .state
            .workspaces
            .create_job(job_id)
            .map_err(|_| workspace_error())?;
        let snapshot = workspace.root.join(SNAPSHOT_RELATIVE_PATH);

        self.transition(
            job_id,
            JobState::Inspecting,
            JobState::Preflight,
            OperationStage::Preflight,
        )?;
        self.progress(
            job_id,
            JobState::Preflight,
            OperationStage::Preflight,
            0,
            source.size_bytes,
            "SNAPSHOTTING_SOURCE",
            "Creating one private ASCII-path PDF snapshot from the retained handle",
            true,
            on_event,
        )?;
        let (snapshot_size, snapshot_hash) = source.copy_snapshot(&snapshot, token)?;
        if snapshot_size != source.size_bytes {
            return Err(source_changed());
        }
        self.state
            .database()
            .update_input_hash(job_id, 0, &snapshot_hash)
            .map_err(|_| metadata_error())?;

        let runtime = self
            .state
            .qpdf
            .as_ref()
            .ok_or_else(dependency_error)?
            .get_or_prepare()
            .map_err(|_| dependency_error())?;
        let profile = ensure_production_profile().map_err(|_| dependency_error())?;
        authorize_qpdf_paths(&profile, &runtime.bin, &workspace).map_err(|_| dependency_error())?;
        verify_qpdf_version(&runtime, &workspace, token)?;
        reject_encrypted(&runtime, &workspace, token)?;
        strict_check(
            &runtime,
            &workspace,
            Path::new(SNAPSHOT_RELATIVE_PATH),
            token,
        )?;
        let fresh_page_count = qpdf_page_count(
            &runtime,
            &workspace,
            Path::new(SNAPSHOT_RELATIVE_PATH),
            token,
            OperationStage::Preflight,
        )?;
        if fresh_page_count != u64::from(validated.stored.envelope.source_page_count) {
            return Err(OperationError::safe(
                "SOURCE_PAGE_COUNT_CHANGED",
                "The PDF page count changed",
                "Close the document, open it again, and rebuild the page plan.",
                OperationStage::Preflight,
                true,
            ));
        }

        self.transition(
            job_id,
            JobState::Preflight,
            JobState::Ready,
            OperationStage::Plan,
        )?;
        check_cancelled(token, OperationStage::Plan)?;
        self.transition(
            job_id,
            JobState::Ready,
            JobState::Running,
            OperationStage::Execute,
        )?;

        let input_relative = if validated.stored.envelope.operation_id == PDF_ROTATE_OPERATION_ID {
            normalize_rotations(&runtime, &workspace, token)?;
            Path::new(NORMALIZED_RELATIVE_PATH)
        } else {
            Path::new(SNAPSHOT_RELATIVE_PATH)
        };
        for (ordinal, output) in validated.outputs.iter().enumerate() {
            check_cancelled(token, OperationStage::Execute)?;
            self.progress(
                job_id,
                JobState::Running,
                OperationStage::Execute,
                ordinal as u64,
                validated.outputs.len() as u64,
                "WRITING_PAGE_OUTPUTS",
                "Writing verified page-plan outputs in the private staging area",
                true,
                on_event,
            )?;
            let output_relative = output_relative_path(ordinal)?;
            let arguments =
                build_page_output_arguments(input_relative, &output_relative, output, &validated)?;
            let execution = run_qpdf(
                &runtime,
                &workspace,
                &arguments,
                token,
                QPDF_PROCESS_TIMEOUT,
                OperationStage::Execute,
            )?;
            if execution.exit_code != 0 {
                return Err(process_error(OperationStage::Execute));
            }
        }

        self.transition(
            job_id,
            JobState::Running,
            JobState::Verifying,
            OperationStage::Verify,
        )?;
        let mut verified = Vec::with_capacity(validated.outputs.len());
        for (ordinal, output) in validated.outputs.iter().enumerate() {
            check_cancelled(token, OperationStage::Verify)?;
            self.progress(
                job_id,
                JobState::Verifying,
                OperationStage::Verify,
                ordinal as u64,
                validated.outputs.len() as u64,
                "VERIFYING_PAGE_OUTPUTS",
                "Strictly verifying every staged PDF before any publication",
                true,
                on_event,
            )?;
            let output_relative = output_relative_path(ordinal)?;
            let path = workspace.root.join(&output_relative);
            let (size, hash) = verify_output(
                &runtime,
                &workspace,
                &output_relative,
                &path,
                output.page_indexes.len() as u64,
                if validated.stored.envelope.operation_id == PDF_ROTATE_OPERATION_ID {
                    Some(&validated)
                } else {
                    None
                },
                token,
            )?;
            self.state
                .database()
                .set_output_staging_at(
                    job_id,
                    ordinal as u32,
                    &path.to_string_lossy(),
                    size,
                    &hash,
                    &timestamp(),
                )
                .map_err(|_| metadata_error())?;
            verified.push((path, size, hash));
        }

        check_cancelled(token, OperationStage::Publish)?;
        let destination =
            canonical_directory(Path::new(&self.current_job(job_id)?.destination_directory))
                .map_err(|_| publication_error())?;
        let source_path = source.path.as_path();
        for (ordinal, ((staging_path, verified_size, verified_hash), output)) in
            verified.iter().zip(validated.outputs.iter()).enumerate()
        {
            let state_for_reservation = self.state.clone();
            let state_for_activation = self.state.clone();
            let destination_for_activation = destination.clone();
            let state_for_release = self.state.clone();
            let state_for_intent = self.state.clone();
            let token_for_commit = token.clone();
            let ordinal_u32 = ordinal as u32;
            let result = publish_verified_staging_with_observer(
                PublicationContext {
                    staging_path,
                    input_paths: &[source_path],
                    destination_directory: &destination,
                    requested_name: &output.output_name,
                    job_id,
                },
                || token.is_cancelled(),
                |_, _| Ok(()),
                move |candidate, partial, resolved_name, size, sha256| {
                    state_for_reservation
                        .database()
                        .reserve_publication_attempt_at(
                            job_id,
                            ordinal_u32,
                            resolved_name,
                            &candidate.to_string_lossy(),
                            &partial.to_string_lossy(),
                            size,
                            sha256,
                        )
                        .map_err(database_publication_error)
                },
                move |partial, identity| {
                    let ownership = partial_ownership_result_code(
                        &destination_for_activation,
                        job_id,
                        partial,
                        identity,
                    )
                    .ok_or_else(publication_io_error)?;
                    state_for_activation
                        .database()
                        .activate_owned_partial_at(
                            job_id,
                            ordinal_u32,
                            &partial.to_string_lossy(),
                            &ownership,
                        )
                        .map_err(database_publication_error)
                },
                move |partial| {
                    state_for_release
                        .database()
                        .clear_owned_partial_at(job_id, ordinal_u32, &partial.to_string_lossy())
                        .map_err(database_publication_error)
                },
                move |candidate| {
                    let already_started = token_for_commit.commit_started();
                    if !token_for_commit.try_begin_publication_commit() {
                        return Err(PublicationError::Cancelled);
                    }
                    let resolved_name = candidate
                        .file_name()
                        .and_then(|name| name.to_str())
                        .ok_or_else(publication_io_error)?;
                    let write = if already_started {
                        state_for_intent.database().set_publication_intent_at(
                            job_id,
                            ordinal_u32,
                            resolved_name,
                            &candidate.to_string_lossy(),
                            *verified_size,
                            verified_hash,
                        )
                    } else {
                        state_for_intent.database().begin_publication_at(
                            job_id,
                            ordinal_u32,
                            resolved_name,
                            &candidate.to_string_lossy(),
                            *verified_size,
                            verified_hash,
                        )
                    };
                    write.map_err(database_publication_error)
                },
            )
            .map_err(|error| map_publication_error(error, ordinal))?;
            self.state
                .database()
                .set_output_published_at(
                    job_id,
                    ordinal_u32,
                    &result.resolved_name,
                    &result.final_path.to_string_lossy(),
                    result.size_bytes,
                    &result.sha256,
                    Some(&result.owned_partial_path.to_string_lossy()),
                )
                .map_err(|_| metadata_error())?;
            (self.hooks.after_output_published)(ordinal)?;
        }

        self.progress(
            job_id,
            JobState::Publishing,
            OperationStage::Cleanup,
            validated.outputs.len() as u64,
            validated.outputs.len() as u64,
            "CLEANING_PAGE_WORKSPACE",
            "Removing private snapshots and verified staging files",
            false,
            on_event,
        )?;
        self.cleanup_workspace_and_staging(job_id)?;
        let completed = self.transition(
            job_id,
            JobState::Publishing,
            JobState::Completed,
            OperationStage::Cleanup,
        )?;
        emit(
            &completed,
            OperationStage::Cleanup,
            "CORE_PDF_COMPLETED",
            "Every verified output has been published",
            false,
            on_event,
        );
        Ok(completed)
    }

    fn finish_unsuccessful<F>(
        &self,
        job_id: &str,
        error: &OperationError,
        on_event: &mut F,
    ) -> Result<(), OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        let before = self.current_job(job_id)?;
        let published = before
            .outputs
            .iter()
            .filter(|output| output.status == OutputStatus::Published)
            .count();
        let cleanup = self
            .cleanup_partials(job_id)
            .and_then(|()| self.cleanup_workspace_and_staging(job_id));
        let mut database = self.state.database();
        database
            .record_error_once(job_id, error)
            .map_err(|_| metadata_error())?;
        if published > 0 {
            database
                .record_error_once(
                    job_id,
                    &partial_publication_error(published, before.outputs.len()),
                )
                .map_err(|_| metadata_error())?;
        }
        if cleanup.is_err() {
            database
                .record_error_once(job_id, &cleanup_error())
                .map_err(|_| metadata_error())?;
            let current = database
                .get_job(job_id)
                .map_err(|_| metadata_error())?
                .ok_or_else(metadata_error)?;
            if !current.state.is_terminal() && current.state != JobState::Interrupted {
                database
                    .mark_interrupted(job_id, current.state)
                    .map_err(|_| metadata_error())?;
            }
            return Ok(());
        }
        for output in before
            .outputs
            .iter()
            .filter(|output| output.status != OutputStatus::Published)
        {
            database
                .clear_unpublished_intent_at(job_id, output.ordinal)
                .map_err(|_| metadata_error())?;
        }
        let current = database
            .get_job(job_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(metadata_error)?;
        if current.state.is_terminal() {
            return Ok(());
        }
        let terminal = if error.code == "CANCELLED" && published == 0 {
            JobState::Cancelled
        } else {
            JobState::Failed
        };
        database
            .transition_job(
                job_id,
                current.state,
                current.version,
                terminal,
                Some(OperationStage::Cleanup),
            )
            .map_err(|_| metadata_error())?;
        let finished = database
            .get_job(job_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(metadata_error)?;
        emit(
            &finished,
            OperationStage::Cleanup,
            if terminal == JobState::Cancelled {
                "CORE_PDF_CANCELLED"
            } else if published > 0 {
                "PARTIAL_PUBLICATION"
            } else {
                "CORE_PDF_FAILED"
            },
            if published > 0 {
                "Processing failed after one or more verified outputs were already published"
            } else {
                "No unverified output was published"
            },
            false,
            on_event,
        );
        Ok(())
    }

    fn cleanup_partials(&self, job_id: &str) -> Result<(), OperationError> {
        let job = self.current_job(job_id)?;
        let destination = Path::new(&job.destination_directory);
        for output in &job.outputs {
            let Some(partial_path) = output.partial_path.as_deref() else {
                continue;
            };
            let partial = Path::new(partial_path);
            if !is_exact_owned_partial_path(destination, job_id, partial) {
                return Err(cleanup_error());
            }
            match open_for_identity_and_delete(partial) {
                Ok(file) => {
                    let identity = identity_from_file(&file).map_err(|_| cleanup_error())?;
                    let ownership =
                        partial_ownership_result_code(destination, job_id, partial, identity)
                            .ok_or_else(cleanup_error)?;
                    if self
                        .state
                        .database()
                        .owned_partial_is_activated_at(
                            job_id,
                            output.ordinal,
                            partial_path,
                            &ownership,
                        )
                        .map_err(|_| metadata_error())?
                    {
                        delete_open_file(file).map_err(|_| cleanup_error())?;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(cleanup_error()),
            }
            self.state
                .database()
                .clear_owned_partial_at(job_id, output.ordinal, partial_path)
                .map_err(|_| metadata_error())?;
        }
        Ok(())
    }

    fn cleanup_workspace_and_staging(&self, job_id: &str) -> Result<(), OperationError> {
        let job = self.current_job(job_id)?;
        self.state
            .workspaces
            .cleanup_job(job_id)
            .map_err(|_| cleanup_error())?;
        let mut database = self.state.database();
        for output in &job.outputs {
            database
                .clear_staging_path_at(job_id, output.ordinal, output.staging_path.as_deref())
                .map_err(|_| metadata_error())?;
        }
        Ok(())
    }

    fn transition(
        &self,
        job_id: &str,
        expected: JobState,
        next: JobState,
        stage: OperationStage,
    ) -> Result<JobRecord, OperationError> {
        let current = self.current_job(job_id)?;
        if current.state != expected {
            return Err(metadata_error());
        }
        self.state
            .database()
            .transition_job(job_id, expected, current.version, next, Some(stage))
            .map_err(|_| metadata_error())?;
        self.current_job(job_id)
    }

    #[allow(clippy::too_many_arguments)]
    fn progress<F>(
        &self,
        job_id: &str,
        state: JobState,
        stage: OperationStage,
        completed: u64,
        total: u64,
        code: &str,
        message: &str,
        cancellable: bool,
        on_event: &mut F,
    ) -> Result<(), OperationError>
    where
        F: FnMut(ProgressEvent),
    {
        self.state
            .database()
            .update_progress(job_id, state, stage, completed, total, ProgressUnit::Items)
            .map_err(|_| metadata_error())?;
        let job = self.current_job(job_id)?;
        emit(&job, stage, code, message, cancellable, on_event);
        Ok(())
    }

    fn current_job(&self, job_id: &str) -> Result<JobRecord, OperationError> {
        self.state
            .database()
            .get_job(job_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(metadata_error)
    }
}

pub fn build_page_output_arguments(
    input_relative: &Path,
    output_relative: &Path,
    output: &PlannedOutput,
    plan: &ValidatedPagePlan,
) -> Result<Vec<OsString>, OperationError> {
    if !is_safe_relative_pdf(input_relative)
        || !is_safe_relative_pdf(output_relative)
        || output.page_indexes.is_empty()
    {
        return Err(process_error(OperationStage::Plan));
    }
    let mut arguments = [
        "--empty",
        "--suppress-recovery",
        "--stream-data=preserve",
        "--object-streams=preserve",
        "--remove-info",
        "--remove-metadata",
        "--remove-page-labels",
    ]
    .into_iter()
    .map(OsString::from)
    .collect::<Vec<_>>();
    if plan.stored.envelope.operation_id == PDF_ROTATE_OPERATION_ID {
        for degrees in [90_u16, 180, 270] {
            let pages = plan
                .rotations
                .iter()
                .filter_map(|(page_index, rotation)| {
                    (rotation.degrees() == degrees).then_some(*page_index + 1)
                })
                .collect::<Vec<_>>();
            if !pages.is_empty() {
                arguments.push(OsString::from(format!(
                    "--rotate=+{degrees}:{}",
                    compact_page_ranges(&pages)
                )));
            }
        }
    }
    arguments.push(OsString::from("--pages"));
    let mut input = OsString::from("--file=");
    input.push(input_relative);
    arguments.push(input);
    let pages = output
        .page_indexes
        .iter()
        .map(|index| index + 1)
        .collect::<Vec<_>>();
    let pages = compact_page_ranges(&pages);
    arguments.push(OsString::from(format!("--range={pages}")));
    arguments.push(OsString::from("--"));
    arguments.push(output_relative.as_os_str().to_owned());
    Ok(arguments)
}

fn compact_page_ranges(pages: &[u32]) -> String {
    let mut ranges = Vec::new();
    let mut start = pages[0];
    let mut end = start;
    for page in pages.iter().copied().skip(1) {
        if page == end + 1 {
            end = page;
            continue;
        }
        ranges.push(if start == end {
            start.to_string()
        } else {
            format!("{start}-{end}")
        });
        start = page;
        end = page;
    }
    ranges.push(if start == end {
        start.to_string()
    } else {
        format!("{start}-{end}")
    });
    ranges.join(",")
}

fn validate_output_command_lines(plan: &ValidatedPagePlan) -> Result<(), OperationError> {
    let input = if plan.stored.envelope.operation_id == PDF_ROTATE_OPERATION_ID {
        Path::new(NORMALIZED_RELATIVE_PATH)
    } else {
        Path::new(SNAPSHOT_RELATIVE_PATH)
    };
    for (ordinal, output) in plan.outputs.iter().enumerate() {
        let output_relative = output_relative_path(ordinal)?;
        let arguments = build_page_output_arguments(input, &output_relative, output, plan)?;
        validate_command_line_budget(std::ffi::OsStr::new("qpdf.exe"), &arguments)
            .map_err(|_| command_line_too_long())?;
    }
    Ok(())
}

fn normalize_rotations(
    runtime: &VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    token: &CancellationToken,
) -> Result<(), OperationError> {
    let arguments = [
        OsString::from(SNAPSHOT_RELATIVE_PATH),
        OsString::from("--suppress-recovery"),
        OsString::from("--stream-data=preserve"),
        OsString::from("--object-streams=preserve"),
        OsString::from("--remove-info"),
        OsString::from("--remove-metadata"),
        OsString::from("--remove-page-labels"),
        OsString::from("--flatten-rotation"),
        OsString::from(NORMALIZED_RELATIVE_PATH),
    ];
    let execution = run_qpdf(
        runtime,
        workspace,
        &arguments,
        token,
        QPDF_PROCESS_TIMEOUT,
        OperationStage::Execute,
    )?;
    if execution.exit_code != 0 {
        return Err(process_error(OperationStage::Execute));
    }
    Ok(())
}

fn reject_encrypted(
    runtime: &VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    token: &CancellationToken,
) -> Result<(), OperationError> {
    let execution = run_qpdf(
        runtime,
        workspace,
        &[
            OsString::from(SNAPSHOT_RELATIVE_PATH),
            OsString::from("--is-encrypted"),
        ],
        token,
        PREFLIGHT_TIMEOUT,
        OperationStage::Preflight,
    )?;
    match interpret_encryption_check_exit(execution.exit_code as i32) {
        Ok(EncryptionCheckOutcome::Unencrypted) => Ok(()),
        Ok(EncryptionCheckOutcome::Encrypted) => Err(OperationError::safe(
            "PDF_ENCRYPTED",
            "Encrypted PDFs cannot be structurally changed",
            "Viewing may use an in-memory password, but output operations are deferred until secure password transport is approved.",
            OperationStage::Preflight,
            false,
        )),
        Err(_) => Err(process_error(OperationStage::Preflight)),
    }
}

fn strict_check(
    runtime: &VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    relative: &Path,
    token: &CancellationToken,
) -> Result<(), OperationError> {
    let execution = run_qpdf(
        runtime,
        workspace,
        &[
            relative.as_os_str().to_owned(),
            OsString::from("--suppress-recovery"),
            OsString::from("--check"),
        ],
        token,
        PREFLIGHT_TIMEOUT,
        OperationStage::Verify,
    )?;
    if interpret_structural_check_exit(execution.exit_code as i32)
        != Ok(StructuralCheckOutcome::Valid)
    {
        return Err(OperationError::safe(
            "PDF_STRUCTURE_INVALID",
            "The PDF failed strict structure validation",
            "No output was published; repair is outside G03.",
            OperationStage::Verify,
            false,
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn verify_output(
    runtime: &VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    relative: &Path,
    path: &Path,
    expected_pages: u64,
    rotation_plan: Option<&ValidatedPagePlan>,
    token: &CancellationToken,
) -> Result<(u64, String), OperationError> {
    let (canonical, _) =
        canonical_regular_file(path).map_err(|_| verification_error("OUTPUT_NOT_REGULAR"))?;
    let staging = canonical_directory(&workspace.staging)
        .map_err(|_| verification_error("OUTPUT_PATH_INVALID"))?;
    if canonical.parent() != Some(staging.as_path()) || !is_safe_relative_pdf(relative) {
        return Err(verification_error("OUTPUT_PATH_INVALID"));
    }
    let metadata = fs::metadata(&canonical).map_err(|_| verification_error("OUTPUT_MISSING"))?;
    if metadata.len() < MIN_PDF_SIZE {
        return Err(verification_error("OUTPUT_SIZE_INVALID"));
    }
    let mut file =
        File::open(&canonical).map_err(|_| verification_error("OUTPUT_REOPEN_FAILED"))?;
    let mut header = [0_u8; 1024];
    let read = file
        .read(&mut header)
        .map_err(|_| verification_error("OUTPUT_REOPEN_FAILED"))?;
    if !header[..read].windows(5).any(|window| window == b"%PDF-") {
        return Err(verification_error("OUTPUT_NOT_PDF"));
    }
    let (size, sha256) =
        hash_file(&canonical).map_err(|_| verification_error("OUTPUT_HASH_FAILED"))?;
    if size != metadata.len() {
        return Err(verification_error("OUTPUT_SIZE_INVALID"));
    }
    strict_check(runtime, workspace, relative, token)?;
    let encryption = run_qpdf(
        runtime,
        workspace,
        &[
            relative.as_os_str().to_owned(),
            OsString::from("--is-encrypted"),
        ],
        token,
        PREFLIGHT_TIMEOUT,
        OperationStage::Verify,
    )?;
    if interpret_encryption_check_exit(encryption.exit_code as i32)
        != Ok(EncryptionCheckOutcome::Unencrypted)
    {
        return Err(verification_error("OUTPUT_ENCRYPTION_INVALID"));
    }
    let pages = qpdf_page_count(runtime, workspace, relative, token, OperationStage::Verify)?;
    if pages != expected_pages {
        return Err(verification_error("OUTPUT_PAGE_COUNT_MISMATCH"));
    }
    if let Some(plan) = rotation_plan {
        verify_rotations(runtime, workspace, relative, plan, token)?;
    }
    Ok((size, sha256))
}

fn verify_rotations(
    runtime: &VerifiedQpdfRuntime,
    workspace: &JobWorkspace,
    relative: &Path,
    plan: &ValidatedPagePlan,
    token: &CancellationToken,
) -> Result<(), OperationError> {
    const ROTATION_VERIFICATION_BATCH: usize = 64;
    for (batch_index, rotations) in plan
        .rotations
        .chunks(ROTATION_VERIFICATION_BATCH)
        .enumerate()
    {
        let verification_relative =
            PathBuf::from(format!(r"temp\rotation-verification-{batch_index:04}.pdf"));
        let pages = rotations
            .iter()
            .map(|(page_index, _)| (page_index + 1).to_string())
            .collect::<Vec<_>>()
            .join(",");
        let mut create_arguments = [
            "--empty",
            "--suppress-recovery",
            "--stream-data=preserve",
            "--object-streams=preserve",
            "--pages",
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
        let mut source_argument = OsString::from("--file=");
        source_argument.push(relative);
        create_arguments.push(source_argument);
        create_arguments.push(OsString::from(format!("--range={pages}")));
        create_arguments.push(OsString::from("--"));
        create_arguments.push(verification_relative.as_os_str().to_owned());
        let created = run_qpdf(
            runtime,
            workspace,
            &create_arguments,
            token,
            QPDF_PROCESS_TIMEOUT,
            OperationStage::Verify,
        )?;
        if created.exit_code != 0 {
            return Err(verification_error("OUTPUT_ROTATION_MISMATCH"));
        }

        let pages_result = run_qpdf(
            runtime,
            workspace,
            &[
                verification_relative.as_os_str().to_owned(),
                OsString::from("--suppress-recovery"),
                OsString::from("--show-pages"),
            ],
            token,
            PREFLIGHT_TIMEOUT,
            OperationStage::Verify,
        )?;
        if pages_result.exit_code != 0 {
            return Err(verification_error("OUTPUT_ROTATION_MISMATCH"));
        }
        let references = parse_page_references(&pages_result.stdout, rotations.len())?;

        let mut json_arguments = ["--json=1", "--json-key=objects"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        for reference in &references {
            let fields = reference.split_whitespace().collect::<Vec<_>>();
            json_arguments.push(OsString::from(format!(
                "--json-object={},{}",
                fields[0], fields[1]
            )));
        }
        json_arguments.push(verification_relative.as_os_str().to_owned());
        let objects_result = run_qpdf(
            runtime,
            workspace,
            &json_arguments,
            token,
            PREFLIGHT_TIMEOUT,
            OperationStage::Verify,
        )?;
        if objects_result.exit_code != 0 {
            return Err(verification_error("OUTPUT_ROTATION_MISMATCH"));
        }
        let document: serde_json::Value = serde_json::from_slice(&objects_result.stdout)
            .map_err(|_| verification_error("OUTPUT_ROTATION_MISMATCH"))?;
        let objects = document
            .get("objects")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| verification_error("OUTPUT_ROTATION_MISMATCH"))?;
        for ((_, rotation), reference) in rotations.iter().zip(references) {
            let page = objects
                .get(&reference)
                .and_then(serde_json::Value::as_object)
                .ok_or_else(|| verification_error("OUTPUT_ROTATION_MISMATCH"))?;
            if page.get("/Type").and_then(serde_json::Value::as_str) != Some("/Page")
                || page.get("/Rotate").and_then(serde_json::Value::as_u64)
                    != Some(u64::from(rotation.degrees()))
            {
                return Err(verification_error("OUTPUT_ROTATION_MISMATCH"));
            }
        }
    }
    Ok(())
}

fn parse_page_references(
    output: &[u8],
    expected_count: usize,
) -> Result<Vec<String>, OperationError> {
    let text =
        std::str::from_utf8(output).map_err(|_| verification_error("OUTPUT_ROTATION_MISMATCH"))?;
    let mut references = Vec::with_capacity(expected_count);
    for line in text.lines() {
        let Some((page, reference)) = line
            .strip_prefix("page ")
            .and_then(|line| line.split_once(": "))
        else {
            continue;
        };
        let fields = reference.split_whitespace().collect::<Vec<_>>();
        if page.parse::<usize>().ok() != Some(references.len() + 1)
            || fields.len() != 3
            || fields[2] != "R"
            || fields[0].parse::<u32>().is_err()
            || fields[1].parse::<u16>().is_err()
        {
            return Err(verification_error("OUTPUT_ROTATION_MISMATCH"));
        }
        references.push(reference.to_owned());
    }
    if references.len() != expected_count {
        return Err(verification_error("OUTPUT_ROTATION_MISMATCH"));
    }
    Ok(references)
}

fn output_relative_path(ordinal: usize) -> Result<PathBuf, OperationError> {
    let ordinal = u32::try_from(ordinal).map_err(|_| invalid_plan())?;
    Ok(PathBuf::from(format!(r"staging\output-{ordinal:04}.pdf")))
}

fn is_safe_relative_pdf(path: &Path) -> bool {
    !path.is_absolute()
        && path.as_os_str().is_ascii()
        && !path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::RootDir
            )
        })
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

fn check_cancelled(token: &CancellationToken, stage: OperationStage) -> Result<(), OperationError> {
    if token.is_cancelled() {
        return Err(OperationError::safe(
            "CANCELLED",
            "The PDF operation was cancelled",
            "Private snapshots and unpublished outputs will be removed safely.",
            stage,
            false,
        ));
    }
    Ok(())
}

fn emit<F>(
    job: &JobRecord,
    stage: OperationStage,
    code: &str,
    message: &str,
    cancellable: bool,
    on_event: &mut F,
) where
    F: FnMut(ProgressEvent),
{
    on_event(ProgressEvent {
        schema_version: 1,
        sequence: job.sequence,
        emitted_at: timestamp(),
        job_id: job.id.clone(),
        operation_id: job.operation_id.clone(),
        state: job.state,
        stage,
        completed_units: job.progress.completed_units,
        total_units: job.progress.total_units,
        unit: job.progress.unit,
        message_code: code.to_owned(),
        message: message.to_owned(),
        cancellable,
    });
}

fn map_publication_error(error: PublicationError, ordinal: usize) -> OperationError {
    match error {
        PublicationError::Cancelled => OperationError::safe(
            "CANCELLED",
            "The PDF operation was cancelled",
            "Private snapshots and unpublished outputs will be removed safely.",
            OperationStage::Publish,
            false,
        ),
        _ => OperationError::safe(
            "OUTPUT_PUBLICATION_FAILED",
            "A verified output could not be published",
            format!(
                "Output {} was not published. Any earlier published outputs remain in the destination.",
                ordinal + 1
            ),
            OperationStage::Publish,
            true,
        ),
    }
}

fn database_publication_error(_: DatabaseError) -> PublicationError {
    publication_io_error()
}

fn publication_io_error() -> PublicationError {
    PublicationError::Io(std::io::Error::other(
        "publication metadata could not be stored",
    ))
}

fn partial_publication_error(published: usize, expected: usize) -> OperationError {
    OperationError::safe(
        "PARTIAL_PUBLICATION",
        "Only some split outputs were published",
        format!(
            "{published} of {expected} verified outputs were published. Published user files were preserved."
        ),
        OperationStage::Publish,
        false,
    )
}

fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn invalid_plan() -> OperationError {
    OperationError::safe(
        "PAGE_PLAN_INVALID",
        "The page operation plan is not valid",
        "Rebuild the page selection and output settings.",
        OperationStage::Plan,
        false,
    )
}

fn metadata_error() -> OperationError {
    OperationError::safe(
        "METADATA_WRITE_FAILED",
        "Job metadata could not be read or saved",
        "The operation cannot continue until local metadata is available.",
        OperationStage::Audit,
        true,
    )
}

fn operation_plan_load_error(error: DatabaseError) -> OperationError {
    if matches!(error, DatabaseError::OperationPlanMismatch) {
        OperationError::safe(
            "PLAN_OPERATION_MISMATCH",
            "The saved page plan does not match its job",
            "The operation stopped before creating temporary files or starting qpdf.",
            OperationStage::Plan,
            false,
        )
    } else {
        metadata_error()
    }
}

fn command_line_too_long() -> OperationError {
    OperationError::safe(
        "QPDF_COMMAND_LINE_TOO_LONG",
        "The page operation is too large to launch safely",
        "Reduce the page selection or split the operation into smaller jobs.",
        OperationStage::Plan,
        false,
    )
}

fn workspace_error() -> OperationError {
    OperationError::safe(
        "WORKSPACE_CREATE_FAILED",
        "The private job workspace could not be prepared",
        "Check local disk access and available space, then try again.",
        OperationStage::Preflight,
        true,
    )
}

fn source_changed() -> OperationError {
    OperationError::safe(
        "SOURCE_CHANGED",
        "The source PDF changed",
        "Open the PDF again before applying the page operation.",
        OperationStage::Execute,
        true,
    )
}

fn dependency_error() -> OperationError {
    OperationError::safe(
        "QPDF_UNAVAILABLE",
        "The bundled PDF engine is unavailable",
        "The accepted qpdf 12.3.2 bundle must pass provenance and runtime checks.",
        OperationStage::Preflight,
        true,
    )
}

fn process_error(stage: OperationStage) -> OperationError {
    OperationError::safe(
        "QPDF_PROCESS_FAILED",
        "The sandboxed PDF engine did not finish successfully",
        "No unverified output was published.",
        stage,
        true,
    )
}

fn verification_error(code: &str) -> OperationError {
    OperationError::safe(
        code,
        "The staged PDF failed independent verification",
        "The output was not published.",
        OperationStage::Verify,
        false,
    )
}

fn publication_error() -> OperationError {
    OperationError::safe(
        "DESTINATION_UNSAFE",
        "The destination folder is no longer safe",
        "Choose an existing local destination again.",
        OperationStage::Publish,
        true,
    )
}

fn cleanup_error() -> OperationError {
    OperationError::safe(
        "CLEANUP_FAILED",
        "Temporary data could not be completely removed",
        "The job remains interrupted so exact cleanup can be retried safely.",
        OperationStage::Recovery,
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::{build_page_output_arguments, output_relative_path};
    use crate::contracts::{
        CorePdfPlanPayload, OperationPlanEnvelope, OutputRotation, ReorderPagesPlan,
        StoredOperationPlan, PDF_ROTATE_OPERATION_ID,
    };
    use crate::page_plan::{validate_plan, PlannedOutput, ValidatedPagePlan};
    use crate::process_sandbox::validate_command_line_budget;
    use std::ffi::{OsStr, OsString};
    use std::path::Path;

    #[test]
    fn qpdf_page_selection_uses_direct_ascii_argv_and_one_based_ranges() {
        let plan = validate_plan(OperationPlanEnvelope {
            schema_version: 1,
            operation_id: "pdf.reorder-pages".to_owned(),
            source_page_count: 3,
            payload: CorePdfPlanPayload::Reorder(ReorderPagesPlan {
                ordered_page_indexes: vec![2, 0, 1],
                output_name: "reordered.pdf".to_owned(),
            }),
        })
        .unwrap();
        let args = build_page_output_arguments(
            Path::new(r"inputs\source-0000.pdf"),
            &output_relative_path(0).unwrap(),
            &plan.outputs[0],
            &plan,
        )
        .unwrap();
        assert!(args.contains(&OsString::from("--range=3,1-2")));
        assert_eq!(
            args.last(),
            Some(&OsString::from(r"staging\output-0000.pdf"))
        );
        assert!(args.iter().all(|argument| argument.as_os_str().is_ascii()));
    }

    fn synthetic_rotate_plan(rotation: impl Fn(u32) -> OutputRotation) -> ValidatedPagePlan {
        let page_count = 4096_u32;
        ValidatedPagePlan {
            stored: StoredOperationPlan {
                envelope: OperationPlanEnvelope {
                    schema_version: 1,
                    operation_id: PDF_ROTATE_OPERATION_ID.to_owned(),
                    source_page_count: page_count,
                    payload: CorePdfPlanPayload::Rotate(crate::contracts::RotatePagesPlan {
                        rotations: Vec::new(),
                        output_name: "rotated.pdf".to_owned(),
                    }),
                },
                canonical_json: "{}".to_owned(),
                sha256: "0".repeat(64),
                created_at: "2026-08-21T00:00:00.000Z".to_owned(),
            },
            outputs: vec![PlannedOutput {
                output_name: "rotated.pdf".to_owned(),
                page_indexes: (0..page_count).collect(),
            }],
            rotations: (0..page_count)
                .map(|page_index| (page_index, rotation(page_index)))
                .collect(),
        }
    }

    #[test]
    fn compact_rotation_arguments_cover_all_4096_pages_below_windows_budget() {
        let all_ninety = synthetic_rotate_plan(|_| OutputRotation::Clockwise90);
        let arguments = build_page_output_arguments(
            Path::new(r"inputs\source-0000.pdf"),
            Path::new(r"staging\output-0000.pdf"),
            &all_ninety.outputs[0],
            &all_ninety,
        )
        .unwrap();
        let text = arguments
            .iter()
            .map(|argument| argument.to_string_lossy())
            .collect::<Vec<_>>();
        assert!(text
            .iter()
            .any(|argument| argument == "--rotate=+90:1-4096"));
        assert!(text.iter().any(|argument| argument == "--range=1-4096"));
        assert!(validate_command_line_budget(OsStr::new("qpdf.exe"), &arguments).is_ok());

        let alternating = synthetic_rotate_plan(|page| match page % 3 {
            0 => OutputRotation::Clockwise90,
            1 => OutputRotation::Clockwise180,
            _ => OutputRotation::Clockwise270,
        });
        let arguments = build_page_output_arguments(
            Path::new(r"inputs\source-0000.pdf"),
            Path::new(r"staging\output-0000.pdf"),
            &alternating.outputs[0],
            &alternating,
        )
        .unwrap();
        assert_eq!(
            arguments
                .iter()
                .filter(|argument| argument.to_string_lossy().starts_with("--rotate="))
                .count(),
            3
        );
        assert!(validate_command_line_budget(OsStr::new("qpdf.exe"), &arguments).is_ok());
    }
}
