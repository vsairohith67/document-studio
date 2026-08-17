use serde_json::json;

use crate::contracts::{
    JobsCreateRequest, OperationError, OperationInputs, OperationManifest, OperationOutputs,
    OperationStage, DIAGNOSTIC_COPY_OPERATION_ID, DIAGNOSTIC_COPY_VERSION, PDF_MERGE_MAX_INPUTS,
    PDF_MERGE_MIN_INPUTS, PDF_MERGE_OPERATION_ID, PDF_MERGE_VERSION, QPDF_DEPENDENCY_ID,
};
use crate::path_policy::validate_output_name;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationKind {
    DiagnosticCopy,
    PdfMerge,
}

pub fn all_manifests() -> Vec<OperationManifest> {
    vec![diagnostic_copy_manifest(), pdf_merge_manifest()]
}

pub fn validate_create_request(
    request: &JobsCreateRequest,
) -> Result<OperationKind, OperationError> {
    validate_output_name(&request.requested_output_name).map_err(|_| invalid_output_name())?;

    match request.operation_id.as_str() {
        DIAGNOSTIC_COPY_OPERATION_ID if request.input_paths.len() == 1 => {
            Ok(OperationKind::DiagnosticCopy)
        }
        PDF_MERGE_OPERATION_ID
            if (PDF_MERGE_MIN_INPUTS..=PDF_MERGE_MAX_INPUTS)
                .contains(&request.input_paths.len()) =>
        {
            if !request
                .requested_output_name
                .to_ascii_lowercase()
                .ends_with(".pdf")
            {
                return Err(invalid_pdf_output_name());
            }
            Ok(OperationKind::PdfMerge)
        }
        DIAGNOSTIC_COPY_OPERATION_ID => Err(OperationError::safe(
            "INVALID_INPUT_COUNT",
            "Choose exactly one input",
            "Diagnostic copy accepts exactly one local file.",
            OperationStage::Inspect,
            false,
        )),
        PDF_MERGE_OPERATION_ID => Err(OperationError::safe(
            "INVALID_INPUT_COUNT",
            "Choose between 2 and 128 PDFs",
            "PDF Merge requires at least two and no more than 128 input files.",
            OperationStage::Inspect,
            false,
        )),
        _ => Err(OperationError::safe(
            "UNSUPPORTED_OPERATION",
            "The operation is not supported",
            "Refresh the operation list and choose an available operation.",
            OperationStage::Inspect,
            false,
        )),
    }
}

pub fn diagnostic_copy_manifest() -> OperationManifest {
    manifest(
        DIAGNOSTIC_COPY_OPERATION_ID,
        DIAGNOSTIC_COPY_VERSION,
        "Diagnostic copy",
        "diagnostics",
        "Streams, verifies, and safely publishes one local file.",
        vec!["application/octet-stream"],
        1,
        1,
        "application/octet-stream",
        vec!["document-studio-core"],
        vec!["sha256", "size", "reopen"],
    )
}

pub fn pdf_merge_manifest() -> OperationManifest {
    manifest(
        PDF_MERGE_OPERATION_ID,
        PDF_MERGE_VERSION,
        "PDF Merge",
        "pdf",
        "Merges 2–128 local PDFs in exact order into one verified page-only PDF.",
        vec!["application/pdf"],
        PDF_MERGE_MIN_INPUTS as u32,
        PDF_MERGE_MAX_INPUTS as u32,
        "application/pdf",
        vec![QPDF_DEPENDENCY_ID],
        vec![
            "regular-file",
            "pdf-magic",
            "sha256",
            "qpdf-strict-check",
            "unencrypted",
            "page-count",
            "reopen",
            "publication-hash",
        ],
    )
}

#[allow(clippy::too_many_arguments)]
fn manifest(
    id: &str,
    version: &str,
    name: &str,
    category: &str,
    description: &str,
    accepted_mime_types: Vec<&str>,
    minimum: u32,
    maximum: u32,
    output_mime_type: &str,
    dependencies: Vec<&str>,
    verification: Vec<&str>,
) -> OperationManifest {
    OperationManifest {
        id: id.to_owned(),
        version: version.to_owned(),
        name: name.to_owned(),
        category: category.to_owned(),
        description: description.to_owned(),
        risk: "normal".to_owned(),
        locality: "local".to_owned(),
        inputs: OperationInputs {
            accepted_mime_types: accepted_mime_types.into_iter().map(str::to_owned).collect(),
            minimum,
            maximum,
            allow_directories: false,
        },
        settings_schema: json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {}
        }),
        outputs: OperationOutputs {
            mime_type: output_mime_type.to_owned(),
            multiplicity: "single".to_owned(),
        },
        dependencies: dependencies.into_iter().map(str::to_owned).collect(),
        verification: verification.into_iter().map(str::to_owned).collect(),
        stages: vec![
            OperationStage::Inspect,
            OperationStage::Preflight,
            OperationStage::Estimate,
            OperationStage::Plan,
            OperationStage::Execute,
            OperationStage::Verify,
            OperationStage::Publish,
            OperationStage::Audit,
            OperationStage::Cleanup,
        ],
    }
}

fn invalid_output_name() -> OperationError {
    OperationError::safe(
        "INVALID_OUTPUT_NAME",
        "The output name is not valid",
        "Use a Windows-safe file name without a path.",
        OperationStage::Preflight,
        false,
    )
}

fn invalid_pdf_output_name() -> OperationError {
    OperationError::safe(
        "INVALID_OUTPUT_NAME",
        "The output name must end in .pdf",
        "Choose a Windows-safe PDF file name without a path.",
        OperationStage::Preflight,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::{all_manifests, validate_create_request, OperationKind};
    use crate::contracts::JobsCreateRequest;

    fn request(operation_id: &str, count: usize, name: &str) -> JobsCreateRequest {
        JobsCreateRequest {
            operation_id: operation_id.to_owned(),
            input_paths: (0..count)
                .map(|index| format!(r"C:\in\{index}.pdf"))
                .collect(),
            destination_directory: r"C:\out".to_owned(),
            requested_output_name: name.to_owned(),
        }
    }

    #[test]
    fn registry_exposes_foundation_and_pdf_merge_manifests() {
        let manifests = all_manifests();
        assert_eq!(manifests.len(), 2);
        assert_eq!(manifests[0].id, "diagnostic.copy");
        assert_eq!(manifests[1].id, "pdf.merge");
        assert_eq!(manifests[1].inputs.minimum, 2);
        assert_eq!(manifests[1].inputs.maximum, 128);
        assert_eq!(manifests[1].dependencies, ["qpdf"]);
    }

    #[test]
    fn request_validation_preserves_exact_operation_limits() {
        assert_eq!(
            validate_create_request(&request("diagnostic.copy", 1, "copy.bin")).unwrap(),
            OperationKind::DiagnosticCopy
        );
        assert_eq!(
            validate_create_request(&request("pdf.merge", 2, "merged.PDF")).unwrap(),
            OperationKind::PdfMerge
        );
        assert_eq!(
            validate_create_request(&request("pdf.merge", 128, "merged.pdf")).unwrap(),
            OperationKind::PdfMerge
        );
        assert_eq!(
            validate_create_request(&request("pdf.merge", 1, "merged.pdf"))
                .unwrap_err()
                .code,
            "INVALID_INPUT_COUNT"
        );
        assert_eq!(
            validate_create_request(&request("pdf.merge", 129, "merged.pdf"))
                .unwrap_err()
                .code,
            "INVALID_INPUT_COUNT"
        );
        assert_eq!(
            validate_create_request(&request("pdf.merge", 2, "merged.txt"))
                .unwrap_err()
                .code,
            "INVALID_OUTPUT_NAME"
        );
    }
}
