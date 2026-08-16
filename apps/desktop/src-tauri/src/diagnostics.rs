use chrono::{SecondsFormat, Utc};

use crate::contracts::{
    DependencyDiagnostic, DependencyKind, DependencyStatus, OperationError, OperationStage,
};
use crate::database::Database;

pub fn scan_dependencies(
    database: &mut Database,
) -> Result<Vec<DependencyDiagnostic>, OperationError> {
    let checked_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let diagnostics = vec![
        DependencyDiagnostic {
            id: "document-studio-core".to_owned(),
            kind: DependencyKind::BuiltIn,
            status: DependencyStatus::Available,
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            capabilities: vec!["diagnostic.copy".to_owned(), "sha256".to_owned()],
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
        deferred("qpdf", &checked_at),
        deferred("pdfjs", &checked_at),
        deferred("libreoffice", &checked_at),
        deferred("ocrmypdf", &checked_at),
        deferred("tesseract", &checked_at),
    ];
    for diagnostic in &diagnostics {
        database.upsert_dependency(diagnostic).map_err(|_| {
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
