use chrono::{SecondsFormat, Utc};

use crate::app_state::AppState;
use crate::contracts::{
    DependencyDiagnostic, DependencyKind, DependencyStatus, OperationError, OperationStage,
    BALANCED_COMPRESSION_OPERATION_ID, IMAGE_TO_PDF_OPERATION_ID, PDFJS_VERSION,
    PDF_COMPRESS_LOSSLESS_OPERATION_ID, PDF_MERGE_OPERATION_ID, PDF_TO_IMAGES_OPERATION_ID,
    QPDF_DEPENDENCY_ID,
};
use crate::process_sandbox::{
    authorize_qpdf_paths, ensure_production_profile, run_sandboxed_capture, SandboxLaunchSpec,
};
use crate::text_to_pdf::TEXT_TO_PDF_OPERATION_ID;
use std::ffi::OsString;
use std::time::Duration;
use uuid::Uuid;

pub fn scan_dependencies(state: &AppState) -> Result<Vec<DependencyDiagnostic>, OperationError> {
    let checked_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let diagnostics = vec![
        DependencyDiagnostic {
            id: "document-studio-core".to_owned(),
            kind: DependencyKind::BuiltIn,
            status: DependencyStatus::Available,
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            capabilities: vec![
                "diagnostic.copy".to_owned(),
                IMAGE_TO_PDF_OPERATION_ID.to_owned(),
                PDF_TO_IMAGES_OPERATION_ID.to_owned(),
                TEXT_TO_PDF_OPERATION_ID.to_owned(),
                "sha256".to_owned(),
            ],
            checked_at: checked_at.clone(),
            error_code: None,
        },
        DependencyDiagnostic {
            id: "sqlite".to_owned(),
            kind: DependencyKind::BuiltIn,
            status: DependencyStatus::Available,
            version: Some(rusqlite::version().to_owned()),
            capabilities: vec!["metadata".to_owned(), "migrations".to_owned()],
            checked_at: checked_at.clone(),
            error_code: None,
        },
        qpdf_diagnostic(state, &checked_at),
        DependencyDiagnostic {
            id: "pdfjs".to_owned(),
            kind: DependencyKind::BuiltIn,
            status: DependencyStatus::Available,
            version: Some(PDFJS_VERSION.to_owned()),
            capabilities: vec![
                PDF_TO_IMAGES_OPERATION_ID.to_owned(),
                BALANCED_COMPRESSION_OPERATION_ID.to_owned(),
            ],
            checked_at: checked_at.clone(),
            error_code: None,
        },
        deferred("libreoffice", &checked_at),
        deferred("ocrmypdf", &checked_at),
        deferred("tesseract", &checked_at),
    ];
    for diagnostic in &diagnostics {
        state
            .database()
            .upsert_dependency(diagnostic)
            .map_err(|_| {
                OperationError::safe(
                    "METADATA_WRITE_FAILED",
                    "Dependency status could not be saved",
                    "Dependency diagnostics finished, but their metadata could not be stored.",
                    OperationStage::Audit,
                    true,
                )
            })?;
    }
    Ok(diagnostics)
}

fn qpdf_diagnostic(state: &AppState, checked_at: &str) -> DependencyDiagnostic {
    let result = (|| {
        let manager = state.qpdf.as_ref().ok_or(())?;
        let runtime = manager.get_or_prepare().map_err(|_| ())?;
        let profile = ensure_production_profile().map_err(|_| ())?;
        let probe_id = Uuid::new_v4().hyphenated().to_string();
        let workspace = state.workspaces.create_job(&probe_id).map_err(|_| ())?;
        let probe = (|| {
            authorize_qpdf_paths(&profile, &runtime.bin, &workspace).map_err(|_| ())?;
            let arguments = [OsString::from("--version")];
            let specification = SandboxLaunchSpec {
                executable: &runtime.executable,
                arguments: &arguments,
                working_directory: &workspace.root,
                temporary_directory: &workspace.temporary,
            };
            let execution =
                run_sandboxed_capture(&profile, &specification, Duration::from_secs(10))
                    .map_err(|_| ())?;
            if execution.exit_code != 0
                || execution.stderr.len() > crate::process_sandbox::CAPTURE_LIMIT_BYTES
                || !crate::qpdf::version_output_is_expected(&execution.stdout)
            {
                return Err(());
            }
            Ok(())
        })();
        if state.workspaces.cleanup_job(&probe_id).is_err() {
            return Err(());
        }
        probe
    })();

    DependencyDiagnostic {
        id: QPDF_DEPENDENCY_ID.to_owned(),
        kind: DependencyKind::External,
        status: if result.is_ok() {
            DependencyStatus::Available
        } else {
            DependencyStatus::Unhealthy
        },
        version: Some(crate::contracts::QPDF_VERSION.to_owned()),
        capabilities: vec![
            PDF_MERGE_OPERATION_ID.to_owned(),
            PDF_COMPRESS_LOSSLESS_OPERATION_ID.to_owned(),
            BALANCED_COMPRESSION_OPERATION_ID.to_owned(),
            IMAGE_TO_PDF_OPERATION_ID.to_owned(),
            PDF_TO_IMAGES_OPERATION_ID.to_owned(),
            TEXT_TO_PDF_OPERATION_ID.to_owned(),
        ],
        checked_at: checked_at.to_owned(),
        error_code: result.err().map(|_| "QPDF_RUNTIME_UNAVAILABLE".to_owned()),
    }
}

fn deferred(id: &str, checked_at: &str) -> DependencyDiagnostic {
    DependencyDiagnostic {
        id: id.to_owned(),
        kind: DependencyKind::Deferred,
        status: DependencyStatus::NotRequired,
        version: None,
        capabilities: Vec::new(),
        checked_at: checked_at.to_owned(),
        error_code: None,
    }
}
