use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DIAGNOSTIC_COPY_OPERATION_ID: &str = "diagnostic.copy";
pub const DIAGNOSTIC_COPY_VERSION: &str = "1.0.1";
pub const PDF_MERGE_OPERATION_ID: &str = "pdf.merge";
pub const PDF_MERGE_VERSION: &str = "1.0.0";
pub const PDF_MERGE_MIN_INPUTS: usize = 2;
pub const PDF_MERGE_MAX_INPUTS: usize = 128;
pub const PDF_COMPRESS_LOSSLESS_OPERATION_ID: &str = "pdf.compress-lossless";
pub const PDF_COMPRESS_LOSSLESS_VERSION: &str = "1.0.0";
pub const BALANCED_COMPRESSION_OPERATION_ID: &str = "pdf.compress-balanced";
pub const BALANCED_COMPRESSION_VERSION: &str = "1.0.0";
pub const BALANCED_COMPRESSION_PROFILE: &str = "balanced-v1";
pub const BALANCED_COMPRESSION_JPEG_QUALITY: u8 = 82;
pub const BALANCED_COMPRESSION_MAX_AFFECTED_PAGES: usize = 128;
pub const BALANCED_COMPRESSION_MAX_TOTAL_PIXELS: u64 = 268_435_456;
pub const BALANCED_VISUAL_READY_EVENT_NAME: &str = "document-studio-balanced-visual-ready-v1";
pub const IMAGE_TO_PDF_OPERATION_ID: &str = "image.to-pdf";
pub const IMAGE_TO_PDF_VERSION: &str = "1.0.0";
pub const IMAGE_TO_PDF_MAX_INPUTS: usize = 128;
pub const PDF_TO_IMAGES_OPERATION_ID: &str = "pdf.to-images";
pub const PDF_TO_IMAGES_VERSION: &str = "1.0.0";
pub const PDF_TO_IMAGES_MAX_OUTPUTS: usize = 128;
pub const PDF_TO_IMAGES_MAX_TOTAL_PIXELS: u64 = 67_108_864;
pub const PDF_TO_IMAGES_JPEG_QUALITY: u8 = 92;
pub const PDFJS_VERSION: &str = "6.2.108";
pub const IMAGE_MAX_DIMENSION: u32 = 8_192;
pub const IMAGE_MAX_PIXELS: u64 = 16_777_216;
pub const IMAGE_TO_PDF_MAX_TOTAL_PIXELS: u64 = 67_108_864;
pub const IMAGE_TO_PDF_MAX_TOTAL_INPUT_BYTES: u64 = 536_870_912;
pub const PDF_EXTRACT_OPERATION_ID: &str = "pdf.extract-pages";
pub const PDF_REMOVE_OPERATION_ID: &str = "pdf.remove-pages";
pub const PDF_REORDER_OPERATION_ID: &str = "pdf.reorder-pages";
pub const PDF_ROTATE_OPERATION_ID: &str = "pdf.rotate-pages";
pub const PDF_SPLIT_OPERATION_ID: &str = "pdf.split";
pub const CORE_PDF_OPERATION_VERSION: &str = "1.0.0";
pub const CORE_PDF_MAX_PAGES: u32 = 4096;
pub const PDF_SPLIT_MAX_OUTPUTS: usize = 128;
pub const OPERATION_PLAN_SCHEMA_VERSION: u8 = 1;
pub const OPERATION_PLAN_MAX_BYTES: usize = 65_536;
pub const OPERATION_SPEC_SCHEMA_VERSION: u8 = 1;
pub const OPERATION_SPEC_MAX_BYTES: usize = 65_536;
pub const BATCH_PREVIEW_SCHEMA_VERSION: u8 = 1;
pub const BATCH_PREVIEW_MAX_BYTES: usize = 262_144;
pub const BATCH_NAMING_TEMPLATE_MAX_BYTES: usize = 1_024;
pub const BATCH_MAX_INPUTS: usize = 128;
pub const BATCH_DEFAULT_NAMING_TEMPLATE: &str = "{stem}-compressed.pdf";
pub const QPDF_DEPENDENCY_ID: &str = "qpdf";
pub const QPDF_VERSION: &str = "12.3.2";
pub const LEGACY_DIAGNOSTIC_COPY_VERSION: &str = "1.0.0";
pub const LEGACY_CLEANUP_PROVEN: &str = "LEGACY_CLEANUP_PROVEN";
pub const LEGACY_CLEANUP_UNPROVEN: &str = "LEGACY_CLEANUP_UNPROVEN";
pub const HISTORY_RETENTION_SCOPE: &str = "application";
pub const HISTORY_RETENTION_KEY: &str = "history.retention_days";
pub const DEFAULT_HISTORY_RETENTION_DAYS: u64 = 30;
pub const MAX_HISTORY_PURGE: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Queued,
    Inspecting,
    Preflight,
    Ready,
    Running,
    Verifying,
    Publishing,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

impl JobState {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Inspecting => "inspecting",
            Self::Preflight => "preflight",
            Self::Ready => "ready",
            Self::Running => "running",
            Self::Verifying => "verifying",
            Self::Publishing => "publishing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    pub fn from_contract(value: &str) -> Option<Self> {
        Some(match value {
            "queued" => Self::Queued,
            "inspecting" => Self::Inspecting,
            "preflight" => Self::Preflight,
            "ready" => Self::Ready,
            "running" => Self::Running,
            "verifying" => Self::Verifying,
            "publishing" => Self::Publishing,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "interrupted" => Self::Interrupted,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OperationStage {
    Inspect,
    Preflight,
    Estimate,
    Plan,
    Execute,
    Verify,
    Publish,
    Audit,
    Cleanup,
    Recovery,
}

impl OperationStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inspect => "inspect",
            Self::Preflight => "preflight",
            Self::Estimate => "estimate",
            Self::Plan => "plan",
            Self::Execute => "execute",
            Self::Verify => "verify",
            Self::Publish => "publish",
            Self::Audit => "audit",
            Self::Cleanup => "cleanup",
            Self::Recovery => "recovery",
        }
    }

    pub fn from_contract(value: &str) -> Option<Self> {
        Some(match value {
            "inspect" => Self::Inspect,
            "preflight" => Self::Preflight,
            "estimate" => Self::Estimate,
            "plan" => Self::Plan,
            "execute" => Self::Execute,
            "verify" => Self::Verify,
            "publish" => Self::Publish,
            "audit" => Self::Audit,
            "cleanup" => Self::Cleanup,
            "recovery" => Self::Recovery,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProgressUnit {
    Bytes,
    Items,
    Steps,
}

impl ProgressUnit {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bytes => "bytes",
            Self::Items => "items",
            Self::Steps => "steps",
        }
    }

    pub fn from_contract(value: &str) -> Option<Self> {
        Some(match value {
            "bytes" => Self::Bytes,
            "items" => Self::Items,
            "steps" => Self::Steps,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputStatus {
    Planned,
    Staged,
    Verified,
    Publishing,
    Published,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JobCompletionKind {
    Published,
    NoBenefit,
}

impl JobCompletionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Published => "published",
            Self::NoBenefit => "no-benefit",
        }
    }

    pub fn from_contract(value: &str) -> Option<Self> {
        Some(match value {
            "published" => Self::Published,
            "no-benefit" => Self::NoBenefit,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JobCompletionReason {
    SavingsThresholdNotMet,
}

impl JobCompletionReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SavingsThresholdNotMet => "savings-threshold-not-met",
        }
    }

    pub fn from_contract(value: &str) -> Option<Self> {
        Some(match value {
            "savings-threshold-not-met" => Self::SavingsThresholdNotMet,
            _ => return None,
        })
    }
}

impl OutputStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Staged => "staged",
            Self::Verified => "verified",
            Self::Publishing => "publishing",
            Self::Published => "published",
        }
    }

    pub fn from_contract(value: &str) -> Option<Self> {
        Some(match value {
            "planned" => Self::Planned,
            "staged" => Self::Staged,
            "verified" => Self::Verified,
            "publishing" => Self::Publishing,
            "published" => Self::Published,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DependencyStatus {
    Available,
    Missing,
    Unhealthy,
    Deferred,
    NotRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DependencyKind {
    BuiltIn,
    External,
    Deferred,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationError {
    pub code: String,
    pub title: String,
    pub detail: String,
    pub stage: OperationStage,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help_id: Option<String>,
}

impl OperationError {
    pub fn safe(
        code: impl Into<String>,
        title: impl Into<String>,
        detail: impl Into<String>,
        stage: OperationStage,
        retryable: bool,
    ) -> Self {
        Self {
            code: code.into(),
            title: title.into(),
            detail: detail.into(),
            stage,
            retryable,
            input_index: None,
            help_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobProgress {
    pub completed_units: u64,
    pub total_units: u64,
    pub unit: ProgressUnit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobInput {
    pub ordinal: u32,
    pub display_name: String,
    pub source_path: String,
    pub canonical_path: String,
    pub file_identity: String,
    pub size_bytes: u64,
    pub modified_at: String,
    pub mime_type: String,
    pub sha256: Option<String>,
    pub password_reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobOutput {
    pub ordinal: u32,
    pub requested_name: String,
    pub resolved_name: Option<String>,
    pub staging_path: Option<String>,
    pub partial_path: Option<String>,
    pub final_path: Option<String>,
    pub size_bytes: Option<u64>,
    pub mime_type: String,
    pub sha256: Option<String>,
    pub status: OutputStatus,
    pub verified_at: Option<String>,
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRecord {
    pub id: String,
    pub operation_id: String,
    pub operation_version: String,
    pub state: JobState,
    pub stage: Option<OperationStage>,
    pub sequence: u64,
    pub progress: JobProgress,
    pub destination_directory: String,
    pub requested_output_name: String,
    pub resolved_output_name: Option<String>,
    pub cancellation_requested_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
    pub version: u64,
    pub completion_kind: Option<JobCompletionKind>,
    pub reason: Option<JobCompletionReason>,
    pub inputs: Vec<JobInput>,
    pub outputs: Vec<JobOutput>,
    pub errors: Vec<OperationError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub schema_version: u8,
    pub sequence: u64,
    pub emitted_at: String,
    pub job_id: String,
    pub operation_id: String,
    pub state: JobState,
    pub stage: OperationStage,
    pub completed_units: u64,
    pub total_units: u64,
    pub unit: ProgressUnit,
    pub message_code: String,
    pub message: String,
    pub cancellable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyDiagnostic {
    pub id: String,
    pub kind: DependencyKind,
    pub status: DependencyStatus,
    pub version: Option<String>,
    pub capabilities: Vec<String>,
    pub checked_at: String,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingRecord {
    pub scope: String,
    pub key: String,
    pub value: Value,
    pub version: u64,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationManifest {
    pub id: String,
    pub version: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub risk: String,
    pub locality: String,
    pub inputs: OperationInputs,
    pub settings_schema: Value,
    pub outputs: OperationOutputs,
    pub dependencies: Vec<String>,
    pub verification: Vec<String>,
    pub stages: Vec<OperationStage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationInputs {
    pub accepted_mime_types: Vec<String>,
    pub minimum: u32,
    pub maximum: u32,
    pub allow_directories: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationOutputs {
    pub mime_type: String,
    pub multiplicity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobsCreateRequest {
    pub operation_id: String,
    pub input_paths: Vec<String>,
    pub destination_directory: String,
    pub requested_output_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BatchSettings {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BatchPreviewRequest {
    pub schema_version: u8,
    pub operation_id: String,
    pub operation_version: String,
    pub settings: BatchSettings,
    pub input_paths: Vec<String>,
    pub destination_directory: String,
    pub naming_template: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BatchCreateRequest {
    pub schema_version: u8,
    pub operation_id: String,
    pub operation_version: String,
    pub settings: BatchSettings,
    pub input_paths: Vec<String>,
    pub destination_directory: String,
    pub naming_template: String,
    pub preview_sha256: String,
    pub optimistic_version: u64,
}

impl From<&BatchCreateRequest> for BatchPreviewRequest {
    fn from(request: &BatchCreateRequest) -> Self {
        Self {
            schema_version: request.schema_version,
            operation_id: request.operation_id.clone(),
            operation_version: request.operation_version.clone(),
            settings: request.settings.clone(),
            input_paths: request.input_paths.clone(),
            destination_directory: request.destination_directory.clone(),
            naming_template: request.naming_template.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BatchPreviewRow {
    pub ordinal: u32,
    pub source_name: String,
    pub output_name: String,
    pub collision_index: u32,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BatchDiskEstimate {
    pub workspace_peak_bytes: u64,
    pub destination_total_bytes: u64,
    pub combined_required_bytes: u64,
    pub workspace_and_destination_share_volume: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BatchPreviewResponse {
    pub schema_version: u8,
    pub operation_id: String,
    pub operation_version: String,
    pub settings_sha256: String,
    pub naming_template: String,
    pub rows: Vec<BatchPreviewRow>,
    pub disk_estimate: BatchDiskEstimate,
    pub preview_sha256: String,
    pub canonical_size_bytes: u32,
    pub optimistic_version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BatchState {
    Queued,
    Active,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

impl BatchState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Active => "active",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    pub fn from_contract(value: &str) -> Option<Self> {
        Some(match value {
            "queued" => Self::Queued,
            "active" => Self::Active,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "interrupted" => Self::Interrupted,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BatchProgress {
    pub settled_children: u32,
    pub total_children: u32,
    pub completed_children: u32,
    pub failed_children: u32,
    pub cancelled_children: u32,
    pub interrupted_children: u32,
    pub published_children: u32,
    pub no_benefit_children: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BatchChildRecord {
    pub ordinal: u32,
    pub job_id: String,
    pub state: JobState,
    pub completion_kind: Option<JobCompletionKind>,
    pub reason: Option<JobCompletionReason>,
    pub requested_name: String,
    pub planned_name: String,
    pub collision_index: u32,
    pub progress: JobProgress,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BatchRecord {
    pub id: String,
    pub schema_version: u8,
    pub operation_id: String,
    pub operation_version: String,
    pub state: BatchState,
    pub preview_sha256: String,
    pub settings_sha256: String,
    pub naming_template: String,
    pub optimistic_version: u64,
    pub disk_estimate: BatchDiskEstimate,
    pub progress: BatchProgress,
    pub created_at: String,
    pub updated_at: String,
    pub version: u64,
    pub children: Vec<BatchChildRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BatchGetRequest {
    pub batch_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BalancedCompressionSettings {
    pub profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BalancedCompressionJobCreateRequest {
    pub operation_id: String,
    pub input_paths: Vec<String>,
    pub destination_directory: String,
    pub requested_output_name: String,
    pub settings: BalancedCompressionSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationSpecEnvelope {
    pub schema_version: u8,
    pub operation_id: String,
    pub settings: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredOperationSpec {
    pub envelope: OperationSpecEnvelope,
    pub canonical_json: String,
    pub sha256: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobWarning {
    pub code: String,
    pub sanitized_detail: String,
    pub input_index: Option<u32>,
    pub page_index: Option<u32>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub enum OutputRotation {
    Clockwise90,
    Clockwise180,
    Clockwise270,
}

impl OutputRotation {
    pub const fn degrees(self) -> u16 {
        match self {
            Self::Clockwise90 => 90,
            Self::Clockwise180 => 180,
            Self::Clockwise270 => 270,
        }
    }
}

impl TryFrom<u16> for OutputRotation {
    type Error = &'static str;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            90 => Ok(Self::Clockwise90),
            180 => Ok(Self::Clockwise180),
            270 => Ok(Self::Clockwise270),
            _ => Err("rotation must be 90, 180, or 270 degrees"),
        }
    }
}

impl From<OutputRotation> for u16 {
    fn from(value: OutputRotation) -> Self {
        value.degrees()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtractPagesPlan {
    pub selected_page_indexes: Vec<u32>,
    pub output_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemovePagesPlan {
    pub removed_page_indexes: Vec<u32>,
    pub output_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReorderPagesPlan {
    pub ordered_page_indexes: Vec<u32>,
    pub output_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageRotation {
    pub page_index: u32,
    pub clockwise_degrees: OutputRotation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RotatePagesPlan {
    pub rotations: Vec<PageRotation>,
    pub output_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SplitOutputRange {
    pub start_page_index: u32,
    pub end_page_index: u32,
    pub output_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SplitPlan {
    pub ranges: Vec<SplitOutputRange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CorePdfPlanPayload {
    Extract(ExtractPagesPlan),
    Remove(RemovePagesPlan),
    Reorder(ReorderPagesPlan),
    Rotate(RotatePagesPlan),
    Split(SplitPlan),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperationPlanEnvelope {
    pub schema_version: u8,
    pub operation_id: String,
    pub source_page_count: u32,
    pub payload: CorePdfPlanPayload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredOperationPlan {
    pub envelope: OperationPlanEnvelope,
    pub canonical_json: String,
    pub sha256: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ViewerDocumentMetadata {
    pub session_id: String,
    pub generation: u64,
    pub display_name: String,
    pub size_bytes: u64,
    pub modified_at: String,
    pub mime_type: String,
    pub file_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BalancedRenderPageTicket {
    pub page_ordinal: u32,
    pub source_page_index: u32,
    pub nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BalancedCompressionVisualSession {
    pub job_id: String,
    pub render_session_id: String,
    pub source: ViewerDocumentMetadata,
    pub candidate: ViewerDocumentMetadata,
    pub pages: Vec<BalancedRenderPageTicket>,
    pub selected_image_count: u32,
    pub skipped_image_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BalancedRenderSide {
    Source,
    Candidate,
}

impl BalancedRenderSide {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Candidate => "candidate",
        }
    }

    pub fn from_contract(value: &str) -> Option<Self> {
        Some(match value {
            "source" => Self::Source,
            "candidate" => Self::Candidate,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BalancedCompressionSkipReason {
    BelowMinimum,
    UnsupportedFilter,
    DecodeParameters,
    UnsupportedColorspace,
    NonRgb8,
    MaskOrTransparency,
    ExternalOrAlternate,
    UnsafeResourceAncestry,
    AmbiguousSharedUse,
    InlineImage,
    CandidateNotSmaller,
    CandidateQuality,
    CandidateDecode,
}

impl BalancedCompressionSkipReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BelowMinimum => "below-minimum",
            Self::UnsupportedFilter => "unsupported-filter",
            Self::DecodeParameters => "decode-parameters",
            Self::UnsupportedColorspace => "unsupported-colorspace",
            Self::NonRgb8 => "non-rgb8",
            Self::MaskOrTransparency => "mask-or-transparency",
            Self::ExternalOrAlternate => "external-or-alternate",
            Self::UnsafeResourceAncestry => "unsafe-resource-ancestry",
            Self::AmbiguousSharedUse => "ambiguous-shared-use",
            Self::InlineImage => "inline-image",
            Self::CandidateNotSmaller => "candidate-not-smaller",
            Self::CandidateQuality => "candidate-quality",
            Self::CandidateDecode => "candidate-decode",
        }
    }

    pub fn from_contract(value: &str) -> Option<Self> {
        Some(match value {
            "below-minimum" => Self::BelowMinimum,
            "unsupported-filter" => Self::UnsupportedFilter,
            "decode-parameters" => Self::DecodeParameters,
            "unsupported-colorspace" => Self::UnsupportedColorspace,
            "non-rgb8" => Self::NonRgb8,
            "mask-or-transparency" => Self::MaskOrTransparency,
            "external-or-alternate" => Self::ExternalOrAlternate,
            "unsafe-resource-ancestry" => Self::UnsafeResourceAncestry,
            "ambiguous-shared-use" => Self::AmbiguousSharedUse,
            "inline-image" => Self::InlineImage,
            "candidate-not-smaller" => Self::CandidateNotSmaller,
            "candidate-quality" => Self::CandidateQuality,
            "candidate-decode" => Self::CandidateDecode,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BalancedCompressionSkipCount {
    pub reason: BalancedCompressionSkipReason,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BalancedCompressionAudit {
    pub profile: String,
    pub source_bytes: u64,
    pub candidate_bytes: u64,
    pub saved_bytes: u64,
    pub saved_percent: f64,
    pub selected_images: u32,
    pub skipped_images: u32,
    pub affected_pages: u32,
    pub compared_pages: u32,
    pub minimum_ssim: Option<f64>,
    pub minimum_psnr_db: Option<f64>,
    pub psnr_is_infinite: bool,
    pub maximum_changed_pixels: u64,
    pub maximum_total_pixels: u64,
    pub quality_passed: bool,
    pub size_gate_passed: bool,
    pub structural_proof_sha256: String,
    pub skipped_reasons: Vec<BalancedCompressionSkipCount>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ViewerRangeRequest {
    pub session_id: String,
    pub generation: u64,
    pub begin: u64,
    pub end: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ViewerSessionRequest {
    pub session_id: String,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DestinationGrant {
    pub grant_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DestinationGrantRequest {
    pub grant_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CorePdfJobCreateRequest {
    pub viewer_session_id: String,
    pub viewer_generation: u64,
    pub destination_grant_id: String,
    pub plan: OperationPlanEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobIdRequest {
    pub job_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesInspectRequest {
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInspection {
    pub path: String,
    pub display_name: String,
    pub size_bytes: u64,
    pub modified_at: String,
    pub mime_type: String,
    pub file_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryListRequest {
    pub limit: u32,
    pub before_updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryDeleteRequest {
    pub job_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingGetRequest {
    pub scope: String,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingSetRequest {
    pub scope: String,
    pub key: String,
    pub value: Value,
    pub expected_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStatus {
    pub product: String,
    pub phase: String,
    pub offline_by_default: bool,
    pub database_schema_version: u32,
    pub webview2_runtime_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelResponse {
    pub outcome: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PdfImageFormat {
    Jpeg,
    Png,
    Webp,
}

impl PdfImageFormat {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Webp => "webp",
        }
    }

    pub const fn mime_type(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::Webp => "image/webp",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfToImagesPagePlan {
    pub source_page_index: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfToImagesJobCreateRequest {
    pub viewer_session_id: String,
    pub viewer_generation: u64,
    pub destination_grant_id: String,
    pub source_page_count: u32,
    pub pages: Vec<PdfToImagesPagePlan>,
    pub format: PdfImageFormat,
    pub dpi: u16,
    pub output_stem: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfPixelTransferTicket {
    pub page_ordinal: u32,
    pub source_page_index: u32,
    pub nonce: String,
    pub expected_width: u32,
    pub expected_height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfToImagesJobSession {
    pub job: JobRecord,
    pub render_session_id: String,
    pub pages: Vec<PdfPixelTransferTicket>,
}

pub const JOB_PROGRESS_EVENT_NAME: &str = "document-studio-job-progress-v1";
pub const VIEWER_DOCUMENT_OPENED_EVENT_NAME: &str = "document-studio-viewer-opened-v1";
