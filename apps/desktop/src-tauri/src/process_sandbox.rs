use std::ffi::{OsStr, OsString};
use std::io::{self, Read};
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use thiserror::Error;
use windows_sys::Win32::Foundation::{
    CloseHandle, LocalFree, SetHandleInformation, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND,
    ERROR_INSUFFICIENT_BUFFER, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::{
    BuildTrusteeWithSidW, ConvertSidToStringSidW, GetNamedSecurityInfoW, SetEntriesInAclW,
    SetNamedSecurityInfoW, EXPLICIT_ACCESS_W, GRANT_ACCESS, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeleteAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
};
use windows_sys::Win32::Security::{
    EqualSid, FreeSid, GetTokenInformation, TokenAppContainerSid, TokenCapabilities,
    TokenIsAppContainer, DACL_SECURITY_INFORMATION, PSID, SECURITY_ATTRIBUTES,
    SECURITY_CAPABILITIES, SUB_CONTAINERS_AND_OBJECTS_INHERIT, TOKEN_APPCONTAINER_INFORMATION,
    TOKEN_GROUPS, TOKEN_QUERY,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_DELETE_CHILD, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, OpenProcessToken, ResumeThread, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED,
    CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

pub const APP_CONTAINER_CONFIGURATION_VERSION: u32 = 1;
pub const QPDF_APP_CONTAINER_PROFILE: &str = "DocumentStudio.PdfEngine.Qpdf.V1";
pub const QPDF_TEST_APP_CONTAINER_PROFILE: &str = "DocumentStudio.PdfEngine.Qpdf.V1.Test";
pub const QPDF_PROCESS_MEMORY_LIMIT_BYTES: usize = 2 * 1024 * 1024 * 1024;
pub const QPDF_PROCESS_LIMIT: u32 = 1;
pub const QPDF_PROCESS_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const OWNED_PROCESS_TERMINATION_CODE: u32 = 0xD502_0001;
pub const CAPTURE_LIMIT_BYTES: usize = 64 * 1024;
pub const WINDOWS_CREATEPROCESS_COMMAND_LINE_LIMIT: usize = 32_767;
const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppContainerProfileEvidence {
    pub name: String,
    pub sid: String,
    pub configuration_version: u32,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AppContainerProfileError {
    #[error("the AppContainer profile name does not match")]
    NameMismatch,
    #[error("the AppContainer SID does not match the derived SID")]
    SidMismatch,
    #[error("the AppContainer configuration version does not match")]
    ConfigurationMismatch,
    #[error("the AppContainer profile must have zero capabilities")]
    CapabilitiesPresent,
}

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("the AppContainer profile operation failed with HRESULT {0:#010x}")]
    Profile(i32),
    #[error("the AppContainer profile evidence was invalid")]
    ProfileEvidence(#[from] AppContainerProfileError),
    #[error("the compiled qpdf AppContainer manifest is invalid")]
    ProfileManifest,
    #[error("the sandbox process operation failed")]
    Io(#[source] io::Error),
    #[error("the sandbox process operation {operation} failed")]
    Windows {
        operation: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("the process executable or argument contains an embedded NUL")]
    EmbeddedNul,
    #[error("the process command line exceeds the Windows CreateProcess limit")]
    CommandLineTooLong,
    #[error("the sandbox process token is not an AppContainer token")]
    NotAppContainer,
    #[error("the sandbox process AppContainer SID does not match the fixed profile")]
    TokenSidMismatch,
    #[error("the sandbox process token unexpectedly contains capabilities")]
    TokenCapabilitiesPresent,
    #[error("the sandbox process exceeded its wall-clock limit")]
    Timeout,
    #[error("the sandbox process was cancelled")]
    Cancelled,
    #[error("the sandbox output capture thread failed")]
    Capture,
    #[error("the sandbox process could not be resumed")]
    Resume,
}

#[derive(Debug)]
pub struct FixedAppContainerProfile {
    name: &'static str,
    sid: PSID,
    sid_string: String,
}

impl FixedAppContainerProfile {
    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn sid_string(&self) -> &str {
        &self.sid_string
    }

    fn evidence(&self) -> AppContainerProfileEvidence {
        AppContainerProfileEvidence {
            name: self.name.to_owned(),
            sid: self.sid_string.clone(),
            configuration_version: APP_CONTAINER_CONFIGURATION_VERSION,
            capabilities: Vec::new(),
        }
    }
}

impl Drop for FixedAppContainerProfile {
    fn drop(&mut self) {
        if !self.sid.is_null() {
            // SAFETY: the SID is returned by the AppContainer APIs and is owned here.
            unsafe {
                FreeSid(self.sid);
            }
            self.sid = null_mut();
        }
    }
}

pub fn validate_production_profile(
    evidence: &AppContainerProfileEvidence,
    derived_sid: &str,
) -> Result<(), AppContainerProfileError> {
    validate_profile(evidence, QPDF_APP_CONTAINER_PROFILE, derived_sid)
}

pub fn validate_test_profile_for_cleanup(
    evidence: &AppContainerProfileEvidence,
    derived_sid: &str,
) -> Result<(), AppContainerProfileError> {
    validate_profile(evidence, QPDF_TEST_APP_CONTAINER_PROFILE, derived_sid)
}

fn validate_profile(
    evidence: &AppContainerProfileEvidence,
    expected_name: &str,
    derived_sid: &str,
) -> Result<(), AppContainerProfileError> {
    if evidence.name != expected_name {
        return Err(AppContainerProfileError::NameMismatch);
    }
    if evidence.sid != derived_sid {
        return Err(AppContainerProfileError::SidMismatch);
    }
    if evidence.configuration_version != APP_CONTAINER_CONFIGURATION_VERSION {
        return Err(AppContainerProfileError::ConfigurationMismatch);
    }
    if !evidence.capabilities.is_empty() {
        return Err(AppContainerProfileError::CapabilitiesPresent);
    }
    Ok(())
}

pub fn ensure_production_profile() -> Result<FixedAppContainerProfile, SandboxError> {
    let _lifecycle = profile_lifecycle_lock();
    let profile = ensure_fixed_profile(QPDF_APP_CONTAINER_PROFILE)?;
    let evidence = compiled_production_profile_evidence(profile.sid_string())?;
    validate_production_profile(&evidence, profile.sid_string())?;
    Ok(profile)
}

pub fn authorize_qpdf_paths(
    profile: &FixedAppContainerProfile,
    runtime_bin: &Path,
    workspace: &crate::workspace::JobWorkspace,
) -> Result<(), SandboxError> {
    if !runtime_bin.is_dir()
        || !workspace.root.is_dir()
        || !workspace.inputs.is_dir()
        || !workspace.staging.is_dir()
        || !workspace.temporary.is_dir()
    {
        return Err(SandboxError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            "qpdf runtime or owned workspace is missing",
        )));
    }
    grant_path_access(
        runtime_bin,
        profile.sid,
        FILE_GENERIC_READ | FILE_GENERIC_EXECUTE,
        SUB_CONTAINERS_AND_OBJECTS_INHERIT,
    )?;
    grant_path_access(
        &workspace.root,
        profile.sid,
        FILE_GENERIC_READ | FILE_GENERIC_EXECUTE,
        0,
    )?;
    grant_path_access(
        &workspace.inputs,
        profile.sid,
        FILE_GENERIC_READ | FILE_GENERIC_EXECUTE,
        SUB_CONTAINERS_AND_OBJECTS_INHERIT,
    )?;
    let mutable_access =
        FILE_GENERIC_READ | FILE_GENERIC_WRITE | FILE_GENERIC_EXECUTE | FILE_DELETE_CHILD | DELETE;
    for path in [&workspace.staging, &workspace.temporary] {
        grant_path_access(
            path,
            profile.sid,
            mutable_access,
            SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        )?;
    }
    Ok(())
}

fn grant_path_access(
    path: &Path,
    sid: PSID,
    permissions: u32,
    inheritance: u32,
) -> Result<(), SandboxError> {
    let mut path_wide = wide_z(path.as_os_str())?;
    let mut old_acl = null_mut();
    let mut security_descriptor = null_mut();
    // SAFETY: path is terminated and all requested outputs are writable.
    let result = unsafe {
        GetNamedSecurityInfoW(
            path_wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut old_acl,
            null_mut(),
            &mut security_descriptor,
        )
    };
    if result != 0 {
        return Err(SandboxError::Io(io::Error::from_raw_os_error(
            result as i32,
        )));
    }
    let descriptor = LocalAllocation(security_descriptor.cast());
    let mut access = EXPLICIT_ACCESS_W {
        grfAccessPermissions: permissions,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: inheritance,
        ..Default::default()
    };
    // SAFETY: the profile SID remains live and uniquely identifies the trustee.
    unsafe {
        BuildTrusteeWithSidW(&mut access.Trustee, sid);
    }
    let mut new_acl = null_mut();
    // SAFETY: access and old ACL remain live through the call.
    let result = unsafe { SetEntriesInAclW(1, &access, old_acl, &mut new_acl) };
    if result != 0 {
        return Err(SandboxError::Io(io::Error::from_raw_os_error(
            result as i32,
        )));
    }
    let new_acl = LocalAllocation(new_acl.cast());
    // SAFETY: path and ACL remain live and only this path's DACL is updated.
    let result = unsafe {
        SetNamedSecurityInfoW(
            path_wide.as_mut_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            new_acl.0.cast(),
            null_mut(),
        )
    };
    drop(descriptor);
    if result != 0 {
        return Err(SandboxError::Io(io::Error::from_raw_os_error(
            result as i32,
        )));
    }
    Ok(())
}

fn compiled_production_profile_evidence(
    derived_sid: &str,
) -> Result<AppContainerProfileEvidence, SandboxError> {
    let manifest: serde_json::Value = serde_json::from_str(crate::qpdf::QPDF_BUNDLE_MANIFEST_JSON)
        .map_err(|_| SandboxError::ProfileManifest)?;
    let name = manifest["appContainerProfile"]
        .as_str()
        .ok_or(SandboxError::ProfileManifest)?;
    let configuration_version = manifest["appContainerConfigurationVersion"]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(SandboxError::ProfileManifest)?;
    let capabilities = manifest["appContainerCapabilities"]
        .as_array()
        .ok_or(SandboxError::ProfileManifest)?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or(SandboxError::ProfileManifest)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AppContainerProfileEvidence {
        name: name.to_owned(),
        sid: derived_sid.to_owned(),
        configuration_version,
        capabilities,
    })
}

#[doc(hidden)]
pub fn reset_test_profile() -> Result<FixedAppContainerProfile, SandboxError> {
    let _lifecycle = profile_lifecycle_lock();
    delete_test_profile_if_present_locked()?;
    let profile = create_fixed_profile(QPDF_TEST_APP_CONTAINER_PROFILE)?;
    validate_test_profile_for_cleanup(&profile.evidence(), profile.sid_string())?;
    Ok(profile)
}

#[doc(hidden)]
pub fn ensure_test_profile() -> Result<FixedAppContainerProfile, SandboxError> {
    let _lifecycle = profile_lifecycle_lock();
    let profile = ensure_fixed_profile(QPDF_TEST_APP_CONTAINER_PROFILE)?;
    validate_test_profile_for_cleanup(&profile.evidence(), profile.sid_string())?;
    Ok(profile)
}

#[doc(hidden)]
pub fn delete_test_profile_if_present() -> Result<(), SandboxError> {
    let _lifecycle = profile_lifecycle_lock();
    delete_test_profile_if_present_locked()
}

fn delete_test_profile_if_present_locked() -> Result<(), SandboxError> {
    let Some(profile) = derive_fixed_profile_if_present(QPDF_TEST_APP_CONTAINER_PROFILE)? else {
        return Ok(());
    };
    validate_test_profile_for_cleanup(&profile.evidence(), profile.sid_string())?;
    let name = wide_z(OsStr::new(QPDF_TEST_APP_CONTAINER_PROFILE))?;
    // SAFETY: name is a valid fixed profile name and the derived SID was validated.
    check_hresult(unsafe { DeleteAppContainerProfile(name.as_ptr()) })
}

fn profile_lifecycle_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("AppContainer profile lifecycle mutex poisoned")
}

fn ensure_fixed_profile(name: &'static str) -> Result<FixedAppContainerProfile, SandboxError> {
    match create_fixed_profile(name) {
        Ok(profile) => Ok(profile),
        Err(SandboxError::Profile(result))
            if result == hresult_from_win32(ERROR_ALREADY_EXISTS) =>
        {
            derive_fixed_profile_if_present(name)?.ok_or(SandboxError::Profile(result))
        }
        Err(error) => Err(error),
    }
}

fn create_fixed_profile(name: &'static str) -> Result<FixedAppContainerProfile, SandboxError> {
    let name_wide = wide_z(OsStr::new(name))?;
    let display_wide = wide_z(OsStr::new("Document Studio qpdf sandbox"))?;
    let description_wide = wide_z(OsStr::new("Document Studio local PDF engine"))?;
    let mut sid = null_mut();
    // SAFETY: strings are terminated; a null list with count zero declares zero capabilities.
    let result = unsafe {
        CreateAppContainerProfile(
            name_wide.as_ptr(),
            display_wide.as_ptr(),
            description_wide.as_ptr(),
            null(),
            0,
            &mut sid,
        )
    };
    if result < 0 {
        if !sid.is_null() {
            // SAFETY: defensive cleanup for any SID returned alongside an error.
            unsafe {
                FreeSid(sid);
            }
        }
        return Err(SandboxError::Profile(result));
    }
    profile_from_sid(name, sid)
}

fn derive_fixed_profile_if_present(
    name: &'static str,
) -> Result<Option<FixedAppContainerProfile>, SandboxError> {
    let name_wide = wide_z(OsStr::new(name))?;
    let mut sid = null_mut();
    // SAFETY: name is terminated and sid is an output pointer.
    let result = unsafe { DeriveAppContainerSidFromAppContainerName(name_wide.as_ptr(), &mut sid) };
    if result == hresult_from_win32(ERROR_FILE_NOT_FOUND) {
        return Ok(None);
    }
    check_hresult(result)?;
    Ok(Some(profile_from_sid(name, sid)?))
}

fn profile_from_sid(
    name: &'static str,
    sid: PSID,
) -> Result<FixedAppContainerProfile, SandboxError> {
    if sid.is_null() {
        return Err(SandboxError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "AppContainer API returned a null SID",
        )));
    }
    Ok(FixedAppContainerProfile {
        name,
        sid,
        sid_string: sid_to_string(sid)?,
    })
}

fn sid_to_string(sid: PSID) -> Result<String, SandboxError> {
    let mut text = null_mut();
    // SAFETY: sid is valid and text receives memory owned by LocalAlloc.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(last_error());
    }
    // SAFETY: text is a NUL-terminated SID string returned by Windows.
    let result = unsafe {
        let mut length = 0;
        while *text.add(length) != 0 {
            length += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(text, length))
    };
    // SAFETY: text was allocated by ConvertSidToStringSidW.
    unsafe {
        LocalFree(text.cast());
    }
    Ok(result)
}

pub struct SandboxLaunchSpec<'a> {
    pub executable: &'a Path,
    pub arguments: &'a [OsString],
    pub working_directory: &'a Path,
    pub temporary_directory: &'a Path,
}

#[derive(Debug)]
pub struct OwnedSandboxProcess {
    process: HANDLE,
    thread: HANDLE,
    job: HANDLE,
    resumed: bool,
    stdout: Option<JoinHandle<Vec<u8>>>,
    stderr: Option<JoinHandle<Vec<u8>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxExecution {
    pub exit_code: u32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub peak_process_memory_bytes: usize,
}

impl OwnedSandboxProcess {
    pub fn resume(&mut self) -> Result<(), SandboxError> {
        if self.resumed {
            return Ok(());
        }
        // SAFETY: thread is the retained primary thread created suspended.
        if unsafe { ResumeThread(self.thread) } == u32::MAX {
            return Err(SandboxError::Resume);
        }
        self.resumed = true;
        Ok(())
    }

    pub fn wait(&mut self, timeout: Duration) -> Result<u32, SandboxError> {
        self.wait_with_cancellation(timeout, || false)
            .map(|result| result.exit_code)
    }

    pub fn wait_with_cancellation<C>(
        &mut self,
        timeout: Duration,
        mut is_cancelled: C,
    ) -> Result<SandboxExecution, SandboxError>
    where
        C: FnMut() -> bool,
    {
        self.resume()?;
        let started = Instant::now();
        loop {
            if is_cancelled() {
                self.terminate_owned()?;
                self.join_capture()?;
                return Err(SandboxError::Cancelled);
            }
            if started.elapsed() >= timeout {
                self.terminate_owned()?;
                self.join_capture()?;
                return Err(SandboxError::Timeout);
            }
            let remaining = timeout.saturating_sub(started.elapsed());
            let wait = remaining.min(CANCELLATION_POLL_INTERVAL);
            let wait_ms = wait.as_millis().max(1).min(u128::from(u32::MAX - 1)) as u32;
            // SAFETY: process is a retained process handle.
            match unsafe { WaitForSingleObject(self.process, wait_ms) } {
                WAIT_OBJECT_0 => {
                    let exit_code = self.exit_code()?;
                    let peak_process_memory_bytes = self.peak_process_memory_bytes()?;
                    let (stdout, stderr) = self.join_capture()?;
                    return Ok(SandboxExecution {
                        exit_code,
                        stdout,
                        stderr,
                        peak_process_memory_bytes,
                    });
                }
                WAIT_TIMEOUT => {}
                _ => return Err(last_error()),
            }
        }
    }

    pub fn terminate_owned(&mut self) -> Result<(), SandboxError> {
        // SAFETY: job is the unique retained Job Object containing only this owned process.
        if unsafe { TerminateJobObject(self.job, OWNED_PROCESS_TERMINATION_CODE) } == 0 {
            return Err(last_error());
        }
        // SAFETY: process is retained and termination is bounded to the owned job.
        if unsafe { WaitForSingleObject(self.process, 2_000) } != WAIT_OBJECT_0 {
            return Err(last_error());
        }
        Ok(())
    }

    fn exit_code(&self) -> Result<u32, SandboxError> {
        let mut exit_code = 0;
        // SAFETY: process is retained and exit_code is writable.
        if unsafe { GetExitCodeProcess(self.process, &mut exit_code) } == 0 {
            return Err(last_error());
        }
        Ok(exit_code)
    }

    fn peak_process_memory_bytes(&self) -> Result<usize, SandboxError> {
        let mut observed: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        // SAFETY: the retained Job Object is valid and observed has the declared layout.
        if unsafe {
            QueryInformationJobObject(
                self.job,
                JobObjectExtendedLimitInformation,
                (&mut observed as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                null_mut(),
            )
        } == 0
        {
            return Err(last_error());
        }
        Ok(observed.PeakProcessMemoryUsed)
    }

    fn join_capture(&mut self) -> Result<(Vec<u8>, Vec<u8>), SandboxError> {
        let stdout = self
            .stdout
            .take()
            .ok_or(SandboxError::Capture)?
            .join()
            .map_err(|_| SandboxError::Capture)?;
        let stderr = self
            .stderr
            .take()
            .ok_or(SandboxError::Capture)?
            .join()
            .map_err(|_| SandboxError::Capture)?;
        Ok((stdout, stderr))
    }
}

impl Drop for OwnedSandboxProcess {
    fn drop(&mut self) {
        if !self.job.is_null() {
            // SAFETY: this unique job is configured kill-on-close.
            unsafe {
                CloseHandle(self.job);
            }
        }
        if !self.thread.is_null() {
            // SAFETY: this is an owned thread handle.
            unsafe {
                CloseHandle(self.thread);
            }
        }
        if !self.process.is_null() {
            // SAFETY: this is an owned process handle.
            unsafe {
                CloseHandle(self.process);
            }
        }
    }
}

pub fn spawn_sandboxed(
    profile: &FixedAppContainerProfile,
    spec: &SandboxLaunchSpec<'_>,
) -> Result<OwnedSandboxProcess, SandboxError> {
    if !spec.executable.is_file()
        || !spec.working_directory.is_dir()
        || !spec.temporary_directory.is_dir()
    {
        return Err(SandboxError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            "sandbox executable or owned directory is missing",
        )));
    }

    let executable = wide_z(spec.executable.as_os_str())?;
    let working_directory = wide_z(spec.working_directory.as_os_str())?;
    let executable_name = spec
        .executable
        .file_name()
        .ok_or_else(|| SandboxError::Io(io::Error::other("sandbox executable has no file name")))?;
    let mut command_line = build_command_line(executable_name, spec.arguments)?;
    let environment = build_environment_block(spec.temporary_directory)?;
    let mut stdout_pipe = CapturePipe::new()?;
    let mut stderr_pipe = CapturePipe::new()?;

    let mut attribute_bytes = 0usize;
    // SAFETY: this sizing call intentionally supplies a null output buffer.
    unsafe {
        InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut attribute_bytes);
    }
    if attribute_bytes == 0 {
        return Err(last_error());
    }
    let mut attribute_storage = vec![0usize; attribute_bytes.div_ceil(size_of::<usize>())];
    let attribute_list = attribute_storage.as_mut_ptr().cast();
    // SAFETY: the allocation is aligned and sized from the API result.
    if unsafe { InitializeProcThreadAttributeList(attribute_list, 1, 0, &mut attribute_bytes) } == 0
    {
        return Err(last_error());
    }
    let attribute_guard = AttributeListGuard(attribute_list);
    let capabilities = SECURITY_CAPABILITIES {
        AppContainerSid: profile.sid,
        Capabilities: null_mut(),
        CapabilityCount: 0,
        Reserved: 0,
    };
    // SAFETY: list is initialized and the zero-capability value remains live through process creation.
    if unsafe {
        UpdateProcThreadAttribute(
            attribute_list,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            (&capabilities as *const SECURITY_CAPABILITIES).cast(),
            size_of::<SECURITY_CAPABILITIES>(),
            null_mut(),
            null(),
        )
    } == 0
    {
        return Err(last_error());
    }

    let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
    startup.StartupInfo.hStdOutput = stdout_pipe.write;
    startup.StartupInfo.hStdError = stderr_pipe.write;
    startup.lpAttributeList = attribute_list;
    let mut process: PROCESS_INFORMATION = unsafe { zeroed() };
    // SAFETY: all pointers remain valid for this call; application name is passed separately.
    let created = unsafe {
        CreateProcessW(
            executable.as_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_SUSPENDED
                | CREATE_UNICODE_ENVIRONMENT
                | CREATE_NO_WINDOW
                | EXTENDED_STARTUPINFO_PRESENT,
            environment.as_ptr().cast(),
            working_directory.as_ptr(),
            &startup.StartupInfo,
            &mut process,
        )
    };
    drop(attribute_guard);
    if created == 0 {
        return Err(SandboxError::Windows {
            operation: "CreateProcessW",
            source: io::Error::last_os_error(),
        });
    }
    stdout_pipe.close_write();
    stderr_pipe.close_write();
    let stdout = capture_bounded(stdout_pipe.take_read());
    let stderr = capture_bounded(stderr_pipe.take_read());

    let mut guard = UnassignedProcessGuard::from(process);
    let job = create_limited_job()?;
    // SAFETY: both handles are valid and the process remains suspended.
    if unsafe { AssignProcessToJobObject(job, process.hProcess) } == 0 {
        // SAFETY: job is owned and contains no process.
        unsafe {
            CloseHandle(job);
        }
        return Err(last_error());
    }
    guard.assigned_job = job;
    verify_process_token(process.hProcess, profile.sid)?;
    let process = guard.disarm();
    Ok(OwnedSandboxProcess {
        process: process.hProcess,
        thread: process.hThread,
        job,
        resumed: false,
        stdout: Some(stdout),
        stderr: Some(stderr),
    })
}

pub fn run_sandboxed(
    profile: &FixedAppContainerProfile,
    spec: &SandboxLaunchSpec<'_>,
    timeout: Duration,
) -> Result<u32, SandboxError> {
    run_sandboxed_capture(profile, spec, timeout).map(|result| result.exit_code)
}

pub fn run_sandboxed_capture(
    profile: &FixedAppContainerProfile,
    spec: &SandboxLaunchSpec<'_>,
    timeout: Duration,
) -> Result<SandboxExecution, SandboxError> {
    let mut process = spawn_sandboxed(profile, spec)?;
    process.wait_with_cancellation(timeout, || false)
}

struct CapturePipe {
    read: HANDLE,
    write: HANDLE,
}

impl CapturePipe {
    fn new() -> Result<Self, SandboxError> {
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        let mut read = null_mut();
        let mut write = null_mut();
        // SAFETY: both handles are writable outputs and attributes is initialized.
        if unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) } == 0 {
            return Err(last_error());
        }
        // SAFETY: the read side is retained only by the parent and must not be inherited.
        if unsafe { SetHandleInformation(read, HANDLE_FLAG_INHERIT, 0) } == 0 {
            // SAFETY: both handles were created by CreatePipe and are uniquely owned here.
            unsafe {
                CloseHandle(read);
                CloseHandle(write);
            }
            return Err(last_error());
        }
        Ok(Self { read, write })
    }

    fn close_write(&mut self) {
        if !self.write.is_null() {
            // SAFETY: the parent uniquely owns this write handle after process creation.
            unsafe {
                CloseHandle(self.write);
            }
            self.write = null_mut();
        }
    }

    fn take_read(&mut self) -> HANDLE {
        let read = self.read;
        self.read = null_mut();
        read
    }
}

impl Drop for CapturePipe {
    fn drop(&mut self) {
        for handle in [self.read, self.write] {
            if !handle.is_null() {
                // SAFETY: every non-null handle is uniquely owned by this pipe.
                unsafe {
                    CloseHandle(handle);
                }
            }
        }
    }
}

fn capture_bounded(handle: HANDLE) -> JoinHandle<Vec<u8>> {
    let handle = handle as usize;
    std::thread::spawn(move || {
        // SAFETY: ownership of the pipe read handle is transferred to this File.
        let mut pipe = unsafe { std::fs::File::from_raw_handle(handle as RawHandle) };
        let mut captured = Vec::with_capacity(CAPTURE_LIMIT_BYTES);
        let mut buffer = [0_u8; 8192];
        while let Ok(read) = pipe.read(&mut buffer) {
            if read == 0 {
                break;
            }
            if read >= CAPTURE_LIMIT_BYTES {
                captured.clear();
                captured.extend_from_slice(&buffer[read - CAPTURE_LIMIT_BYTES..read]);
                continue;
            }
            let overflow = captured
                .len()
                .saturating_add(read)
                .saturating_sub(CAPTURE_LIMIT_BYTES);
            if overflow > 0 {
                captured.drain(..overflow);
            }
            captured.extend_from_slice(&buffer[..read]);
        }
        captured
    })
}

fn create_limited_job() -> Result<HANDLE, SandboxError> {
    // SAFETY: null attributes and name create a private unnamed Job Object.
    let job = unsafe { CreateJobObjectW(null(), null()) };
    if job.is_null() {
        return Err(last_error());
    }
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
        | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
    limits.BasicLimitInformation.ActiveProcessLimit = QPDF_PROCESS_LIMIT;
    limits.ProcessMemoryLimit = QPDF_PROCESS_MEMORY_LIMIT_BYTES;
    // SAFETY: job is owned and limits is a complete structure.
    if unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    } == 0
    {
        let error = last_error();
        // SAFETY: job is owned and no process has been assigned.
        unsafe {
            CloseHandle(job);
        }
        return Err(error);
    }
    let mut observed: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    // SAFETY: job is valid and observed is a writable structure of the declared size.
    if unsafe {
        QueryInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&mut observed as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            null_mut(),
        )
    } == 0
    {
        let error = last_error();
        // SAFETY: job is owned and no process has been assigned.
        unsafe {
            CloseHandle(job);
        }
        return Err(error);
    }
    let required_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
        | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
    if observed.BasicLimitInformation.LimitFlags & required_flags != required_flags
        || observed.BasicLimitInformation.ActiveProcessLimit != QPDF_PROCESS_LIMIT
        || observed.ProcessMemoryLimit != QPDF_PROCESS_MEMORY_LIMIT_BYTES
    {
        // SAFETY: job is owned and no process has been assigned.
        unsafe {
            CloseHandle(job);
        }
        return Err(SandboxError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "Job Object limits did not match the fail-closed policy",
        )));
    }
    Ok(job)
}

fn verify_process_token(process: HANDLE, expected_sid: PSID) -> Result<(), SandboxError> {
    let mut token = null_mut();
    // SAFETY: process is valid and token is an output handle.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(last_error());
    }
    let token = HandleGuard(token);
    let is_app_container: u32 = token_information_value(token.0, TokenIsAppContainer)?;
    if is_app_container != 1 {
        return Err(SandboxError::NotAppContainer);
    }
    let app_container_storage = token_information_buffer(token.0, TokenAppContainerSid)?;
    let information = app_container_storage
        .as_ptr()
        .cast::<TOKEN_APPCONTAINER_INFORMATION>();
    // SAFETY: the complete token buffer remains live during the SID comparison.
    let token_sid = unsafe { (*information).TokenAppContainer };
    if token_sid.is_null()
        // SAFETY: both SIDs are valid for the duration of this comparison.
        || unsafe { EqualSid(token_sid, expected_sid) } == 0
    {
        return Err(SandboxError::TokenSidMismatch);
    }
    let capability_storage = token_information_buffer(token.0, TokenCapabilities)?;
    let groups = capability_storage.as_ptr().cast::<TOKEN_GROUPS>();
    // SAFETY: the buffer contains a complete TOKEN_GROUPS value.
    if unsafe { (*groups).GroupCount } != 0 {
        return Err(SandboxError::TokenCapabilitiesPresent);
    }
    Ok(())
}

fn token_information_value<T: Copy>(token: HANDLE, class: i32) -> Result<T, SandboxError> {
    let mut value: T = unsafe { zeroed() };
    let mut returned = 0;
    // SAFETY: value is writable and sized for this fixed token information class.
    if unsafe {
        GetTokenInformation(
            token,
            class,
            (&mut value as *mut T).cast(),
            size_of::<T>() as u32,
            &mut returned,
        )
    } == 0
    {
        return Err(last_error());
    }
    if returned < size_of::<T>() as u32 {
        return Err(SandboxError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "token information was truncated",
        )));
    }
    Ok(value)
}

fn token_information_buffer(token: HANDLE, class: i32) -> Result<Vec<usize>, SandboxError> {
    let mut required = 0;
    // SAFETY: this sizing call intentionally supplies a null buffer.
    let first = unsafe { GetTokenInformation(token, class, null_mut(), 0, &mut required) };
    let sizing_error = io::Error::last_os_error();
    if first != 0
        || required == 0
        || sizing_error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32)
    {
        return Err(SandboxError::Io(sizing_error));
    }
    let mut storage = vec![0usize; (required as usize).div_ceil(size_of::<usize>())];
    let mut returned = 0;
    // SAFETY: storage is aligned and at least required bytes long.
    if unsafe {
        GetTokenInformation(
            token,
            class,
            storage.as_mut_ptr().cast(),
            required,
            &mut returned,
        )
    } == 0
    {
        return Err(last_error());
    }
    if returned > required {
        return Err(SandboxError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "token information exceeded its allocated buffer",
        )));
    }
    Ok(storage)
}

fn build_environment_block(temporary_directory: &Path) -> Result<Vec<u16>, SandboxError> {
    let mut values = Vec::<(OsString, OsString)>::new();
    for key in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(key) {
            values.push((OsString::from(key), value));
        }
    }
    for key in ["LOCALAPPDATA", "TEMP", "TMP"] {
        values.push((
            OsString::from(key),
            temporary_directory.as_os_str().to_owned(),
        ));
    }
    values.sort_by_key(|(key, _)| key.to_string_lossy().to_ascii_uppercase());

    let mut block = Vec::new();
    for (key, value) in values {
        let mut entry: Vec<u16> = key.encode_wide().collect();
        entry.push('=' as u16);
        entry.extend(value.encode_wide());
        if entry.contains(&0) {
            return Err(SandboxError::EmbeddedNul);
        }
        block.extend(entry);
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

fn build_command_line(
    executable: &OsStr,
    arguments: &[OsString],
) -> Result<Vec<u16>, SandboxError> {
    let mut command_line = Vec::new();
    append_quoted_argument(&mut command_line, executable)?;
    for argument in arguments {
        command_line.push(' ' as u16);
        append_quoted_argument(&mut command_line, argument)?;
    }
    command_line.push(0);
    if command_line.len() >= WINDOWS_CREATEPROCESS_COMMAND_LINE_LIMIT {
        return Err(SandboxError::CommandLineTooLong);
    }
    Ok(command_line)
}

pub fn validate_command_line_budget(
    executable: &OsStr,
    arguments: &[OsString],
) -> Result<usize, SandboxError> {
    build_command_line(executable, arguments).map(|command_line| command_line.len())
}

fn append_quoted_argument(output: &mut Vec<u16>, argument: &OsStr) -> Result<(), SandboxError> {
    let units: Vec<u16> = argument.encode_wide().collect();
    if units.contains(&0) {
        return Err(SandboxError::EmbeddedNul);
    }
    let quote = units.is_empty()
        || units
            .iter()
            .any(|unit| *unit == ' ' as u16 || *unit == '\t' as u16 || *unit == '"' as u16);
    if !quote {
        output.extend(units);
        return Ok(());
    }

    output.push('"' as u16);
    let mut backslashes = 0usize;
    for unit in units {
        if unit == '\\' as u16 {
            backslashes += 1;
            continue;
        }
        if unit == '"' as u16 {
            output.extend(std::iter::repeat_n('\\' as u16, backslashes * 2 + 1));
        } else {
            output.extend(std::iter::repeat_n('\\' as u16, backslashes));
        }
        output.push(unit);
        backslashes = 0;
    }
    output.extend(std::iter::repeat_n('\\' as u16, backslashes * 2));
    output.push('"' as u16);
    Ok(())
}

fn wide_z(value: &OsStr) -> Result<Vec<u16>, SandboxError> {
    let mut wide: Vec<u16> = value.encode_wide().collect();
    if wide.contains(&0) {
        return Err(SandboxError::EmbeddedNul);
    }
    wide.push(0);
    Ok(wide)
}

fn check_hresult(result: i32) -> Result<(), SandboxError> {
    if result < 0 {
        Err(SandboxError::Profile(result))
    } else {
        Ok(())
    }
}

fn hresult_from_win32(error: u32) -> i32 {
    ((error & 0xFFFF) | 0x8007_0000) as i32
}

fn last_error() -> SandboxError {
    SandboxError::Io(io::Error::last_os_error())
}

struct AttributeListGuard(*mut std::ffi::c_void);

impl Drop for AttributeListGuard {
    fn drop(&mut self) {
        // SAFETY: pointer was initialized by InitializeProcThreadAttributeList.
        unsafe {
            DeleteProcThreadAttributeList(self.0);
        }
    }
}

struct HandleGuard(HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this guard uniquely owns the handle.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

struct LocalAllocation(*mut std::ffi::c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: allocation was returned by a Windows local-allocation API.
            unsafe {
                LocalFree(self.0);
            }
        }
    }
}

struct UnassignedProcessGuard {
    information: PROCESS_INFORMATION,
    assigned_job: HANDLE,
    armed: bool,
}

impl UnassignedProcessGuard {
    fn disarm(mut self) -> PROCESS_INFORMATION {
        self.armed = false;
        self.information
    }
}

impl From<PROCESS_INFORMATION> for UnassignedProcessGuard {
    fn from(information: PROCESS_INFORMATION) -> Self {
        Self {
            information,
            assigned_job: null_mut(),
            armed: true,
        }
    }
}

impl Drop for UnassignedProcessGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if !self.assigned_job.is_null() {
            // SAFETY: job uniquely owns the suspended process and is kill-on-close.
            unsafe {
                TerminateJobObject(self.assigned_job, OWNED_PROCESS_TERMINATION_CODE);
                CloseHandle(self.assigned_job);
            }
        } else if !self.information.hProcess.is_null() {
            // SAFETY: suspended process is unassigned and uniquely owned here.
            unsafe {
                TerminateProcess(self.information.hProcess, OWNED_PROCESS_TERMINATION_CODE);
            }
        }
        if !self.information.hThread.is_null() {
            // SAFETY: the thread handle is uniquely owned here.
            unsafe {
                CloseHandle(self.information.hThread);
            }
        }
        if !self.information.hProcess.is_null() {
            // SAFETY: the process handle is uniquely owned here.
            unsafe {
                CloseHandle(self.information.hProcess);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_quoting_handles_spaces_quotes_and_backslashes() {
        let args = [
            OsString::from("plain"),
            OsString::from("has space"),
            OsString::from("quote\"inside"),
        ];
        let command =
            build_command_line(OsStr::new("C:\\Program Files\\probe.exe"), &args).unwrap();
        let text = String::from_utf16_lossy(&command[..command.len() - 1]);
        assert_eq!(
            text,
            "\"C:\\Program Files\\probe.exe\" plain \"has space\" \"quote\\\"inside\""
        );
    }

    #[test]
    fn command_line_budget_accepts_near_limit_and_rejects_limit_or_larger() {
        let executable = OsStr::new("qpdf.exe");
        let fixed = validate_command_line_budget(executable, &[]).unwrap();
        let accepted =
            OsString::from("a".repeat(WINDOWS_CREATEPROCESS_COMMAND_LINE_LIMIT - fixed - 2));
        assert!(validate_command_line_budget(executable, &[accepted]).is_ok());
        let rejected =
            OsString::from("a".repeat(WINDOWS_CREATEPROCESS_COMMAND_LINE_LIMIT - fixed - 1));
        assert!(matches!(
            validate_command_line_budget(executable, &[rejected]),
            Err(SandboxError::CommandLineTooLong)
        ));
    }

    #[test]
    fn environment_is_an_explicit_allow_list() {
        let block = build_environment_block(Path::new("C:\\owned temp")).unwrap();
        let text = String::from_utf16_lossy(&block);
        assert!(text.contains("TEMP=C:\\owned temp\0"));
        assert!(text.contains("TMP=C:\\owned temp\0"));
        assert!(text.contains("LOCALAPPDATA=C:\\owned temp\0"));
        assert!(!text.contains("USERPROFILE="));
        assert!(!text.contains("HTTP_PROXY="));
        assert!(block.ends_with(&[0, 0]));
    }
}
