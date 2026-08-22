use serde_json::json;

use crate::contracts::{
    JobsCreateRequest, OperationError, OperationInputs, OperationManifest, OperationOutputs,
    OperationStage, CORE_PDF_OPERATION_VERSION, DIAGNOSTIC_COPY_OPERATION_ID,
    DIAGNOSTIC_COPY_VERSION, IMAGE_TO_PDF_MAX_INPUTS, IMAGE_TO_PDF_OPERATION_ID,
    IMAGE_TO_PDF_VERSION, PDF_COMPRESS_LOSSLESS_OPERATION_ID, PDF_COMPRESS_LOSSLESS_VERSION,
    PDF_EXTRACT_OPERATION_ID, PDF_MERGE_MAX_INPUTS, PDF_MERGE_MIN_INPUTS, PDF_MERGE_OPERATION_ID,
    PDF_MERGE_VERSION, PDF_REMOVE_OPERATION_ID, PDF_REORDER_OPERATION_ID, PDF_ROTATE_OPERATION_ID,
    PDF_SPLIT_OPERATION_ID, QPDF_DEPENDENCY_ID,
};
use crate::path_policy::validate_output_name;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationKind {
    DiagnosticCopy,
    PdfMerge,
    PdfCompressLossless,
    ImageToPdf,
}

pub fn all_manifests() -> Vec<OperationManifest> {
    vec![
        diagnostic_copy_manifest(),
        pdf_merge_manifest(),
        pdf_compress_lossless_manifest(),
        image_to_pdf_manifest(),
        extract_pages_manifest(),
        remove_pages_manifest(),
        reorder_pages_manifest(),
        rotate_pages_manifest(),
        split_manifest(),
    ]
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
        PDF_COMPRESS_LOSSLESS_OPERATION_ID if request.input_paths.len() == 1 => {
            if !request
                .requested_output_name
                .to_ascii_lowercase()
                .ends_with(".pdf")
            {
                return Err(invalid_pdf_output_name());
            }
            Ok(OperationKind::PdfCompressLossless)
        }
        IMAGE_TO_PDF_OPERATION_ID
            if (1..=IMAGE_TO_PDF_MAX_INPUTS).contains(&request.input_paths.len()) =>
        {
            if !request
                .requested_output_name
                .to_ascii_lowercase()
                .ends_with(".pdf")
            {
                return Err(invalid_pdf_output_name());
            }
            Ok(OperationKind::ImageToPdf)
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
        PDF_COMPRESS_LOSSLESS_OPERATION_ID => Err(OperationError::safe(
            "INVALID_INPUT_COUNT",
            "Choose exactly one PDF",
            "Lossless PDF Compression accepts exactly one local PDF.",
            OperationStage::Inspect,
            false,
        )),
        IMAGE_TO_PDF_OPERATION_ID => Err(OperationError::safe(
            "INVALID_INPUT_COUNT",
            "Choose between 1 and 128 images",
            "Image to PDF accepts JPEG, PNG, or WebP files in the selected order.",
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

pub fn pdf_compress_lossless_manifest() -> OperationManifest {
    manifest(
        PDF_COMPRESS_LOSSLESS_OPERATION_ID,
        PDF_COMPRESS_LOSSLESS_VERSION,
        "Lossless PDF Compression",
        "optimize",
        "Losslessly recompresses one local PDF while preserving its referenced document structure.",
        vec!["application/pdf"],
        1,
        1,
        "application/pdf",
        vec![QPDF_DEPENDENCY_ID],
        vec![
            "regular-file",
            "pdf-magic",
            "sha256",
            "qpdf-strict-check",
            "unencrypted",
            "page-count",
            "structural-inventory",
            "reopen",
            "publication-hash",
            "source-immutability",
        ],
    )
}

pub fn image_to_pdf_manifest() -> OperationManifest {
    manifest(
        IMAGE_TO_PDF_OPERATION_ID,
        IMAGE_TO_PDF_VERSION,
        "Images to PDF",
        "convert",
        "Creates one verified PDF page per selected JPEG, PNG, or WebP image in exact order.",
        vec!["image/jpeg", "image/png", "image/webp"],
        1,
        IMAGE_TO_PDF_MAX_INPUTS as u32,
        "application/pdf",
        vec!["document-studio-core", QPDF_DEPENDENCY_ID],
        vec![
            "regular-file",
            "content-codec",
            "sha256",
            "dimension-cap",
            "pixel-cap",
            "qpdf-strict-check",
            "unencrypted",
            "page-count",
            "selected-order",
            "source-immutability",
            "publication-hash",
        ],
    )
}

pub fn extract_pages_manifest() -> OperationManifest {
    core_pdf_manifest(
        PDF_EXTRACT_OPERATION_ID,
        "Extract pages",
        "Exports selected pages once, in the selected order, to one verified PDF.",
        "single",
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["selectedPageIndexes", "outputName"],
            "properties": {
                "selectedPageIndexes": { "type": "array", "minItems": 1, "maxItems": 4096, "uniqueItems": true, "items": { "type": "integer", "minimum": 0, "maximum": 4095 } },
                "outputName": { "type": "string", "minLength": 5, "maxLength": 255, "pattern": "\\.[pP][dD][fF]$" }
            }
        }),
    )
}

pub fn remove_pages_manifest() -> OperationManifest {
    core_pdf_manifest(
        PDF_REMOVE_OPERATION_ID,
        "Remove pages",
        "Exports the complement of the selected pages while keeping at least one page.",
        "single",
        json!({
            "type": "object", "additionalProperties": false,
            "required": ["removedPageIndexes", "outputName"],
            "properties": {
                "removedPageIndexes": { "type": "array", "minItems": 1, "maxItems": 4095, "uniqueItems": true, "items": { "type": "integer", "minimum": 0, "maximum": 4095 } },
                "outputName": { "type": "string", "minLength": 5, "maxLength": 255, "pattern": "\\.[pP][dD][fF]$" }
            }
        }),
    )
}

pub fn reorder_pages_manifest() -> OperationManifest {
    core_pdf_manifest(
        PDF_REORDER_OPERATION_ID,
        "Reorder pages",
        "Exports one exact permutation containing every source page once.",
        "single",
        json!({
            "type": "object", "additionalProperties": false,
            "required": ["orderedPageIndexes", "outputName"],
            "properties": {
                "orderedPageIndexes": { "type": "array", "minItems": 1, "maxItems": 4096, "uniqueItems": true, "items": { "type": "integer", "minimum": 0, "maximum": 4095 } },
                "outputName": { "type": "string", "minLength": 5, "maxLength": 255, "pattern": "\\.[pP][dD][fF]$" }
            }
        }),
    )
}

pub fn rotate_pages_manifest() -> OperationManifest {
    core_pdf_manifest(
        PDF_ROTATE_OPERATION_ID,
        "Rotate pages",
        "Applies clockwise 90, 180, or 270 degree output rotations to selected pages.",
        "single",
        json!({
            "type": "object", "additionalProperties": false,
            "required": ["rotations", "outputName"],
            "properties": {
                "rotations": { "type": "array", "minItems": 1, "maxItems": 4096, "items": {
                    "type": "object", "additionalProperties": false, "required": ["pageIndex", "clockwiseDegrees"],
                    "properties": { "pageIndex": { "type": "integer", "minimum": 0, "maximum": 4095 }, "clockwiseDegrees": { "enum": [90, 180, 270] } }
                } },
                "outputName": { "type": "string", "minLength": 5, "maxLength": 255, "pattern": "\\.[pP][dD][fF]$" }
            }
        }),
    )
}

pub fn split_manifest() -> OperationManifest {
    core_pdf_manifest(
        PDF_SPLIT_OPERATION_ID,
        "Split PDF",
        "Exports 1–128 explicit, ordered, non-overlapping page ranges as independently verified PDFs.",
        "multiple",
        json!({
            "type": "object", "additionalProperties": false,
            "required": ["ranges"],
            "properties": { "ranges": { "type": "array", "minItems": 1, "maxItems": 128, "items": {
                "type": "object", "additionalProperties": false,
                "required": ["startPageIndex", "endPageIndex", "outputName"],
                "properties": {
                    "startPageIndex": { "type": "integer", "minimum": 0, "maximum": 4095 },
                    "endPageIndex": { "type": "integer", "minimum": 0, "maximum": 4095 },
                    "outputName": { "type": "string", "minLength": 5, "maxLength": 255, "pattern": "\\.[pP][dD][fF]$" }
                }
            } } }
        }),
    )
}

fn core_pdf_manifest(
    id: &str,
    name: &str,
    description: &str,
    multiplicity: &str,
    settings_schema: serde_json::Value,
) -> OperationManifest {
    let mut manifest = manifest(
        id,
        CORE_PDF_OPERATION_VERSION,
        name,
        "pdf",
        description,
        vec!["application/pdf"],
        1,
        1,
        "application/pdf",
        vec![QPDF_DEPENDENCY_ID],
        vec![
            "regular-file",
            "pdf-magic",
            "sha256",
            "qpdf-strict-check",
            "unencrypted",
            "page-count",
            "plan-invariants",
            "publication-hash",
        ],
    );
    manifest.settings_schema = settings_schema;
    manifest.outputs.multiplicity = multiplicity.to_owned();
    manifest
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
    fn registry_exposes_accepted_through_g04b_manifests() {
        let manifests = all_manifests();
        assert_eq!(manifests.len(), 9);
        assert_eq!(manifests[0].id, "diagnostic.copy");
        assert_eq!(manifests[1].id, "pdf.merge");
        assert_eq!(manifests[1].inputs.minimum, 2);
        assert_eq!(manifests[1].inputs.maximum, 128);
        assert_eq!(manifests[1].dependencies, ["qpdf"]);
        assert_eq!(manifests[2].id, "pdf.compress-lossless");
        assert_eq!(manifests[2].inputs.minimum, 1);
        assert_eq!(manifests[2].inputs.maximum, 1);
        assert_eq!(manifests[2].dependencies, ["qpdf"]);
        assert!(manifests[2]
            .verification
            .contains(&"structural-inventory".to_owned()));
        assert_eq!(manifests[3].id, "image.to-pdf");
        assert_eq!(manifests[3].inputs.minimum, 1);
        assert_eq!(manifests[3].inputs.maximum, 128);
        assert_eq!(
            manifests[3].inputs.accepted_mime_types,
            ["image/jpeg", "image/png", "image/webp"]
        );
        assert_eq!(manifests[3].dependencies, ["document-studio-core", "qpdf"]);
        assert_eq!(
            manifests[4..]
                .iter()
                .map(|manifest| manifest.id.as_str())
                .collect::<Vec<_>>(),
            [
                "pdf.extract-pages",
                "pdf.remove-pages",
                "pdf.reorder-pages",
                "pdf.rotate-pages",
                "pdf.split",
            ]
        );
        assert!(manifests[4..]
            .iter()
            .all(|manifest| { manifest.version == "1.0.0" && manifest.dependencies == ["qpdf"] }));
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
            validate_create_request(&request("pdf.compress-lossless", 1, "compressed.PDF"))
                .unwrap(),
            OperationKind::PdfCompressLossless
        );
        assert_eq!(
            validate_create_request(&request("image.to-pdf", 1, "images.PDF")).unwrap(),
            OperationKind::ImageToPdf
        );
        assert_eq!(
            validate_create_request(&request("image.to-pdf", 128, "images.pdf")).unwrap(),
            OperationKind::ImageToPdf
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
        assert_eq!(
            validate_create_request(&request("pdf.compress-lossless", 2, "compressed.pdf"))
                .unwrap_err()
                .code,
            "INVALID_INPUT_COUNT"
        );
        assert_eq!(
            validate_create_request(&request("pdf.compress-lossless", 1, "compressed.txt"))
                .unwrap_err()
                .code,
            "INVALID_OUTPUT_NAME"
        );
        assert_eq!(
            validate_create_request(&request("image.to-pdf", 129, "images.pdf"))
                .unwrap_err()
                .code,
            "INVALID_INPUT_COUNT"
        );
        assert_eq!(
            validate_create_request(&request("image.to-pdf", 1, "images.png"))
                .unwrap_err()
                .code,
            "INVALID_OUTPUT_NAME"
        );
    }
}
