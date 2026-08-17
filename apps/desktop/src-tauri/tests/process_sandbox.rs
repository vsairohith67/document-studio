#[cfg(feature = "test-runtime")]
use document_studio_lib::process_sandbox::{
    delete_test_profile_if_present, ensure_test_profile, reset_test_profile, run_sandboxed,
    run_sandboxed_capture, spawn_sandboxed, SandboxError, SandboxLaunchSpec, CAPTURE_LIMIT_BYTES,
};
use document_studio_lib::process_sandbox::{
    validate_production_profile, validate_test_profile_for_cleanup, AppContainerProfileError,
    AppContainerProfileEvidence, APP_CONTAINER_CONFIGURATION_VERSION, QPDF_APP_CONTAINER_PROFILE,
    QPDF_TEST_APP_CONTAINER_PROFILE,
};
#[cfg(feature = "test-runtime")]
use document_studio_lib::qpdf::{
    build_production_merge_arguments, OrdinalSnapshot, MERGED_STAGING_RELATIVE_PATH,
};
#[cfg(feature = "test-runtime")]
use std::ffi::OsString;
#[cfg(feature = "test-runtime")]
use std::fs;
#[cfg(feature = "test-runtime")]
use std::net::TcpListener;
#[cfg(feature = "test-runtime")]
use std::path::Path;
#[cfg(feature = "test-runtime")]
use std::process::Command;
#[cfg(feature = "test-runtime")]
use std::sync::{Mutex, OnceLock};
#[cfg(feature = "test-runtime")]
use std::time::{Duration, Instant};

fn evidence(name: &str) -> AppContainerProfileEvidence {
    AppContainerProfileEvidence {
        name: name.to_owned(),
        sid: "S-1-15-2-expected".to_owned(),
        configuration_version: APP_CONTAINER_CONFIGURATION_VERSION,
        capabilities: Vec::new(),
    }
}

#[test]
fn production_profile_is_stable_idempotent_and_zero_capability() {
    validate_production_profile(&evidence(QPDF_APP_CONTAINER_PROFILE), "S-1-15-2-expected")
        .unwrap();

    let mut mismatch = evidence(QPDF_APP_CONTAINER_PROFILE);
    mismatch.configuration_version += 1;
    assert_eq!(
        validate_production_profile(&mismatch, "S-1-15-2-expected").unwrap_err(),
        AppContainerProfileError::ConfigurationMismatch
    );
    mismatch.configuration_version = APP_CONTAINER_CONFIGURATION_VERSION;
    mismatch.capabilities.push("internetClient".to_owned());
    assert_eq!(
        validate_production_profile(&mismatch, "S-1-15-2-expected").unwrap_err(),
        AppContainerProfileError::CapabilitiesPresent
    );
}

#[test]
fn cleanup_authorization_is_limited_to_the_fixed_test_profile_and_sid() {
    validate_test_profile_for_cleanup(
        &evidence(QPDF_TEST_APP_CONTAINER_PROFILE),
        "S-1-15-2-expected",
    )
    .unwrap();
    assert_eq!(
        validate_test_profile_for_cleanup(
            &evidence(QPDF_APP_CONTAINER_PROFILE),
            "S-1-15-2-expected",
        )
        .unwrap_err(),
        AppContainerProfileError::NameMismatch
    );
    assert_eq!(
        validate_test_profile_for_cleanup(
            &evidence(QPDF_TEST_APP_CONTAINER_PROFILE),
            "S-1-15-2-other",
        )
        .unwrap_err(),
        AppContainerProfileError::SidMismatch
    );
}

#[cfg(feature = "test-runtime")]
#[test]
fn qpdf_runs_in_fixed_zero_capability_profile_with_owned_limits() {
    static PROFILE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _lock = PROFILE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("sandbox profile test lock");
    let cleanup = TestProfileCleanup;
    let profile = reset_test_profile().expect("create the exact fixed test AppContainer profile");
    assert_eq!(profile.name(), QPDF_TEST_APP_CONTAINER_PROFILE);
    assert!(profile.sid_string().starts_with("S-1-15-2-"));
    let repeated_profile = ensure_test_profile().expect("reuse the exact fixed test profile");
    assert_eq!(repeated_profile.sid_string(), profile.sid_string());
    drop(repeated_profile);

    let allowed = tempfile::tempdir().expect("allowed sandbox directory");
    let denied = tempfile::tempdir().expect("denied sandbox directory");
    let engine = allowed.path().join("engine");
    let workspace = allowed.path().join("workspace");
    let temporary = workspace.join("temp");
    let inputs = workspace.join("inputs");
    let staging = workspace.join("staging");
    fs::create_dir_all(&engine).unwrap();
    fs::create_dir_all(&temporary).unwrap();
    fs::create_dir_all(&inputs).unwrap();
    fs::create_dir_all(&staging).unwrap();
    copy_flat_directory(&qpdf_bundle_bin(), &engine);

    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .expect("repository root");
    fs::copy(
        repository.join("report/Document_Studio_Master_Blueprint.pdf"),
        inputs.join("source-0000.pdf"),
    )
    .expect("copy first ordinal snapshot");
    fs::copy(
        repository.join("notion-import/attachments/archive/Legacy-Product-Specification.pdf"),
        inputs.join("source-0001.pdf"),
    )
    .expect("copy second ordinal snapshot");

    let probe = engine.join("sandbox-probe.exe");
    fs::copy(env!("CARGO_BIN_EXE_sandbox-probe"), &probe).expect("copy sandbox probe");
    let denied_file = denied.path().join("not-authorized.txt");
    fs::write(&denied_file, b"must remain outside the AppContainer ACL").unwrap();
    grant_app_container_modify(allowed.path(), profile.sid_string());

    let qpdf = engine.join("qpdf.exe");
    let qpdf_arguments = vec![OsString::from("--version")];
    let qpdf_spec = SandboxLaunchSpec {
        executable: &qpdf,
        arguments: &qpdf_arguments,
        working_directory: &workspace,
        temporary_directory: &temporary,
    };
    assert_eq!(
        run_sandboxed(&profile, &qpdf_spec, Duration::from_secs(10)).unwrap(),
        0,
        "the verified qpdf runtime must launch in the zero-capability AppContainer"
    );

    let merge_arguments = build_production_merge_arguments(
        &[
            OrdinalSnapshot::for_ordinal(0),
            OrdinalSnapshot::for_ordinal(1),
        ],
        Path::new(MERGED_STAGING_RELATIVE_PATH),
    )
    .unwrap();
    let merge_spec = SandboxLaunchSpec {
        executable: &qpdf,
        arguments: &merge_arguments,
        working_directory: &workspace,
        temporary_directory: &temporary,
    };
    assert_eq!(
        run_sandboxed(&profile, &merge_spec, Duration::from_secs(30)).unwrap(),
        0,
        "qpdf 12.3.2 must accept the exact production merge argument vector"
    );
    let merged = staging.join("merged.pdf");
    assert!(fs::metadata(&merged).unwrap().len() > 8);

    let structural_arguments = [
        OsString::from(MERGED_STAGING_RELATIVE_PATH),
        OsString::from("--suppress-recovery"),
        OsString::from("--check"),
    ];
    let structural_spec = SandboxLaunchSpec {
        executable: &qpdf,
        arguments: &structural_arguments,
        working_directory: &workspace,
        temporary_directory: &temporary,
    };
    assert_eq!(
        run_sandboxed(&profile, &structural_spec, Duration::from_secs(10)).unwrap(),
        0,
        "the production output must reopen under the strict structural check"
    );

    let encryption_arguments = [
        OsString::from(MERGED_STAGING_RELATIVE_PATH),
        OsString::from("--is-encrypted"),
    ];
    let encryption_spec = SandboxLaunchSpec {
        executable: &qpdf,
        arguments: &encryption_arguments,
        working_directory: &workspace,
        temporary_directory: &temporary,
    };
    assert_eq!(
        run_sandboxed(&profile, &encryption_spec, Duration::from_secs(10)).unwrap(),
        2,
        "qpdf exit 2 must identify the merged output as unencrypted"
    );

    let allowed_file = workspace.join("sandbox-write-proof.txt");
    let filesystem_arguments = vec![
        OsString::from("filesystem"),
        allowed_file.as_os_str().to_owned(),
        denied_file.as_os_str().to_owned(),
    ];
    let filesystem_spec = SandboxLaunchSpec {
        executable: &probe,
        arguments: &filesystem_arguments,
        working_directory: &workspace,
        temporary_directory: &temporary,
    };
    assert_eq!(
        run_sandboxed(&profile, &filesystem_spec, Duration::from_secs(10)).unwrap(),
        0,
        "the AppContainer must write only inside the granted owned workspace"
    );
    assert_eq!(fs::read(&allowed_file).unwrap(), b"sandbox write proof");

    let listener = TcpListener::bind("127.0.0.1:0").expect("local network-denial listener");
    let network_arguments = vec![
        OsString::from("network"),
        OsString::from(listener.local_addr().unwrap().to_string()),
    ];
    let network_spec = SandboxLaunchSpec {
        executable: &probe,
        arguments: &network_arguments,
        working_directory: &workspace,
        temporary_directory: &temporary,
    };
    assert_eq!(
        run_sandboxed(&profile, &network_spec, Duration::from_secs(10)).unwrap(),
        0,
        "zero-capability AppContainer must not connect to loopback"
    );

    let child_arguments = vec![OsString::from("spawn-child")];
    let child_spec = SandboxLaunchSpec {
        executable: &probe,
        arguments: &child_arguments,
        working_directory: &workspace,
        temporary_directory: &temporary,
    };
    assert_eq!(
        run_sandboxed(&profile, &child_spec, Duration::from_secs(10)).unwrap(),
        0,
        "the one-process Job Object limit must reject a child process"
    );

    let flood_arguments = vec![OsString::from("flood")];
    let flood_spec = SandboxLaunchSpec {
        executable: &probe,
        arguments: &flood_arguments,
        working_directory: &workspace,
        temporary_directory: &temporary,
    };
    let flooded = run_sandboxed_capture(&profile, &flood_spec, Duration::from_secs(10)).unwrap();
    assert_eq!(flooded.exit_code, 0);
    assert_eq!(flooded.stdout.len(), CAPTURE_LIMIT_BYTES);
    assert_eq!(flooded.stderr.len(), CAPTURE_LIMIT_BYTES);

    let timeout_arguments = vec![OsString::from("wait")];
    let timeout_spec = SandboxLaunchSpec {
        executable: &probe,
        arguments: &timeout_arguments,
        working_directory: &workspace,
        temporary_directory: &temporary,
    };
    let timeout_started = Instant::now();
    assert!(matches!(
        run_sandboxed(&profile, &timeout_spec, Duration::from_millis(100)),
        Err(SandboxError::Timeout)
    ));
    assert!(timeout_started.elapsed() <= Duration::from_secs(2));

    let wait_arguments = vec![OsString::from("wait")];
    let wait_spec = SandboxLaunchSpec {
        executable: &probe,
        arguments: &wait_arguments,
        working_directory: &workspace,
        temporary_directory: &temporary,
    };
    let mut owned = spawn_sandboxed(&profile, &wait_spec).unwrap();
    let mut unrelated = Command::new(&probe)
        .arg("wait")
        .spawn()
        .expect("spawn an unrelated test process");
    owned.resume().unwrap();
    let started = Instant::now();
    owned.terminate_owned().unwrap();
    assert!(
        started.elapsed() <= Duration::from_secs(2),
        "owned Job Object termination exceeded two seconds"
    );
    assert!(
        unrelated.try_wait().unwrap().is_none(),
        "terminating the owned Job Object must not terminate an unrelated process"
    );
    unrelated
        .kill()
        .expect("terminate only the owned unrelated test process");
    unrelated.wait().expect("reap the unrelated test process");

    drop(listener);
    drop(owned);
    drop(profile);
    drop(cleanup);
}

#[cfg(feature = "test-runtime")]
fn qpdf_bundle_bin() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("qpdf")
        .join("12.3.2")
        .join("bin")
}

#[cfg(feature = "test-runtime")]
fn copy_flat_directory(source: &Path, destination: &Path) {
    for entry in fs::read_dir(source).expect("read verified qpdf bundle") {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            fs::copy(entry.path(), destination.join(entry.file_name())).unwrap();
        }
    }
}

#[cfg(feature = "test-runtime")]
fn grant_app_container_modify(path: &Path, sid: &str) {
    let grant = format!("*{sid}:(OI)(CI)M");
    let output = Command::new("icacls.exe")
        .arg(path)
        .args(["/grant", &grant, "/T", "/C", "/Q"])
        .output()
        .expect("run icacls directly");
    assert!(
        output.status.success(),
        "icacls failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(feature = "test-runtime")]
struct TestProfileCleanup;

#[cfg(feature = "test-runtime")]
impl Drop for TestProfileCleanup {
    fn drop(&mut self) {
        delete_test_profile_if_present().expect("delete only the fixed validated test profile");
    }
}
