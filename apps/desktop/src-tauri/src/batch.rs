use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ADD_FILE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_FLAG_SEQUENTIAL_SCAN, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

use crate::app_state::AppState;
use crate::contracts::{
    BatchChildRecord, BatchCreateRequest, BatchDiskEstimate, BatchPreviewRequest,
    BatchPreviewResponse, BatchPreviewRow, BatchProgress, BatchRecord, BatchState, JobInput,
    JobOutput, JobProgress, JobRecord, JobState, OperationError, OperationSpecEnvelope,
    OperationStage, OutputStatus, ProgressUnit, StoredOperationSpec, BATCH_MAX_INPUTS,
    BATCH_NAMING_TEMPLATE_MAX_BYTES, BATCH_PREVIEW_MAX_BYTES, BATCH_PREVIEW_SCHEMA_VERSION,
    OPERATION_SPEC_SCHEMA_VERSION, PDF_COMPRESS_LOSSLESS_OPERATION_ID,
    PDF_COMPRESS_LOSSLESS_VERSION,
};
use crate::operation_registry::validate_batch_eligibility;
use crate::path_policy::{
    canonical_directory, canonical_regular_file, ensure_different_files, validate_output_name,
    windows_file_names_equal,
};
use crate::publication::{collision_name, MAX_COLLISION_ATTEMPTS};
use crate::windows_security::{available_bytes, file_identity, identity_from_file, FileIdentity};

const SPACE_MARGIN_MINIMUM: u64 = 64 * 1024 * 1024;
const HASH_BUFFER_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct BatchPreviewHooks {
    pub workspace_available_bytes: Option<u64>,
    pub destination_available_bytes: Option<u64>,
    pub destination_permission_denied: bool,
}

struct SourceSnapshot {
    original_path: String,
    canonical_path: PathBuf,
    fingerprint: CanonicalSourceFingerprint,
    _guard: File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalSourceFingerprint {
    ordinal: u32,
    display_name: String,
    canonical_path_sha256: String,
    file_identity: String,
    size_bytes: u64,
    modified_at: String,
    sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalDestinationFingerprint {
    file_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CollisionPlanEntry {
    ordinal: u32,
    requested_name: String,
    planned_name: String,
    collision_index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlanKeyEnvelope {
    schema_version: u8,
    operation_id: String,
    operation_version: String,
    settings_sha256: String,
    sources: Vec<CanonicalSourceFingerprint>,
    destination: CanonicalDestinationFingerprint,
    naming_template: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalPreviewEnvelope {
    schema_version: u8,
    operation_id: String,
    operation_version: String,
    settings_sha256: String,
    sources: Vec<CanonicalSourceFingerprint>,
    destination: CanonicalDestinationFingerprint,
    naming_template: String,
    collision_plan: Vec<CollisionPlanEntry>,
    disk_estimate: BatchDiskEstimate,
    optimistic_version: u64,
}

struct PreparedPreview {
    response: BatchPreviewResponse,
    destination_directory: PathBuf,
    destination_identity: FileIdentity,
    sources: Vec<SourceSnapshot>,
    collision_plan: Vec<CollisionPlanEntry>,
    plan_key_sha256: String,
    operation_spec: StoredOperationSpec,
    _destination_permission_guard: File,
}

#[derive(Clone)]
pub struct BatchPreviewService {
    state: AppState,
}

impl BatchPreviewService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub fn preview(
        &self,
        request: BatchPreviewRequest,
    ) -> Result<BatchPreviewResponse, OperationError> {
        Ok(self
            .prepare(&request, &BatchPreviewHooks::default())?
            .response)
    }

    #[doc(hidden)]
    pub fn preview_with_hooks(
        &self,
        request: BatchPreviewRequest,
        hooks: BatchPreviewHooks,
    ) -> Result<BatchPreviewResponse, OperationError> {
        Ok(self.prepare(&request, &hooks)?.response)
    }

    pub fn create(&self, request: BatchCreateRequest) -> Result<BatchRecord, OperationError> {
        self.create_with_hooks(request, BatchPreviewHooks::default())
    }

    #[doc(hidden)]
    pub fn create_with_hooks(
        &self,
        request: BatchCreateRequest,
        hooks: BatchPreviewHooks,
    ) -> Result<BatchRecord, OperationError> {
        if !is_sha256(&request.preview_sha256) {
            return Err(stale_preview());
        }
        let preview_request = BatchPreviewRequest::from(&request);
        let prepared = self.prepare(&preview_request, &hooks).map_err(|error| {
            if error.code == "BATCH_DEPENDENCY_UNAVAILABLE" {
                error
            } else {
                stale_preview()
            }
        })?;
        require_preview_proof(
            &request.preview_sha256,
            request.optimistic_version,
            &prepared.response,
        )?;

        self.recheck_destination(&prepared)?;
        self.check_available_space(&prepared, &hooks)?;

        let timestamp = now();
        let batch_id = Uuid::new_v4().hyphenated().to_string();
        let mut jobs = Vec::with_capacity(prepared.sources.len());
        let mut specs = Vec::with_capacity(prepared.sources.len());
        let mut children = Vec::with_capacity(prepared.sources.len());
        for (source, collision) in prepared.sources.iter().zip(&prepared.collision_plan) {
            let job_id = Uuid::new_v4().hyphenated().to_string();
            let job = JobRecord {
                id: job_id.clone(),
                operation_id: PDF_COMPRESS_LOSSLESS_OPERATION_ID.to_owned(),
                operation_version: PDF_COMPRESS_LOSSLESS_VERSION.to_owned(),
                state: JobState::Queued,
                stage: None,
                sequence: 0,
                progress: JobProgress {
                    completed_units: 0,
                    total_units: source.fingerprint.size_bytes,
                    unit: ProgressUnit::Bytes,
                },
                destination_directory: prepared
                    .destination_directory
                    .to_string_lossy()
                    .into_owned(),
                requested_output_name: collision.planned_name.clone(),
                resolved_output_name: None,
                cancellation_requested_at: None,
                created_at: timestamp.clone(),
                updated_at: timestamp.clone(),
                finished_at: None,
                version: 0,
                completion_kind: None,
                reason: None,
                inputs: vec![JobInput {
                    ordinal: 0,
                    display_name: source.fingerprint.display_name.clone(),
                    source_path: source.original_path.clone(),
                    canonical_path: source.canonical_path.to_string_lossy().into_owned(),
                    file_identity: source.fingerprint.file_identity.clone(),
                    size_bytes: source.fingerprint.size_bytes,
                    modified_at: source.fingerprint.modified_at.clone(),
                    mime_type: "application/pdf".to_owned(),
                    sha256: Some(source.fingerprint.sha256.clone()),
                    password_reference: None,
                }],
                outputs: vec![JobOutput {
                    ordinal: 0,
                    requested_name: collision.planned_name.clone(),
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
                }],
                errors: Vec::new(),
            };
            children.push(BatchChildRecord {
                ordinal: collision.ordinal,
                job_id,
                state: JobState::Queued,
                completion_kind: None,
                reason: None,
                requested_name: collision.requested_name.clone(),
                planned_name: collision.planned_name.clone(),
                collision_index: collision.collision_index,
                progress: job.progress.clone(),
            });
            jobs.push(job);
            specs.push(prepared.operation_spec.clone());
        }

        let total_children = u32::try_from(children.len()).map_err(|_| invalid_request())?;
        let batch = BatchRecord {
            id: batch_id,
            schema_version: BATCH_PREVIEW_SCHEMA_VERSION,
            operation_id: PDF_COMPRESS_LOSSLESS_OPERATION_ID.to_owned(),
            operation_version: PDF_COMPRESS_LOSSLESS_VERSION.to_owned(),
            state: BatchState::Queued,
            preview_sha256: prepared.response.preview_sha256.clone(),
            settings_sha256: prepared.response.settings_sha256.clone(),
            naming_template: prepared.response.naming_template.clone(),
            optimistic_version: prepared.response.optimistic_version,
            disk_estimate: prepared.response.disk_estimate.clone(),
            progress: BatchProgress {
                settled_children: 0,
                total_children,
                completed_children: 0,
                failed_children: 0,
                cancelled_children: 0,
                interrupted_children: 0,
                published_children: 0,
                no_benefit_children: 0,
            },
            created_at: timestamp.clone(),
            updated_at: timestamp,
            version: 0,
            children,
        };

        let write_result = self.state.database().create_batch_with_jobs(
            &batch,
            &jobs,
            &specs,
            &prepared.destination_directory,
            &prepared.destination_identity.to_string(),
            &prepared.plan_key_sha256,
        );
        match write_result {
            Ok(()) => {}
            Err(crate::database::DatabaseError::BatchPlanConflict) => {
                return Err(stale_preview());
            }
            Err(_) => return Err(metadata_error()),
        }
        self.state
            .database()
            .get_batch(&batch.id)
            .map_err(|_| metadata_error())?
            .ok_or_else(metadata_error)
    }

    pub fn get(&self, batch_id: &str) -> Result<BatchRecord, OperationError> {
        if Uuid::parse_str(batch_id).is_err() {
            return Err(batch_not_found());
        }
        self.state
            .database()
            .get_batch(batch_id)
            .map_err(|_| metadata_error())?
            .ok_or_else(batch_not_found)
    }

    fn prepare(
        &self,
        request: &BatchPreviewRequest,
        hooks: &BatchPreviewHooks,
    ) -> Result<PreparedPreview, OperationError> {
        validate_batch_request(request)?;
        self.validate_qpdf_available()?;
        let destination_directory = canonical_directory(Path::new(&request.destination_directory))
            .map_err(|_| path_error())?;
        let destination_identity =
            file_identity(&destination_directory).map_err(|_| path_error())?;
        if hooks.destination_permission_denied {
            return Err(destination_permission_error());
        }
        let destination_permission_guard = OpenOptions::new()
            .access_mode(FILE_ADD_FILE)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&destination_directory)
            .map_err(|_| destination_permission_error())?;
        if identity_from_file(&destination_permission_guard).map_err(|_| path_error())?
            != destination_identity
        {
            return Err(path_error());
        }

        let mut sources = Vec::with_capacity(request.input_paths.len());
        for (index, path) in request.input_paths.iter().enumerate() {
            sources.push(inspect_source(path, index)?);
        }
        let requested_names = render_requested_names(&request.naming_template, &sources)?;
        for (index, (source, requested_name)) in sources.iter().zip(&requested_names).enumerate() {
            ensure_different_files(
                &source.canonical_path,
                &destination_directory.join(requested_name),
            )
            .map_err(|_| input_alias_error(index))?;
        }

        let collision_plan = plan_collisions(&destination_directory, &requested_names)?;
        let disk_estimate = self.disk_estimate(&sources, destination_identity)?;
        let operation_spec_envelope = OperationSpecEnvelope {
            schema_version: OPERATION_SPEC_SCHEMA_VERSION,
            operation_id: request.operation_id.clone(),
            settings: json!({}),
        };
        let operation_spec_json =
            serde_json::to_string(&operation_spec_envelope).map_err(|_| invalid_request())?;
        let settings_sha256 = sha256_hex(b"{}");
        let operation_spec = StoredOperationSpec {
            envelope: operation_spec_envelope,
            sha256: sha256_hex(operation_spec_json.as_bytes()),
            canonical_json: operation_spec_json,
            created_at: now(),
        };
        let destination = CanonicalDestinationFingerprint {
            file_identity: destination_identity.to_string(),
        };
        let source_fingerprints = sources
            .iter()
            .map(|source| source.fingerprint.clone())
            .collect::<Vec<_>>();
        let plan_key_envelope = PlanKeyEnvelope {
            schema_version: BATCH_PREVIEW_SCHEMA_VERSION,
            operation_id: request.operation_id.clone(),
            operation_version: request.operation_version.clone(),
            settings_sha256: settings_sha256.clone(),
            sources: source_fingerprints.clone(),
            destination: destination.clone(),
            naming_template: request.naming_template.clone(),
        };
        let plan_key_bytes =
            serde_json::to_vec(&plan_key_envelope).map_err(|_| invalid_request())?;
        let plan_key_sha256 = sha256_hex(&plan_key_bytes);
        let optimistic_version = self
            .state
            .database()
            .next_batch_optimistic_version(&plan_key_sha256)
            .map_err(|_| metadata_error())?;
        let envelope = CanonicalPreviewEnvelope {
            schema_version: BATCH_PREVIEW_SCHEMA_VERSION,
            operation_id: request.operation_id.clone(),
            operation_version: request.operation_version.clone(),
            settings_sha256: settings_sha256.clone(),
            sources: source_fingerprints,
            destination,
            naming_template: request.naming_template.clone(),
            collision_plan: collision_plan.clone(),
            disk_estimate: disk_estimate.clone(),
            optimistic_version,
        };
        let canonical = canonical_preview_bytes(&envelope)?;
        if canonical.len() > BATCH_PREVIEW_MAX_BYTES {
            return Err(preview_too_large());
        }
        let canonical_size_bytes =
            u32::try_from(canonical.len()).map_err(|_| preview_too_large())?;
        let response = BatchPreviewResponse {
            schema_version: BATCH_PREVIEW_SCHEMA_VERSION,
            operation_id: request.operation_id.clone(),
            operation_version: request.operation_version.clone(),
            settings_sha256,
            naming_template: request.naming_template.clone(),
            rows: collision_plan
                .iter()
                .zip(&sources)
                .map(|(collision, source)| BatchPreviewRow {
                    ordinal: collision.ordinal,
                    source_name: source.fingerprint.display_name.clone(),
                    output_name: collision.planned_name.clone(),
                    collision_index: collision.collision_index,
                    size_bytes: source.fingerprint.size_bytes,
                })
                .collect(),
            disk_estimate,
            preview_sha256: sha256_hex(&canonical),
            canonical_size_bytes,
            optimistic_version,
        };
        Ok(PreparedPreview {
            response,
            destination_directory,
            destination_identity,
            sources,
            collision_plan,
            plan_key_sha256,
            operation_spec,
            _destination_permission_guard: destination_permission_guard,
        })
    }

    fn validate_qpdf_available(&self) -> Result<(), OperationError> {
        self.state
            .qpdf
            .as_ref()
            .ok_or_else(dependency_error)?
            .verify_available_read_only()
            .map_err(|_| dependency_error())?;
        Ok(())
    }

    fn disk_estimate(
        &self,
        sources: &[SourceSnapshot],
        destination_identity: FileIdentity,
    ) -> Result<BatchDiskEstimate, OperationError> {
        let workspace_identity =
            file_identity(self.state.workspaces.root()).map_err(|_| path_error())?;
        estimate_disk_sizes(
            sources.iter().map(|source| source.fingerprint.size_bytes),
            workspace_identity.volume_serial == destination_identity.volume_serial,
        )
    }

    fn recheck_destination(&self, prepared: &PreparedPreview) -> Result<(), OperationError> {
        let canonical =
            canonical_directory(&prepared.destination_directory).map_err(|_| stale_preview())?;
        let identity = file_identity(&canonical).map_err(|_| stale_preview())?;
        if canonical != prepared.destination_directory || identity != prepared.destination_identity
        {
            return Err(stale_preview());
        }
        let requested_names = prepared
            .collision_plan
            .iter()
            .map(|entry| entry.requested_name.clone())
            .collect::<Vec<_>>();
        if plan_collisions(&canonical, &requested_names).map_err(|_| stale_preview())?
            != prepared.collision_plan
        {
            return Err(stale_preview());
        }
        Ok(())
    }

    fn check_available_space(
        &self,
        prepared: &PreparedPreview,
        hooks: &BatchPreviewHooks,
    ) -> Result<(), OperationError> {
        let estimate = &prepared.response.disk_estimate;
        if estimate.workspace_and_destination_share_volume {
            let available = hooks.destination_available_bytes.unwrap_or(
                available_bytes(&prepared.destination_directory).map_err(|_| disk_check_error())?,
            );
            if available < estimate.combined_required_bytes {
                return Err(insufficient_space());
            }
            return Ok(());
        }
        let workspace_available = hooks.workspace_available_bytes.unwrap_or(
            available_bytes(self.state.workspaces.root()).map_err(|_| disk_check_error())?,
        );
        let destination_available = hooks.destination_available_bytes.unwrap_or(
            available_bytes(&prepared.destination_directory).map_err(|_| disk_check_error())?,
        );
        if workspace_available < estimate.workspace_peak_bytes
            || destination_available < estimate.destination_total_bytes
        {
            return Err(insufficient_space());
        }
        Ok(())
    }
}

fn canonical_preview_bytes(envelope: &CanonicalPreviewEnvelope) -> Result<Vec<u8>, OperationError> {
    serde_json::to_vec(envelope).map_err(|_| invalid_request())
}

fn require_preview_proof(
    expected_sha256: &str,
    expected_optimistic_version: u64,
    actual: &BatchPreviewResponse,
) -> Result<(), OperationError> {
    if actual.preview_sha256 != expected_sha256
        || actual.optimistic_version != expected_optimistic_version
    {
        return Err(stale_preview());
    }
    Ok(())
}

fn estimate_disk_sizes(
    sizes: impl IntoIterator<Item = u64>,
    shared_volume: bool,
) -> Result<BatchDiskEstimate, OperationError> {
    let mut workspace_peak_bytes = 0_u64;
    let mut destination_total_bytes = 0_u64;
    for size in sizes {
        let margin = SPACE_MARGIN_MINIMUM.max(size / 10);
        let workspace = size
            .checked_mul(2)
            .and_then(|value| value.checked_add(margin))
            .ok_or_else(size_overflow)?;
        let destination = size.checked_add(margin).ok_or_else(size_overflow)?;
        workspace_peak_bytes = workspace_peak_bytes.max(workspace);
        destination_total_bytes = destination_total_bytes
            .checked_add(destination)
            .ok_or_else(size_overflow)?;
    }
    let combined_required_bytes = workspace_peak_bytes
        .checked_add(destination_total_bytes)
        .ok_or_else(size_overflow)?;
    Ok(BatchDiskEstimate {
        workspace_peak_bytes,
        destination_total_bytes,
        combined_required_bytes,
        workspace_and_destination_share_volume: shared_volume,
    })
}

fn validate_batch_request(request: &BatchPreviewRequest) -> Result<(), OperationError> {
    if request.schema_version != BATCH_PREVIEW_SCHEMA_VERSION {
        return Err(invalid_request());
    }
    validate_batch_eligibility(&request.operation_id, &request.operation_version)?;
    if request.input_paths.is_empty() || request.input_paths.len() > BATCH_MAX_INPUTS {
        return Err(invalid_request());
    }
    if request.naming_template.is_empty()
        || request.naming_template.len() > BATCH_NAMING_TEMPLATE_MAX_BYTES
    {
        return Err(invalid_template());
    }
    parse_naming_template(&request.naming_template)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TemplatePart<'a> {
    Literal(&'a str),
    Stem,
    Index,
    EscapedOpen,
    EscapedClose,
}

fn parse_naming_template(template: &str) -> Result<Vec<TemplatePart<'_>>, OperationError> {
    let mut parts = Vec::new();
    let mut cursor = 0;
    let mut stem_count = 0_u8;
    let mut index_count = 0_u8;
    while cursor < template.len() {
        let remaining = &template[cursor..];
        if remaining.starts_with("{{") {
            parts.push(TemplatePart::EscapedOpen);
            cursor += 2;
            continue;
        }
        if remaining.starts_with("}}") {
            parts.push(TemplatePart::EscapedClose);
            cursor += 2;
            continue;
        }
        if remaining.starts_with("{stem}") {
            stem_count = stem_count.saturating_add(1);
            parts.push(TemplatePart::Stem);
            cursor += "{stem}".len();
            continue;
        }
        if remaining.starts_with("{index}") {
            index_count = index_count.saturating_add(1);
            parts.push(TemplatePart::Index);
            cursor += "{index}".len();
            continue;
        }
        if remaining.starts_with('{') || remaining.starts_with('}') {
            return Err(invalid_template());
        }
        let literal_length = remaining.find(['{', '}']).unwrap_or(remaining.len());
        if literal_length == 0 {
            return Err(invalid_template());
        }
        parts.push(TemplatePart::Literal(&remaining[..literal_length]));
        cursor += literal_length;
    }
    if stem_count != 1 || index_count > 1 {
        return Err(invalid_template());
    }
    Ok(parts)
}

fn render_requested_names(
    template: &str,
    sources: &[SourceSnapshot],
) -> Result<Vec<String>, OperationError> {
    let parts = parse_naming_template(template)?;
    let mut rendered: Vec<String> = Vec::with_capacity(sources.len());
    for (index, source) in sources.iter().enumerate() {
        let stem = source
            .canonical_path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(invalid_name)?;
        let mut name = String::new();
        for part in &parts {
            match part {
                TemplatePart::Literal(value) => name.push_str(value),
                TemplatePart::Stem => name.push_str(stem),
                TemplatePart::Index => {
                    use std::fmt::Write as _;
                    write!(&mut name, "{:03}", index + 1).map_err(|_| invalid_name())?;
                }
                TemplatePart::EscapedOpen => name.push('{'),
                TemplatePart::EscapedClose => name.push('}'),
            }
        }
        validate_pdf_output_name(&name)?;
        if rendered
            .iter()
            .any(|existing| windows_file_names_equal(existing, &name))
        {
            return Err(duplicate_name());
        }
        rendered.push(name);
    }
    Ok(rendered)
}

fn validate_pdf_output_name(name: &str) -> Result<(), OperationError> {
    validate_output_name(name).map_err(|_| invalid_name())?;
    if !name
        .get(name.len().saturating_sub(4)..)
        .is_some_and(|extension| extension.eq_ignore_ascii_case(".pdf"))
    {
        return Err(invalid_name());
    }
    Ok(())
}

fn inspect_source(path: &str, index: usize) -> Result<SourceSnapshot, OperationError> {
    let original = Path::new(path);
    if !original
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
    {
        return Err(source_not_pdf(index));
    }
    let (canonical_path, expected_identity) =
        canonical_regular_file(original).map_err(|_| source_path_error(index))?;
    let mut guard = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_SEQUENTIAL_SCAN | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&canonical_path)
        .map_err(|_| source_busy(index))?;
    let identity = identity_from_file(&guard).map_err(|_| source_path_error(index))?;
    if identity != expected_identity {
        return Err(source_changed(index));
    }
    let before = guard.metadata().map_err(|_| source_path_error(index))?;
    let modified_at = modified_timestamp(&before).map_err(|_| source_path_error(index))?;
    let display_name = canonical_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| source_path_error(index))?
        .to_owned();
    let canonical_path_sha256 = sha256_hex(
        canonical_path
            .to_str()
            .ok_or_else(|| source_path_error(index))?
            .as_bytes(),
    );

    guard
        .seek(SeekFrom::Start(0))
        .map_err(|_| source_path_error(index))?;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut header = Vec::with_capacity(1024);
    let mut buffer = vec![0_u8; HASH_BUFFER_BYTES];
    loop {
        let read = guard
            .read(&mut buffer)
            .map_err(|_| source_path_error(index))?;
        if read == 0 {
            break;
        }
        if header.len() < 1024 {
            let remaining = 1024 - header.len();
            header.extend_from_slice(&buffer[..read.min(remaining)]);
        }
        hasher.update(&buffer[..read]);
        total = total.checked_add(read as u64).ok_or_else(size_overflow)?;
    }
    if !header.windows(5).any(|window| window == b"%PDF-") {
        return Err(source_not_pdf(index));
    }
    let after = guard.metadata().map_err(|_| source_path_error(index))?;
    if total != before.len()
        || after.len() != before.len()
        || modified_timestamp(&after).map_err(|_| source_path_error(index))? != modified_at
        || identity_from_file(&guard).map_err(|_| source_path_error(index))? != identity
    {
        return Err(source_changed(index));
    }
    Ok(SourceSnapshot {
        original_path: path.to_owned(),
        canonical_path,
        fingerprint: CanonicalSourceFingerprint {
            ordinal: u32::try_from(index).map_err(|_| invalid_request())?,
            display_name,
            canonical_path_sha256,
            file_identity: identity.to_string(),
            size_bytes: total,
            modified_at,
            sha256: digest_hex(hasher.finalize().as_slice()),
        },
        _guard: guard,
    })
}

fn plan_collisions(
    destination: &Path,
    requested_names: &[String],
) -> Result<Vec<CollisionPlanEntry>, OperationError> {
    let existing_names = destination_entry_names(destination)?;
    plan_collisions_against_names(requested_names, &existing_names)
}

fn destination_entry_names(destination: &Path) -> Result<Vec<String>, OperationError> {
    let entries = fs::read_dir(destination).map_err(|_| path_error())?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| path_error())?;
        names.push(entry.file_name().into_string().map_err(|_| path_error())?);
    }
    Ok(names)
}

fn plan_collisions_against_names(
    requested_names: &[String],
    existing_names: &[String],
) -> Result<Vec<CollisionPlanEntry>, OperationError> {
    let mut reserved: Vec<String> = Vec::with_capacity(requested_names.len());
    let mut plan = Vec::with_capacity(requested_names.len());
    for (index, requested_name) in requested_names.iter().enumerate() {
        let mut selected = None;
        for collision_index in 0..MAX_COLLISION_ATTEMPTS {
            let candidate = collision_name(requested_name, collision_index);
            if validate_output_name(&candidate).is_err() {
                continue;
            }
            if reserved
                .iter()
                .any(|name| windows_file_names_equal(name, &candidate))
                || existing_names
                    .iter()
                    .any(|name| windows_file_names_equal(name, &candidate))
            {
                continue;
            }
            reserved.push(candidate.clone());
            selected = Some((candidate, collision_index));
            break;
        }
        let (planned_name, collision_index) = selected.ok_or_else(collision_exhausted)?;
        plan.push(CollisionPlanEntry {
            ordinal: u32::try_from(index).map_err(|_| invalid_request())?,
            requested_name: requested_name.clone(),
            planned_name,
            collision_index,
        });
    }
    Ok(plan)
}

fn modified_timestamp(metadata: &fs::Metadata) -> Result<String, std::io::Error> {
    let modified: DateTime<Utc> = metadata.modified()?.into();
    Ok(modified.to_rfc3339_opts(SecondsFormat::Nanos, true))
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn sha256_hex(bytes: &[u8]) -> String {
    digest_hex(Sha256::digest(bytes).as_slice())
}

fn digest_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn error(
    code: &str,
    title: &str,
    detail: &str,
    stage: OperationStage,
    retryable: bool,
) -> OperationError {
    OperationError::safe(code, title, detail, stage, retryable)
}

fn indexed_error(mut value: OperationError, index: usize) -> OperationError {
    value.input_index = Some(index);
    value
}

fn invalid_request() -> OperationError {
    error(
        "BATCH_REQUEST_INVALID",
        "The batch preview request is not valid",
        "Choose 1–128 local PDFs and the supported contract version.",
        OperationStage::Inspect,
        false,
    )
}

fn invalid_template() -> OperationError {
    error(
        "BATCH_NAMING_TEMPLATE_INVALID",
        "The batch naming template is not valid",
        "Use {stem}, optionally {index}, and double braces for literal braces.",
        OperationStage::Plan,
        false,
    )
}

fn duplicate_name() -> OperationError {
    error(
        "BATCH_OUTPUT_NAME_DUPLICATE",
        "The batch naming template creates duplicate names",
        "Add {index} or choose a template that creates a distinct PDF name for every source.",
        OperationStage::Plan,
        false,
    )
}

fn invalid_name() -> OperationError {
    error(
        "BATCH_OUTPUT_NAME_INVALID",
        "A requested output name is not safe",
        "Use a safe local Windows file name ending in .pdf.",
        OperationStage::Plan,
        false,
    )
}

fn stale_preview() -> OperationError {
    error(
        "BATCH_PLAN_STALE",
        "The batch preview is stale",
        "A source, destination, requested name, collision, or estimate changed. Preview the batch again.",
        OperationStage::Preflight,
        true,
    )
}

fn dependency_error() -> OperationError {
    error(
        "BATCH_DEPENDENCY_UNAVAILABLE",
        "The local PDF engine is unavailable",
        "Repair the installed qpdf engine before previewing this batch.",
        OperationStage::Preflight,
        false,
    )
}

fn destination_permission_error() -> OperationError {
    error(
        "BATCH_DESTINATION_PERMISSION_DENIED",
        "The destination cannot accept new files",
        "Choose an existing local destination where Document Studio may add a file.",
        OperationStage::Preflight,
        false,
    )
}

fn path_error() -> OperationError {
    error(
        "BATCH_PATH_UNSAFE",
        "A selected local path is not safe",
        "Choose existing regular local PDFs and an existing local destination without reparse points.",
        OperationStage::Inspect,
        false,
    )
}

fn source_path_error(index: usize) -> OperationError {
    indexed_error(path_error(), index)
}

fn source_busy(index: usize) -> OperationError {
    indexed_error(
        error(
            "BATCH_SOURCE_BUSY",
            "A source PDF is busy",
            "Close the program changing the PDF, then preview the batch again.",
            OperationStage::Inspect,
            true,
        ),
        index,
    )
}

fn source_not_pdf(index: usize) -> OperationError {
    indexed_error(
        error(
            "BATCH_SOURCE_NOT_PDF",
            "A selected file is not a PDF",
            "Choose a local .pdf file with a valid PDF header.",
            OperationStage::Inspect,
            false,
        ),
        index,
    )
}

fn source_changed(index: usize) -> OperationError {
    indexed_error(
        error(
            "BATCH_SOURCE_CHANGED",
            "A source PDF changed during inspection",
            "Wait for the file to stop changing, then preview the batch again.",
            OperationStage::Inspect,
            true,
        ),
        index,
    )
}

fn input_alias_error(index: usize) -> OperationError {
    indexed_error(
        error(
            "BATCH_DESTINATION_INPUT_ALIAS",
            "A requested output aliases a source PDF",
            "Choose another destination or requested output name.",
            OperationStage::Preflight,
            false,
        ),
        index,
    )
}

fn collision_exhausted() -> OperationError {
    error(
        "BATCH_COLLISION_EXHAUSTED",
        "No safe collision-free preview name is available",
        "Choose different requested names or another destination.",
        OperationStage::Plan,
        false,
    )
}

fn size_overflow() -> OperationError {
    error(
        "BATCH_ESTIMATE_OVERFLOW",
        "The batch disk estimate is too large",
        "Reduce the number or size of the selected PDFs.",
        OperationStage::Estimate,
        false,
    )
}

fn preview_too_large() -> OperationError {
    error(
        "BATCH_PREVIEW_TOO_LARGE",
        "The batch preview metadata is too large",
        "Use fewer or shorter requested file names.",
        OperationStage::Plan,
        false,
    )
}

fn disk_check_error() -> OperationError {
    error(
        "BATCH_DISK_CHECK_FAILED",
        "Available disk space could not be checked",
        "Check local disk access, then preview the batch again.",
        OperationStage::Preflight,
        true,
    )
}

fn insufficient_space() -> OperationError {
    error(
        "BATCH_INSUFFICIENT_SPACE",
        "There is not enough available disk space",
        "Free space in the workspace or destination, then create a fresh preview.",
        OperationStage::Preflight,
        true,
    )
}

fn metadata_error() -> OperationError {
    error(
        "BATCH_METADATA_WRITE_FAILED",
        "Batch metadata could not be saved",
        "No batch was created. Retry after local metadata storage is available.",
        OperationStage::Audit,
        true,
    )
}

fn batch_not_found() -> OperationError {
    error(
        "BATCH_NOT_FOUND",
        "The batch metadata was not found",
        "Refresh batch history and try again.",
        OperationStage::Audit,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_preview_bytes, estimate_disk_sizes, plan_collisions_against_names,
        require_preview_proof, validate_pdf_output_name, BatchPreviewHooks, BatchPreviewService,
        CanonicalDestinationFingerprint, CanonicalPreviewEnvelope, CanonicalSourceFingerprint,
        CollisionPlanEntry,
    };
    use crate::app_state::AppState;
    use crate::contracts::{
        BatchCreateRequest, BatchPreviewRequest, BatchSettings, JobCompletionKind,
    };
    use crate::database::Database;
    use crate::qpdf::QpdfRuntimeManager;
    use crate::workspace::WorkspaceManager;
    use std::fs;
    use std::path::PathBuf;

    struct Fixture {
        _directory: tempfile::TempDir,
        state: AppState,
        inputs: Vec<PathBuf>,
        destination: PathBuf,
    }

    fn fixture(count: usize) -> Fixture {
        let directory = tempfile::tempdir().unwrap();
        let app_data = directory.path().join("app-data");
        let input_directory = directory.path().join("inputs");
        let destination = directory.path().join("destination");
        fs::create_dir_all(&input_directory).unwrap();
        fs::create_dir_all(&destination).unwrap();
        let inputs = (0..count)
            .map(|index| {
                let path = input_directory.join(format!("source-{index:03}.pdf"));
                fs::write(
                    &path,
                    format!("%PDF-1.7\n% private generated fixture {index}\n%%EOF\n"),
                )
                .unwrap();
                path
            })
            .collect();
        let workspaces = WorkspaceManager::initialize(&app_data).unwrap();
        let qpdf = QpdfRuntimeManager::new(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/qpdf/12.3.2"),
            app_data.join("engines"),
        );
        let state = AppState::new(Database::open_in_memory().unwrap(), workspaces).with_qpdf(qpdf);
        Fixture {
            _directory: directory,
            state,
            inputs,
            destination,
        }
    }

    fn request(fixture: &Fixture) -> BatchPreviewRequest {
        BatchPreviewRequest {
            schema_version: 1,
            operation_id: "pdf.compress-lossless".to_owned(),
            operation_version: "1.0.0".to_owned(),
            settings: BatchSettings {},
            input_paths: fixture
                .inputs
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            destination_directory: fixture.destination.to_string_lossy().into_owned(),
            naming_template: "{stem}-compressed.pdf".to_owned(),
        }
    }

    fn create_request(
        request: &BatchPreviewRequest,
        hash: &str,
        optimistic_version: u64,
    ) -> BatchCreateRequest {
        BatchCreateRequest {
            schema_version: request.schema_version,
            operation_id: request.operation_id.clone(),
            operation_version: request.operation_version.clone(),
            settings: request.settings.clone(),
            input_paths: request.input_paths.clone(),
            destination_directory: request.destination_directory.clone(),
            naming_template: request.naming_template.clone(),
            preview_sha256: hash.to_owned(),
            optimistic_version,
        }
    }

    fn exact_create_request(
        request: &BatchPreviewRequest,
        preview: &crate::contracts::BatchPreviewResponse,
    ) -> BatchCreateRequest {
        create_request(request, &preview.preview_sha256, preview.optimistic_version)
    }

    fn count(state: &AppState, table: &str) -> i64 {
        let sql = format!("SELECT COUNT(*) FROM {table}");
        state
            .database()
            .connection()
            .query_row(&sql, [], |row| row.get(0))
            .unwrap()
    }

    fn assert_single_batch_child_graph(state: &AppState) {
        assert_eq!(count(state, "batch_runs"), 1);
        assert_eq!(count(state, "batch_run_jobs"), 1);
        assert_eq!(count(state, "jobs"), 1);
        assert_eq!(count(state, "job_inputs"), 1);
        assert_eq!(count(state, "job_outputs"), 1);
        assert_eq!(count(state, "job_operation_specs"), 1);
    }

    #[test]
    fn preview_is_deterministic_ordered_path_free_and_collision_aware() {
        let fixture = fixture(2);
        fs::create_dir(fixture.destination.join("source-000-compressed.pdf")).unwrap();
        fs::write(
            fixture.destination.join("source-001-compressed.pdf"),
            b"existing user file",
        )
        .unwrap();
        let service = BatchPreviewService::new(fixture.state.clone());
        let qpdf_cache_parent = fixture._directory.path().join("app-data/engines");
        assert!(!qpdf_cache_parent.exists());
        let workspace_entries_before = fs::read_dir(fixture.state.workspaces.root())
            .unwrap()
            .count();
        let destination_entries_before = fs::read_dir(&fixture.destination).unwrap().count();
        let first = service.preview(request(&fixture)).unwrap();
        let second = service.preview(request(&fixture)).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.rows[0].ordinal, 0);
        assert_eq!(first.rows[0].collision_index, 1);
        assert_eq!(first.rows[0].output_name, "source-000-compressed (1).pdf");
        assert_eq!(first.rows[1].ordinal, 1);
        assert_eq!(first.rows[1].collision_index, 1);
        assert_eq!(first.rows[1].output_name, "source-001-compressed (1).pdf");
        assert_eq!(
            first.settings_sha256,
            "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
        );
        let response_json = serde_json::to_string(&first).unwrap();
        for path in fixture.inputs.iter().chain([&fixture.destination]) {
            assert!(!response_json.contains(&path.to_string_lossy().into_owned()));
        }
        assert!(!response_json.contains("sourcePath"));
        assert!(!response_json.contains("destinationDirectory"));
        assert!(!response_json.contains("fileIdentity"));
        assert!(!response_json.contains("sha256\":\"%PDF"));
        for table in [
            "batch_runs",
            "batch_run_jobs",
            "jobs",
            "job_inputs",
            "job_outputs",
            "job_operation_specs",
        ] {
            assert_eq!(count(&fixture.state, table), 0, "preview wrote {table}");
        }
        assert_eq!(
            fs::read_dir(fixture.state.workspaces.root())
                .unwrap()
                .count(),
            workspace_entries_before
        );
        assert_eq!(
            fs::read_dir(&fixture.destination).unwrap().count(),
            destination_entries_before
        );
        assert!(
            !qpdf_cache_parent.exists(),
            "preview must not materialize the qpdf execution cache"
        );
    }

    #[test]
    fn preview_and_safe_errors_omit_paths_content_and_private_canonical_fingerprints() {
        let fixture = fixture(1);
        let sentinel = "%PDF-1.7\nDOCUMENT-CONTENT-SENTINEL-9F41\n%%EOF\n";
        fs::write(&fixture.inputs[0], sentinel).unwrap();
        let service = BatchPreviewService::new(fixture.state.clone());
        let request = request(&fixture);
        let response_json =
            serde_json::to_string(&service.preview(request.clone()).unwrap()).unwrap();
        let mut invalid = request;
        invalid.naming_template = "{unknown}.pdf".to_owned();
        let error_json = serde_json::to_string(&service.preview(invalid).unwrap_err()).unwrap();
        let parent_path = fixture._directory.path().to_string_lossy().into_owned();
        let source_path = fixture.inputs[0].to_string_lossy().into_owned();
        let destination_path = fixture.destination.to_string_lossy().into_owned();
        for serialized in [&response_json, &error_json] {
            for private in [
                parent_path.as_str(),
                source_path.as_str(),
                destination_path.as_str(),
                "DOCUMENT-CONTENT-SENTINEL-9F41",
                "sourcePath",
                "canonicalPath",
                "destinationDirectory",
                "fileIdentity",
                "modifiedAt",
                "collisionPlan",
                "sources",
            ] {
                assert!(!serialized.contains(private), "leaked {private}");
            }
        }
    }

    #[test]
    fn order_changes_hash_and_template_uses_padded_index_and_escapes() {
        let fixture = fixture(2);
        let service = BatchPreviewService::new(fixture.state.clone());
        let first_request = request(&fixture);
        let first = service.preview(first_request.clone()).unwrap();
        let mut reversed_request = first_request.clone();
        reversed_request.input_paths.reverse();
        let reversed = service.preview(reversed_request).unwrap();
        assert_ne!(first.preview_sha256, reversed.preview_sha256);

        let mut escaped_request = first_request;
        escaped_request.naming_template = "{{{index}}}-{stem}.pdf".to_owned();
        let escaped = service.preview(escaped_request).unwrap();
        assert_eq!(escaped.rows[0].output_name, "{001}-source-000.pdf");
        assert_eq!(escaped.rows[1].output_name, "{002}-source-001.pdf");
    }

    #[test]
    fn every_source_fingerprint_dimension_independently_invalidates_the_exact_proof() {
        let source = CanonicalSourceFingerprint {
            ordinal: 0,
            display_name: "source.pdf".to_owned(),
            canonical_path_sha256: "c".repeat(64),
            file_identity: "volume-1:file-1".to_owned(),
            size_bytes: 100,
            modified_at: "2026-08-26T08:00:00.000000000Z".to_owned(),
            sha256: "a".repeat(64),
        };
        let envelope = CanonicalPreviewEnvelope {
            schema_version: 1,
            operation_id: "pdf.compress-lossless".to_owned(),
            operation_version: "1.0.0".to_owned(),
            settings_sha256: "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
                .to_owned(),
            sources: vec![source],
            destination: CanonicalDestinationFingerprint {
                file_identity: "volume-1:file-2".to_owned(),
            },
            naming_template: "{stem}-compressed.pdf".to_owned(),
            collision_plan: vec![CollisionPlanEntry {
                ordinal: 0,
                requested_name: "source-compressed.pdf".to_owned(),
                planned_name: "source-compressed.pdf".to_owned(),
                collision_index: 0,
            }],
            disk_estimate: estimate_disk_sizes([100], true).unwrap(),
            optimistic_version: 0,
        };
        let baseline_hash = super::sha256_hex(&canonical_preview_bytes(&envelope).unwrap());
        for mutation in 0..5 {
            let mut changed = envelope.clone();
            match mutation {
                0 => changed.sources[0].canonical_path_sha256 = "d".repeat(64),
                1 => changed.sources[0].file_identity = "volume-1:file-3".to_owned(),
                2 => changed.sources[0].size_bytes += 1,
                3 => {
                    changed.sources[0].modified_at = "2026-08-26T08:00:00.000000001Z".to_owned();
                }
                4 => changed.sources[0].sha256 = "b".repeat(64),
                _ => unreachable!(),
            }
            let changed_hash = super::sha256_hex(&canonical_preview_bytes(&changed).unwrap());
            assert_ne!(changed_hash, baseline_hash);
            let response = crate::contracts::BatchPreviewResponse {
                schema_version: 1,
                operation_id: changed.operation_id.clone(),
                operation_version: changed.operation_version.clone(),
                settings_sha256: changed.settings_sha256.clone(),
                naming_template: changed.naming_template.clone(),
                rows: vec![],
                disk_estimate: changed.disk_estimate.clone(),
                preview_sha256: changed_hash,
                canonical_size_bytes: 1,
                optimistic_version: 0,
            };
            assert_eq!(
                require_preview_proof(&baseline_hash, 0, &response)
                    .unwrap_err()
                    .code,
                "BATCH_PLAN_STALE"
            );
        }
    }

    #[test]
    fn collision_planning_treats_every_existing_entry_name_as_ordinal_insensitive() {
        let requested = vec!["Report-compressed.pdf".to_owned()];
        let existing = vec!["rEpOrT-CoMpReSsEd.PDF".to_owned()];
        let plan = plan_collisions_against_names(&requested, &existing).unwrap();
        assert_eq!(plan[0].collision_index, 1);
        assert_eq!(plan[0].planned_name, "Report-compressed (1).pdf");
    }

    #[test]
    fn naming_grammar_reserved_names_duplicates_and_utf16_limits_fail_closed() {
        let base_fixture = fixture(1);
        let service = BatchPreviewService::new(base_fixture.state.clone());
        for template in [
            "fixed.pdf",
            "{stem}-{stem}.pdf",
            "{stem}-{index}-{index}.pdf",
            "{unknown}.pdf",
            "{stem.pdf",
            "{stem}.txt",
            "../{stem}.pdf",
            "{stem}:stream.pdf",
        ] {
            let mut candidate = request(&base_fixture);
            candidate.naming_template = template.to_owned();
            assert!(matches!(
                service.preview(candidate).unwrap_err().code.as_str(),
                "BATCH_NAMING_TEMPLATE_INVALID" | "BATCH_OUTPUT_NAME_INVALID"
            ));
        }
        assert!(validate_pdf_output_name(&format!("{}.pdf", "a".repeat(251))).is_ok());
        assert!(validate_pdf_output_name(&format!("{}.pdf", "a".repeat(252))).is_err());
        assert!(validate_pdf_output_name("CON.pdf").is_err());
        assert!(validate_pdf_output_name("lpt9.PDF").is_err());

        let duplicate_fixture = fixture(1);
        let duplicated_path = duplicate_fixture.inputs[0].to_string_lossy().into_owned();
        let mut duplicate_request = request(&duplicate_fixture);
        duplicate_request.input_paths = vec![duplicated_path.clone(), duplicated_path];
        assert_eq!(
            BatchPreviewService::new(duplicate_fixture.state)
                .preview(duplicate_request)
                .unwrap_err()
                .code,
            "BATCH_OUTPUT_NAME_DUPLICATE"
        );
    }

    #[test]
    fn unicode_stems_render_literally_and_distinct_case_variants_collide_on_windows() {
        let unicode_fixture = fixture(1);
        let unicode_path = unicode_fixture.inputs[0].with_file_name("नमस्ते-తెలుగు-😀-e\u{301}.pdf");
        fs::rename(&unicode_fixture.inputs[0], &unicode_path).unwrap();
        let mut unicode_request = request(&unicode_fixture);
        unicode_request.input_paths = vec![unicode_path.to_string_lossy().into_owned()];
        assert_eq!(
            BatchPreviewService::new(unicode_fixture.state)
                .preview(unicode_request)
                .unwrap()
                .rows[0]
                .output_name,
            "नमस्ते-తెలుగు-😀-e\u{301}-compressed.pdf"
        );

        let case_fixture = fixture(1);
        let first_directory = case_fixture._directory.path().join("case-one");
        let second_directory = case_fixture._directory.path().join("case-two");
        fs::create_dir_all(&first_directory).unwrap();
        fs::create_dir_all(&second_directory).unwrap();
        let upper = first_directory.join("Report.pdf");
        let lower = second_directory.join("report.pdf");
        fs::write(&upper, b"%PDF-1.7\nupper\n%%EOF\n").unwrap();
        fs::write(&lower, b"%PDF-1.7\nlower\n%%EOF\n").unwrap();
        let mut case_request = request(&case_fixture);
        case_request.input_paths = vec![
            upper.to_string_lossy().into_owned(),
            lower.to_string_lossy().into_owned(),
        ];
        assert_eq!(
            BatchPreviewService::new(case_fixture.state)
                .preview(case_request)
                .unwrap_err()
                .code,
            "BATCH_OUTPUT_NAME_DUPLICATE"
        );
    }

    #[test]
    fn exact_hash_creates_queued_metadata_only_without_files_or_workers() {
        let fixture = fixture(2);
        let service = BatchPreviewService::new(fixture.state.clone());
        let request = request(&fixture);
        let preview = service.preview(request.clone()).unwrap();
        let destination_before = fs::read_dir(&fixture.destination).unwrap().count();
        let batch = service
            .create(exact_create_request(&request, &preview))
            .unwrap();
        assert_eq!(batch.progress.settled_children, 0);
        assert_eq!(batch.progress.total_children, 2);
        assert!(batch
            .children
            .iter()
            .all(|child| child.state.as_str() == "queued"));
        assert_eq!(count(&fixture.state, "batch_runs"), 1);
        assert_eq!(count(&fixture.state, "batch_run_jobs"), 2);
        assert_eq!(count(&fixture.state, "jobs"), 2);
        assert_eq!(count(&fixture.state, "job_inputs"), 2);
        assert_eq!(count(&fixture.state, "job_outputs"), 2);
        assert_eq!(count(&fixture.state, "job_operation_specs"), 2);
        assert!(fixture
            .state
            .database()
            .startup_recovery_jobs()
            .unwrap()
            .is_empty());
        for child in &batch.children {
            assert!(!fixture.state.workspaces.root().join(&child.job_id).exists());
        }
        assert_eq!(
            fs::read_dir(&fixture.destination).unwrap().count(),
            destination_before
        );
    }

    #[test]
    fn explicit_history_delete_preserves_batch_linked_job_and_relationships() {
        let fixture = fixture(1);
        let service = BatchPreviewService::new(fixture.state.clone());
        let request = request(&fixture);
        let preview = service.preview(request.clone()).unwrap();
        let batch = service
            .create(exact_create_request(&request, &preview))
            .unwrap();
        let job_id = batch.children[0].job_id.clone();
        {
            let mut database = fixture.state.database();
            database
                .connection()
                .execute(
                    "UPDATE jobs SET state = 'cancelled', stage = NULL,
                            updated_at = '2026-01-01T00:00:00Z',
                            finished_at = '2026-01-01T00:00:00Z'
                     WHERE id = ?1",
                    [&job_id],
                )
                .unwrap();
            assert_eq!(
                database
                    .delete_terminal_history(std::slice::from_ref(&job_id))
                    .unwrap(),
                0
            );
        }
        assert_single_batch_child_graph(&fixture.state);
        let loaded = service.get(&batch.id).unwrap();
        assert_eq!(loaded.children[0].job_id, job_id);
        assert_eq!(loaded.children[0].state.as_str(), "cancelled");
    }

    #[test]
    fn retention_preserves_batch_linked_job_and_relationships() {
        let fixture = fixture(1);
        let service = BatchPreviewService::new(fixture.state.clone());
        let request = request(&fixture);
        let preview = service.preview(request.clone()).unwrap();
        let batch = service
            .create(exact_create_request(&request, &preview))
            .unwrap();
        let job_id = batch.children[0].job_id.clone();
        {
            let mut database = fixture.state.database();
            database
                .connection()
                .execute(
                    "UPDATE jobs SET state = 'cancelled', stage = NULL,
                            updated_at = '2026-01-01T00:00:00Z',
                            finished_at = '2026-01-01T00:00:00Z'
                     WHERE id = ?1",
                    [&job_id],
                )
                .unwrap();
            assert_eq!(
                database
                    .purge_terminal_before("2026-02-01T00:00:00Z")
                    .unwrap(),
                0
            );
        }
        assert_single_batch_child_graph(&fixture.state);
        let loaded = service.get(&batch.id).unwrap();
        assert_eq!(loaded.children[0].job_id, job_id);
        assert_eq!(loaded.children[0].state.as_str(), "cancelled");
    }

    #[test]
    fn two_exact_same_plan_previews_allow_only_one_live_batch() {
        let fixture = fixture(2);
        let service = BatchPreviewService::new(fixture.state.clone());
        let request = request(&fixture);
        let first_preview = service.preview(request.clone()).unwrap();
        let second_preview = service.preview(request.clone()).unwrap();
        assert_eq!(first_preview.optimistic_version, 0);
        assert_eq!(second_preview.optimistic_version, 0);
        assert_eq!(first_preview.preview_sha256, second_preview.preview_sha256);

        service
            .create(exact_create_request(&request, &first_preview))
            .unwrap();
        assert_eq!(
            service
                .create(exact_create_request(&request, &second_preview))
                .unwrap_err()
                .code,
            "BATCH_PLAN_STALE"
        );
        assert_eq!(count(&fixture.state, "batch_runs"), 1);
        assert_eq!(count(&fixture.state, "batch_run_jobs"), 2);
        assert_eq!(count(&fixture.state, "jobs"), 2);
        assert_eq!(count(&fixture.state, "job_inputs"), 2);
        assert_eq!(count(&fixture.state, "job_outputs"), 2);
        assert_eq!(count(&fixture.state, "job_operation_specs"), 2);
    }

    #[test]
    fn restart_reopens_batch_with_exact_ordinals_relationships_and_no_worker_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let app_data = directory.path().join("app-data");
        let input_directory = directory.path().join("inputs");
        let destination = directory.path().join("destination");
        let database_path = app_data.join("metadata.sqlite3");
        fs::create_dir_all(&input_directory).unwrap();
        fs::create_dir_all(&destination).unwrap();
        let inputs = (0..3)
            .map(|index| {
                let path = input_directory.join(format!("restart-{index}.pdf"));
                fs::write(&path, format!("%PDF-1.7\nrestart-{index}\n%%EOF\n")).unwrap();
                path
            })
            .collect::<Vec<_>>();
        let qpdf = QpdfRuntimeManager::new(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/qpdf/12.3.2"),
            app_data.join("engines"),
        );
        let workspaces = WorkspaceManager::initialize(&app_data).unwrap();
        let state = AppState::new(Database::open(&database_path).unwrap(), workspaces)
            .with_qpdf(qpdf.clone());
        let request = BatchPreviewRequest {
            schema_version: 1,
            operation_id: "pdf.compress-lossless".to_owned(),
            operation_version: "1.0.0".to_owned(),
            settings: BatchSettings {},
            input_paths: inputs
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            destination_directory: destination.to_string_lossy().into_owned(),
            naming_template: "{index}-{stem}-compressed.pdf".to_owned(),
        };
        let service = BatchPreviewService::new(state.clone());
        let preview = service.preview(request.clone()).unwrap();
        let created = service
            .create(exact_create_request(&request, &preview))
            .unwrap();
        let batch_id = created.id.clone();
        let expected_jobs = created
            .children
            .iter()
            .map(|child| child.job_id.clone())
            .collect::<Vec<_>>();
        drop(service);
        drop(state);

        let reopened = AppState::new(
            Database::open(&database_path).unwrap(),
            WorkspaceManager::initialize(&app_data).unwrap(),
        )
        .with_qpdf(qpdf);
        let loaded = BatchPreviewService::new(reopened.clone())
            .get(&batch_id)
            .unwrap();
        assert_eq!(loaded.children.len(), 3);
        for (ordinal, child) in loaded.children.iter().enumerate() {
            assert_eq!(child.ordinal, ordinal as u32);
            assert_eq!(child.job_id, expected_jobs[ordinal]);
            let relationship_count: i64 = reopened
                .database()
                .connection()
                .query_row(
                    "SELECT COUNT(*) FROM batch_run_jobs
                     WHERE batch_id = ?1 AND ordinal = ?2 AND job_id = ?3",
                    rusqlite::params![&batch_id, ordinal as u32, &child.job_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(relationship_count, 1);
        }
        assert!(reopened
            .database()
            .startup_recovery_jobs()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn source_destination_or_collision_staleness_creates_nothing() {
        let source_fixture = fixture(1);
        let source_service = BatchPreviewService::new(source_fixture.state.clone());
        let source_request = request(&source_fixture);
        let source_preview = source_service.preview(source_request.clone()).unwrap();
        fs::write(&source_fixture.inputs[0], b"%PDF-1.7\nchanged\n%%EOF\n").unwrap();
        let error = source_service
            .create(exact_create_request(&source_request, &source_preview))
            .unwrap_err();
        assert_eq!(error.code, "BATCH_PLAN_STALE");
        assert_eq!(count(&source_fixture.state, "batch_runs"), 0);
        assert_eq!(count(&source_fixture.state, "jobs"), 0);

        let destination_fixture = fixture(1);
        let destination_service = BatchPreviewService::new(destination_fixture.state.clone());
        let destination_request = request(&destination_fixture);
        let destination_preview = destination_service
            .preview(destination_request.clone())
            .unwrap();
        let displaced = destination_fixture
            .destination
            .with_file_name("destination-displaced");
        fs::rename(&destination_fixture.destination, &displaced).unwrap();
        fs::create_dir(&destination_fixture.destination).unwrap();
        let error = destination_service
            .create(exact_create_request(
                &destination_request,
                &destination_preview,
            ))
            .unwrap_err();
        assert_eq!(error.code, "BATCH_PLAN_STALE");
        assert_eq!(count(&destination_fixture.state, "batch_runs"), 0);
        assert_eq!(count(&destination_fixture.state, "jobs"), 0);

        let collision_fixture = fixture(1);
        let collision_service = BatchPreviewService::new(collision_fixture.state.clone());
        let collision_request = request(&collision_fixture);
        let collision_preview = collision_service
            .preview(collision_request.clone())
            .unwrap();
        fs::write(
            collision_fixture
                .destination
                .join("source-000-compressed.pdf"),
            b"existing user file",
        )
        .unwrap();
        let error = collision_service
            .create(exact_create_request(&collision_request, &collision_preview))
            .unwrap_err();
        assert_eq!(error.code, "BATCH_PLAN_STALE");
        assert_eq!(count(&collision_fixture.state, "batch_runs"), 0);
        assert_eq!(count(&collision_fixture.state, "jobs"), 0);
    }

    #[test]
    fn same_name_hard_link_alias_replay_is_stale_despite_identical_file_fingerprint() {
        let fixture = fixture(1);
        let alias_directory = fixture._directory.path().join("alias-input");
        fs::create_dir(&alias_directory).unwrap();
        let alias = alias_directory.join(fixture.inputs[0].file_name().unwrap());
        fs::hard_link(&fixture.inputs[0], &alias).unwrap();

        let service = BatchPreviewService::new(fixture.state.clone());
        let preview_request = request(&fixture);
        let preview = service.preview(preview_request.clone()).unwrap();
        let mut replay = exact_create_request(&preview_request, &preview);
        replay.input_paths[0] = alias.to_string_lossy().into_owned();
        assert_eq!(service.create(replay).unwrap_err().code, "BATCH_PLAN_STALE");
        assert_eq!(count(&fixture.state, "batch_runs"), 0);
        assert_eq!(count(&fixture.state, "batch_run_jobs"), 0);
        assert_eq!(count(&fixture.state, "jobs"), 0);
    }

    #[test]
    fn exact_but_mismatched_preview_hash_creates_nothing() {
        let fixture = fixture(1);
        let service = BatchPreviewService::new(fixture.state.clone());
        let request = request(&fixture);
        let preview = service.preview(request.clone()).unwrap();
        let mismatched = if preview.preview_sha256.starts_with('0') {
            format!("1{}", &preview.preview_sha256[1..])
        } else {
            format!("0{}", &preview.preview_sha256[1..])
        };
        let error = service
            .create(create_request(
                &request,
                &mismatched,
                preview.optimistic_version,
            ))
            .unwrap_err();
        assert_eq!(error.code, "BATCH_PLAN_STALE");
        assert_eq!(count(&fixture.state, "batch_runs"), 0);
        assert_eq!(count(&fixture.state, "batch_run_jobs"), 0);
        assert_eq!(count(&fixture.state, "jobs"), 0);

        let malformed = service
            .create(create_request(
                &request,
                "not-a-preview-hash",
                preview.optimistic_version,
            ))
            .unwrap_err();
        assert_eq!(malformed.code, "BATCH_PLAN_STALE");
        assert_eq!(count(&fixture.state, "batch_runs"), 0);
        assert_eq!(count(&fixture.state, "jobs"), 0);
    }

    #[test]
    fn stale_contract_fields_cas_and_insufficient_disk_create_nothing() {
        let base_fixture = fixture(1);
        let service = BatchPreviewService::new(base_fixture.state.clone());
        let base_request = request(&base_fixture);
        let preview = service.preview(base_request.clone()).unwrap();
        let mut stale_version = exact_create_request(&base_request, &preview);
        stale_version.operation_version = "2.0.0".to_owned();
        assert_eq!(
            service.create(stale_version).unwrap_err().code,
            "BATCH_PLAN_STALE"
        );
        let mut stale_template = exact_create_request(&base_request, &preview);
        stale_template.naming_template = "{stem}-{index}.pdf".to_owned();
        assert_eq!(
            service.create(stale_template).unwrap_err().code,
            "BATCH_PLAN_STALE"
        );
        let mut stale_cas = exact_create_request(&base_request, &preview);
        stale_cas.optimistic_version += 1;
        assert_eq!(
            service.create(stale_cas).unwrap_err().code,
            "BATCH_PLAN_STALE"
        );
        assert_eq!(count(&base_fixture.state, "batch_runs"), 0);

        let disk_fixture = fixture(2);
        let disk_service = BatchPreviewService::new(disk_fixture.state.clone());
        let disk_request = request(&disk_fixture);
        let disk_preview = disk_service.preview(disk_request.clone()).unwrap();
        let error = disk_service
            .create_with_hooks(
                exact_create_request(&disk_request, &disk_preview),
                BatchPreviewHooks {
                    workspace_available_bytes: Some(0),
                    destination_available_bytes: Some(0),
                    ..BatchPreviewHooks::default()
                },
            )
            .unwrap_err();
        assert_eq!(error.code, "BATCH_INSUFFICIENT_SPACE");
        assert_eq!(count(&disk_fixture.state, "batch_runs"), 0);
        assert_eq!(count(&disk_fixture.state, "jobs"), 0);
    }

    #[test]
    fn disk_threshold_is_inclusive_and_estimate_overflow_fails_closed() {
        assert_eq!(
            estimate_disk_sizes([u64::MAX], true).unwrap_err().code,
            "BATCH_ESTIMATE_OVERFLOW"
        );

        let exact_fixture = fixture(1);
        let exact_service = BatchPreviewService::new(exact_fixture.state.clone());
        let exact_request = request(&exact_fixture);
        let exact_preview = exact_service.preview(exact_request.clone()).unwrap();
        exact_service
            .create_with_hooks(
                exact_create_request(&exact_request, &exact_preview),
                BatchPreviewHooks {
                    workspace_available_bytes: Some(
                        exact_preview.disk_estimate.workspace_peak_bytes,
                    ),
                    destination_available_bytes: Some(
                        exact_preview.disk_estimate.combined_required_bytes,
                    ),
                    ..BatchPreviewHooks::default()
                },
            )
            .unwrap();

        let short_fixture = fixture(1);
        let short_service = BatchPreviewService::new(short_fixture.state.clone());
        let short_request = request(&short_fixture);
        let short_preview = short_service.preview(short_request.clone()).unwrap();
        assert_eq!(
            short_service
                .create_with_hooks(
                    exact_create_request(&short_request, &short_preview),
                    BatchPreviewHooks {
                        workspace_available_bytes: Some(
                            short_preview.disk_estimate.workspace_peak_bytes,
                        ),
                        destination_available_bytes: Some(
                            short_preview.disk_estimate.combined_required_bytes - 1,
                        ),
                        ..BatchPreviewHooks::default()
                    },
                )
                .unwrap_err()
                .code,
            "BATCH_INSUFFICIENT_SPACE"
        );
    }

    #[test]
    fn every_insert_boundary_rolls_back_the_entire_batch() {
        for (table, condition) in [
            ("batch_runs", "1"),
            ("jobs", "1"),
            ("job_inputs", "1"),
            ("job_outputs", "1"),
            ("job_operation_specs", "1"),
            ("batch_run_jobs", "NEW.ordinal = 1"),
        ] {
            let fixture = fixture(2);
            fixture
                .state
                .database()
                .connection()
                .execute_batch(&format!(
                    "CREATE TRIGGER fail_batch_insert BEFORE INSERT ON {table} WHEN {condition}
                     BEGIN SELECT RAISE(ABORT, 'injected failure'); END;"
                ))
                .unwrap();
            let service = BatchPreviewService::new(fixture.state.clone());
            let request = request(&fixture);
            let preview = service.preview(request.clone()).unwrap();
            assert_eq!(
                service
                    .create(exact_create_request(&request, &preview))
                    .unwrap_err()
                    .code,
                "BATCH_METADATA_WRITE_FAILED"
            );
            for checked in [
                "batch_runs",
                "batch_run_jobs",
                "jobs",
                "job_inputs",
                "job_outputs",
                "job_operation_specs",
            ] {
                assert_eq!(count(&fixture.state, checked), 0, "{table} -> {checked}");
            }
        }
    }

    #[test]
    fn completed_no_benefit_is_settled_but_neither_failed_nor_published() {
        let fixture = fixture(2);
        let service = BatchPreviewService::new(fixture.state.clone());
        let request = request(&fixture);
        let preview = service.preview(request.clone()).unwrap();
        let batch = service
            .create(exact_create_request(&request, &preview))
            .unwrap();
        let child_id = &batch.children[0].job_id;
        fixture
            .state
            .database()
            .connection()
            .execute_batch(&format!(
                "DELETE FROM job_outputs WHERE job_id = '{child_id}';
                 UPDATE jobs SET state = 'completed', stage = NULL,
                   completed_units = total_units, finished_at = '2026-08-26T09:00:00Z',
                   updated_at = '2026-08-26T09:00:00Z' WHERE id = '{child_id}';
                 INSERT INTO job_completion_outcomes
                   (job_id, completion_kind, reason, created_at)
                 VALUES ('{child_id}', 'no-benefit', 'savings-threshold-not-met',
                   '2026-08-26T09:00:00Z');"
            ))
            .unwrap();
        let loaded = service.get(&batch.id).unwrap();
        assert_eq!(loaded.progress.settled_children, 1);
        assert_eq!(loaded.progress.completed_children, 1);
        assert_eq!(loaded.progress.no_benefit_children, 1);
        assert_eq!(loaded.progress.published_children, 0);
        assert_eq!(loaded.progress.failed_children, 0);
        assert_eq!(
            loaded.children[0].completion_kind,
            Some(JobCompletionKind::NoBenefit)
        );
    }

    #[test]
    fn ordinary_published_and_no_benefit_outcomes_remain_distinct() {
        let fixture = fixture(2);
        let service = BatchPreviewService::new(fixture.state.clone());
        let request = request(&fixture);
        let preview = service.preview(request.clone()).unwrap();
        let batch = service
            .create(exact_create_request(&request, &preview))
            .unwrap();
        let published_id = &batch.children[0].job_id;
        fixture
            .state
            .database()
            .connection()
            .execute_batch(&format!(
                "UPDATE jobs SET state = 'completed', completed_units = total_units,
               resolved_output_name = 'published.pdf', finished_at = '2026-08-26T09:00:00Z'
             WHERE id = '{published_id}';
             UPDATE job_outputs SET status = 'published', resolved_name = 'published.pdf',
               final_path = 'C:\\safe\\published.pdf', size_bytes = 10,
               sha256 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
               verified_at = '2026-08-26T09:00:00Z', published_at = '2026-08-26T09:00:00Z'
             WHERE job_id = '{published_id}';
             INSERT INTO job_completion_outcomes(job_id, completion_kind, reason, created_at)
             VALUES ('{published_id}', 'published', NULL, '2026-08-26T09:00:00Z');"
            ))
            .unwrap();
        let loaded = service.get(&batch.id).unwrap();
        assert_eq!(
            loaded.children[0].completion_kind,
            Some(JobCompletionKind::Published)
        );
        assert_eq!(loaded.progress.published_children, 1);
        assert_eq!(loaded.progress.no_benefit_children, 0);
    }

    #[test]
    fn all_ineligible_operations_and_versions_fail_before_metadata() {
        let fixture = fixture(1);
        let service = BatchPreviewService::new(fixture.state.clone());
        for (operation_id, operation_version) in [
            ("pdf.compress-balanced", "1.0.0"),
            ("image.to-pdf", "1.0.0"),
            ("pdf.merge", "1.0.0"),
            ("pdf.split", "1.0.0"),
            ("viewer.page-plan", "1.0.0"),
            ("pdf.compress-lossless", "2.0.0"),
            ("unknown.operation", "1.0.0"),
        ] {
            let mut candidate = request(&fixture);
            candidate.operation_id = operation_id.to_owned();
            candidate.operation_version = operation_version.to_owned();
            assert_eq!(
                service.preview(candidate).unwrap_err().code,
                "BATCH_OPERATION_INELIGIBLE"
            );
        }
        assert_eq!(count(&fixture.state, "batch_runs"), 0);
        assert_eq!(count(&fixture.state, "jobs"), 0);
    }

    #[test]
    fn accepts_128_inputs_and_rejects_129_before_inspection() {
        let accepted = fixture(128);
        let service = BatchPreviewService::new(accepted.state.clone());
        assert_eq!(service.preview(request(&accepted)).unwrap().rows.len(), 128);

        let rejected = fixture(1);
        let mut candidate = request(&rejected);
        candidate.input_paths = (0..129)
            .map(|index| format!("C:\\private\\{index}.pdf"))
            .collect();
        let service = BatchPreviewService::new(rejected.state.clone());
        assert_eq!(
            service.preview(candidate).unwrap_err().code,
            "BATCH_REQUEST_INVALID"
        );
    }

    #[test]
    fn missing_qpdf_fails_before_preview_metadata_or_workspace_creation() {
        let fixture = fixture(1);
        let state_without_qpdf = AppState::new(
            Database::open_in_memory().unwrap(),
            fixture.state.workspaces.clone(),
        );
        let workspace_entries = fs::read_dir(state_without_qpdf.workspaces.root())
            .unwrap()
            .count();
        assert_eq!(
            BatchPreviewService::new(state_without_qpdf.clone())
                .preview(request(&fixture))
                .unwrap_err()
                .code,
            "BATCH_DEPENDENCY_UNAVAILABLE"
        );
        assert_eq!(count(&state_without_qpdf, "batch_runs"), 0);
        assert_eq!(
            fs::read_dir(state_without_qpdf.workspaces.root())
                .unwrap()
                .count(),
            workspace_entries
        );
    }

    #[test]
    fn destination_add_file_permission_is_checked_without_a_probe_file() {
        let fixture = fixture(1);
        let service = BatchPreviewService::new(fixture.state.clone());
        let destination_entries = fs::read_dir(&fixture.destination).unwrap().count();
        assert_eq!(
            service
                .preview_with_hooks(
                    request(&fixture),
                    BatchPreviewHooks {
                        destination_permission_denied: true,
                        ..BatchPreviewHooks::default()
                    },
                )
                .unwrap_err()
                .code,
            "BATCH_DESTINATION_PERMISSION_DENIED"
        );
        assert_eq!(
            fs::read_dir(&fixture.destination).unwrap().count(),
            destination_entries
        );
        assert_eq!(count(&fixture.state, "batch_runs"), 0);
        assert_eq!(count(&fixture.state, "jobs"), 0);
    }

    #[test]
    fn retained_destination_guard_blocks_directory_replacement_until_drop() {
        let fixture = fixture(1);
        let service = BatchPreviewService::new(fixture.state.clone());
        let request = request(&fixture);
        let prepared = service
            .prepare(&request, &BatchPreviewHooks::default())
            .unwrap();
        let renamed = fixture
            .destination
            .with_file_name("destination-after-guard-drop");
        assert!(
            fs::rename(&fixture.destination, &renamed).is_err(),
            "the prepared destination handle must deny delete sharing"
        );
        drop(prepared);
        fs::rename(&fixture.destination, &renamed).unwrap();
        fs::rename(&renamed, &fixture.destination).unwrap();
    }
}
