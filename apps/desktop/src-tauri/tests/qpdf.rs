use std::ffi::OsString;
use std::path::{Path, PathBuf};

use document_studio_lib::qpdf::{
    build_production_merge_arguments, interpret_encryption_check_exit,
    interpret_structural_check_exit, EncryptionCheckOutcome, OrdinalSnapshot, QpdfContractError,
    StructuralCheckOutcome, MERGED_STAGING_RELATIVE_PATH, QPDF_BUNDLE_MANIFEST_JSON,
};

#[test]
fn compiled_qpdf_manifest_matches_the_reviewed_zero_capability_bundle() {
    let manifest: serde_json::Value = serde_json::from_str(QPDF_BUNDLE_MANIFEST_JSON).unwrap();
    assert_eq!(manifest["version"], "12.3.2");
    assert_eq!(
        manifest["sourceArchiveSha256"],
        "8941870a604e7c87ed24566b038d46c24ce76616254d2383c578f60c0677f202"
    );
    assert_eq!(
        manifest["appContainerProfile"],
        "DocumentStudio.PdfEngine.Qpdf.V1"
    );
    assert_eq!(manifest["appContainerConfigurationVersion"], 1);
    assert_eq!(
        manifest["appContainerCapabilities"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(manifest["files"].as_array().unwrap().len(), 15);
}

#[test]
fn production_merge_arguments_match_the_accepted_qpdf_12_3_2_shape() {
    let arguments = build_production_merge_arguments(
        &[
            OrdinalSnapshot::for_ordinal(0),
            OrdinalSnapshot::for_ordinal(1),
            OrdinalSnapshot::for_ordinal(2),
        ],
        Path::new(MERGED_STAGING_RELATIVE_PATH),
    )
    .unwrap();
    let expected = [
        "--empty",
        "--suppress-recovery",
        "--stream-data=preserve",
        "--object-streams=preserve",
        "--remove-info",
        "--remove-metadata",
        "--remove-page-labels",
        "--pages",
        r"--file=inputs\source-0000.pdf",
        r"--file=inputs\source-0001.pdf",
        r"--file=inputs\source-0002.pdf",
        "--",
        r"staging\merged.pdf",
    ]
    .map(OsString::from)
    .to_vec();
    assert_eq!(arguments, expected);
    assert!(!arguments
        .iter()
        .any(|argument| argument == "--deterministic-id"));
}

#[test]
fn production_builder_rejects_shared_or_misordered_snapshot_paths() {
    let shared = PathBuf::from(r"inputs\source-0000.pdf");
    let error = build_production_merge_arguments(
        &[
            OrdinalSnapshot {
                ordinal: 0,
                relative_path: shared.clone(),
            },
            OrdinalSnapshot {
                ordinal: 1,
                relative_path: shared,
            },
        ],
        Path::new(MERGED_STAGING_RELATIVE_PATH),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        QpdfContractError::SnapshotPath | QpdfContractError::DuplicateSnapshotPath
    ));

    assert_eq!(
        build_production_merge_arguments(
            &[
                OrdinalSnapshot::for_ordinal(1),
                OrdinalSnapshot::for_ordinal(0),
            ],
            Path::new(MERGED_STAGING_RELATIVE_PATH),
        )
        .unwrap_err(),
        QpdfContractError::OrdinalOrder
    );
}

#[test]
fn qpdf_exit_code_contracts_are_exact() {
    assert_eq!(
        interpret_structural_check_exit(0).unwrap(),
        StructuralCheckOutcome::Valid
    );
    assert_eq!(
        interpret_structural_check_exit(2).unwrap(),
        StructuralCheckOutcome::Rejected
    );
    assert_eq!(
        interpret_structural_check_exit(3).unwrap(),
        StructuralCheckOutcome::Rejected
    );
    assert_eq!(
        interpret_structural_check_exit(1).unwrap_err(),
        QpdfContractError::UnexpectedExit
    );

    assert_eq!(
        interpret_encryption_check_exit(0).unwrap(),
        EncryptionCheckOutcome::Encrypted
    );
    assert_eq!(
        interpret_encryption_check_exit(2).unwrap(),
        EncryptionCheckOutcome::Unencrypted
    );
    assert_eq!(
        interpret_encryption_check_exit(3).unwrap_err(),
        QpdfContractError::UnexpectedExit
    );
}
