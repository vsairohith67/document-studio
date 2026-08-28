use std::cell::RefCell;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant};

use thiserror::Error;
use uuid::Uuid;
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    take_pwstr, AcceleratorKeyPressedEventHandler, BasicAuthenticationRequestedEventHandler,
    ClientCertificateRequestedEventHandler, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    DOMContentLoadedEventHandler, DownloadStartingEventHandler, NavigationCompletedEventHandler,
    NavigationStartingEventHandler, NewWindowRequestedEventHandler,
    PermissionRequestedEventHandler, PrintToPdfCompletedHandler, ProcessFailedEventHandler,
    ServerCertificateErrorDetectedEventHandler, WebResourceRequestedEventHandler,
    WebResourceResponseReceivedEventHandler,
};
use windows::core::{Interface, HSTRING, PCWSTR};
use windows::Win32::Foundation::{E_POINTER, HWND, RECT};
use windows::Win32::System::Com::{
    CoInitializeEx, CoUninitialize, IStream, COINIT_APARTMENTTHREADED, STATFLAG_NONAME, STATSTG,
};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW,
    RegisterClassExW, TranslateMessage, UnregisterClassW, MSG, PM_REMOVE, WNDCLASSEXW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};

use crate::app_state::CancellationToken;
use crate::contracts::{OperationError, OperationStage};
use crate::text_to_pdf::{
    AdmittedScript, TextToPdfSettings, CONTENT_SECURITY_POLICY, CSS_URL, DOCUMENT_URL,
    NOTO_DEVANAGARI_BYTES, NOTO_DEVANAGARI_URL, NOTO_SANS_BYTES, NOTO_SANS_URL, NOTO_TELUGU_BYTES,
    NOTO_TELUGU_URL, TXT_MAX_SERVED_BYTES,
};

const CREATION_TIMEOUT: Duration = Duration::from_secs(30);
const READINESS_TIMEOUT: Duration = Duration::from_secs(30);
const PRINT_TIMEOUT: Duration = Duration::from_secs(180);
const PUMP_INTERVAL: Duration = Duration::from_millis(5);
const MAX_RETAINED_RESPONSES: usize = 8;
const RENDERER_BROWSER_ARGUMENTS: &str = "--no-proxy-server --disable-background-networking --disable-component-update --disable-sync --disable-features=msSmartScreenProtection,OptimizationHints,MediaRouter,AutofillServerCommunication";

static RENDERER_MANAGER: OnceLock<Mutex<()>> = OnceLock::new();
type EventRegistrationToken = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RendererCheckpoint {
    StaInitialized,
    #[cfg(test)]
    EnvironmentCallback,
    EnvironmentCreated,
    #[cfg(test)]
    ControllerCallback,
    ControllerCreated,
    BoundaryInstalled,
    NavigationStarted,
    ReadinessCompleted,
    PrintStarted,
    #[cfg(test)]
    PrintCallback,
    PrintCompleted,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RendererTestAction {
    Fail,
    Timeout,
    Crash,
    Cancel,
    WithholdCallback,
    FailCallback,
}

#[cfg(test)]
static RENDERER_TEST_FAULT: OnceLock<Mutex<Option<(RendererCheckpoint, RendererTestAction)>>> =
    OnceLock::new();

#[derive(Debug, Clone)]
pub struct TextRenderRequest {
    pub job_id: String,
    pub renderer_generation: String,
    pub lifecycle_version: u64,
    pub user_data_directory: PathBuf,
    pub raw_pdf_path: PathBuf,
    pub html: Arc<[u8]>,
    pub css: Arc<[u8]>,
    pub used_scripts: BTreeSet<AdmittedScript>,
    pub settings: TextToPdfSettings,
    pub operation_deadline: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRenderEvidence {
    pub renderer_generation: String,
    pub runtime_version: String,
    pub served_urls: Vec<String>,
    pub denied_requests: u32,
}

#[derive(Debug, Error)]
enum RendererFailure {
    #[error("cancelled")]
    Cancelled,
    #[error("timed out")]
    Timeout,
    #[error("native renderer failed")]
    Native,
    #[error("renderer resource boundary failed")]
    Resource,
}

impl From<windows::core::Error> for RendererFailure {
    fn from(_: windows::core::Error) -> Self {
        Self::Native
    }
}

#[derive(Debug)]
struct CallbackAuthority {
    job_id: String,
    generation: String,
    completion_token: String,
    lifecycle_version: u64,
    active: AtomicBool,
    completed: AtomicBool,
    cancellation: CancellationToken,
}

impl CallbackAuthority {
    fn owns_generation(&self, expected_job: &str, expected_generation: &str, version: u64) -> bool {
        self.active.load(Ordering::Acquire)
            && self.job_id == expected_job
            && self.generation == expected_generation
            && self.lifecycle_version == version
            && !self.completion_token.is_empty()
    }

    fn is_current(&self, expected_job: &str, expected_generation: &str, version: u64) -> bool {
        self.owns_generation(expected_job, expected_generation, version)
            && !self.cancellation.is_cancelled()
    }

    fn complete_once(&self) -> bool {
        self.completed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    fn invalidate(&self) {
        self.active.store(false, Ordering::Release);
    }
}

#[derive(Clone)]
struct CallbackGuard {
    job_id: String,
    generation: String,
    completion_token: String,
    lifecycle_version: u64,
    renderer: Weak<CallbackAuthority>,
}

impl CallbackGuard {
    fn new(authority: &Arc<CallbackAuthority>) -> Self {
        Self {
            job_id: authority.job_id.clone(),
            generation: authority.generation.clone(),
            completion_token: authority.completion_token.clone(),
            lifecycle_version: authority.lifecycle_version,
            renderer: Arc::downgrade(authority),
        }
    }

    fn owns_current_generation(&self) -> bool {
        self.renderer.upgrade().is_some_and(|authority| {
            authority.is_current(&self.job_id, &self.generation, self.lifecycle_version)
                && authority.completion_token == self.completion_token
        })
    }
}

#[cfg(test)]
fn test_checkpoint(
    authority: &Arc<CallbackAuthority>,
    checkpoint: RendererCheckpoint,
) -> Result<(), RendererFailure> {
    let fault = *RENDERER_TEST_FAULT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| RendererFailure::Native)?;
    let Some((expected, action)) = fault else {
        return Ok(());
    };
    if expected != checkpoint {
        return Ok(());
    }
    match action {
        RendererTestAction::Fail => Err(RendererFailure::Native),
        RendererTestAction::Timeout => Err(RendererFailure::Timeout),
        RendererTestAction::Crash => panic!("injected renderer crash at {checkpoint:?}"),
        RendererTestAction::Cancel => {
            authority.cancellation.request_for_test();
            authority.invalidate();
            Err(RendererFailure::Cancelled)
        }
        RendererTestAction::WithholdCallback | RendererTestAction::FailCallback => Ok(()),
    }
}

#[cfg(not(test))]
fn test_checkpoint(
    _authority: &Arc<CallbackAuthority>,
    _checkpoint: RendererCheckpoint,
) -> Result<(), RendererFailure> {
    Ok(())
}

#[cfg(test)]
fn test_callback_action(checkpoint: RendererCheckpoint) -> Option<RendererTestAction> {
    RENDERER_TEST_FAULT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|fault| {
            fault.and_then(|(expected, action)| (expected == checkpoint).then_some(action))
        })
}

pub fn render_text_pdf(
    request: TextRenderRequest,
    cancellation: CancellationToken,
) -> Result<TextRenderEvidence, OperationError> {
    let started = Instant::now();
    let manager = RENDERER_MANAGER.get_or_init(|| Mutex::new(()));
    let _lease = loop {
        if cancellation.is_cancelled() {
            return Err(cancelled_error());
        }
        if Instant::now() >= request.operation_deadline {
            return Err(renderer_error("TXT_OPERATION_TIMEOUT"));
        }
        match manager.try_lock() {
            Ok(lease) => break lease,
            Err(std::sync::TryLockError::WouldBlock) if started.elapsed() < CREATION_TIMEOUT => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(_) => return Err(renderer_error("TXT_RENDERER_BUSY")),
        }
    };
    if !crate::webview2_environment::webview2_environment_policy_is_current() {
        return Err(renderer_error("TXT_WEBVIEW2_ENVIRONMENT_POLICY"));
    }
    let generation = Uuid::parse_str(&request.renderer_generation)
        .map_err(|_| renderer_error("TXT_RENDERER_GENERATION_INVALID"))?
        .hyphenated()
        .to_string();
    let callback_authority = Arc::new(CallbackAuthority {
        job_id: request.job_id.clone(),
        generation: generation.clone(),
        completion_token: Uuid::new_v4().hyphenated().to_string(),
        lifecycle_version: request.lifecycle_version,
        active: AtomicBool::new(true),
        completed: AtomicBool::new(false),
        cancellation: cancellation.clone(),
    });
    let thread_authority = callback_authority.clone();
    let thread_generation = generation.clone();
    let operation_deadline = request.operation_deadline;
    let worker = thread::Builder::new()
        .name(format!("txt-renderer-{generation}"))
        .spawn(move || render_on_sta(request, thread_authority, operation_deadline))
        .map_err(|_| renderer_error("TXT_RENDERER_START_FAILED"))?;
    let result = worker
        .join()
        .map_err(|_| renderer_error("TXT_RENDERER_CRASHED"))?;
    callback_authority.invalidate();
    result
        .map(|mut evidence| {
            evidence.renderer_generation = thread_generation;
            evidence
        })
        .map_err(map_failure)
}

fn render_on_sta(
    request: TextRenderRequest,
    authority: Arc<CallbackAuthority>,
    operation_deadline: Instant,
) -> Result<TextRenderEvidence, RendererFailure> {
    // SAFETY: this dedicated thread owns the STA and uninitializes it before exit.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
        .ok()
        .map_err(|_| RendererFailure::Native)?;
    let result = test_checkpoint(&authority, RendererCheckpoint::StaInitialized)
        .and_then(|()| render_on_initialized_sta(&request, &authority, operation_deadline));
    authority.invalidate();
    // SAFETY: paired with the successful initialization on this thread.
    unsafe { CoUninitialize() };
    result
}

fn render_on_initialized_sta(
    request: &TextRenderRequest,
    authority: &Arc<CallbackAuthority>,
    operation_deadline: Instant,
) -> Result<TextRenderEvidence, RendererFailure> {
    if !request.user_data_directory.is_dir()
        || request.raw_pdf_path.exists()
        || request
            .raw_pdf_path
            .parent()
            .is_none_or(|path| !path.is_dir())
    {
        return Err(RendererFailure::Native);
    }
    let host = HiddenHostWindow::create(&authority.generation)?;
    let environment = create_environment(request, authority, operation_deadline)?;
    test_checkpoint(authority, RendererCheckpoint::EnvironmentCreated)?;
    let runtime_version = browser_version(&environment)?;
    let controller = create_controller(&environment, host.hwnd(), authority, operation_deadline)?;
    let _controller_owner = ControllerOwner(controller.clone());
    test_checkpoint(authority, RendererCheckpoint::ControllerCreated)?;
    if authority.cancellation.is_cancelled() {
        // SAFETY: this controller belongs to the current STA and generation.
        let _ = unsafe { controller.Close() };
        return Err(RendererFailure::Cancelled);
    }
    let webview = unsafe { controller.CoreWebView2() }?;
    let webview22 = webview.cast::<ICoreWebView2_22>()?;
    let environment6 = environment.cast::<ICoreWebView2Environment6>()?;
    let webview7 = webview.cast::<ICoreWebView2_7>()?;

    // SAFETY: all calls are made on the owning STA before navigation.
    unsafe {
        controller.SetBounds(RECT {
            left: 0,
            top: 0,
            right: 1200,
            bottom: 1600,
        })?;
        controller.SetIsVisible(false)?;
        if let Ok(controller4) = controller.cast::<ICoreWebView2Controller4>() {
            controller4.SetAllowExternalDrop(false)?;
        }
    }

    let resource_state = Rc::new(RefCell::new(ResourceState::new(request)));
    let callback_guard = CallbackGuard::new(authority);
    install_resource_boundary(
        &environment,
        &webview,
        &webview22,
        &controller,
        resource_state.clone(),
        callback_guard.clone(),
    )?;
    apply_settings_and_denials(
        &webview,
        &controller,
        resource_state.clone(),
        callback_guard,
    )?;
    test_checkpoint(authority, RendererCheckpoint::BoundaryInstalled)?;

    // SAFETY: exact navigation occurs only after every filter, handler, setting, and denial.
    unsafe { webview.Navigate(&HSTRING::from(DOCUMENT_URL)) }?;
    test_checkpoint(authority, RendererCheckpoint::NavigationStarted)?;
    pump_until(
        || {
            let state = resource_state.borrow();
            state.is_ready() || state.fatal
        },
        authority,
        READINESS_TIMEOUT,
        operation_deadline,
    )?;
    if resource_state.borrow().fatal || resource_state.borrow().denied_requests != 0 {
        let _ = unsafe { controller.Close() };
        return Err(RendererFailure::Resource);
    }
    test_checkpoint(authority, RendererCheckpoint::ReadinessCompleted)?;
    let print_settings = unsafe { environment6.CreatePrintSettings() }?;
    let (width, height) = request.settings.paper_inches();
    // SAFETY: exact fixed print policy and dimensions are applied on the owning STA.
    unsafe {
        print_settings.SetOrientation(COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT)?;
        print_settings.SetScaleFactor(1.0)?;
        print_settings.SetPageWidth(width)?;
        print_settings.SetPageHeight(height)?;
        print_settings.SetMarginTop(0.5)?;
        print_settings.SetMarginBottom(0.5)?;
        print_settings.SetMarginLeft(0.5)?;
        print_settings.SetMarginRight(0.5)?;
        print_settings.SetShouldPrintBackgrounds(true)?;
        print_settings.SetShouldPrintSelectionOnly(false)?;
        print_settings.SetShouldPrintHeaderAndFooter(false)?;
    }
    let (print_tx, print_rx) = mpsc::channel();
    #[cfg(test)]
    let withheld_print_sender = print_tx.clone();
    let expected_job = authority.job_id.clone();
    let expected_generation = authority.generation.clone();
    let expected_completion_token = authority.completion_token.clone();
    let expected_version = authority.lifecycle_version;
    let print_authority = authority.clone();
    let print_handler = PrintToPdfCompletedHandler::create(Box::new(move |status, success| {
        #[cfg(test)]
        if let Some(action) = test_callback_action(RendererCheckpoint::PrintCallback) {
            match action {
                RendererTestAction::WithholdCallback => return Ok(()),
                RendererTestAction::FailCallback => {
                    if print_authority.complete_once() {
                        let _ = print_tx.send(Err(windows::core::Error::from(
                            windows::Win32::Foundation::E_FAIL,
                        )));
                    }
                    return Ok(());
                }
                _ => {}
            }
        }
        let allowed =
            print_authority.is_current(&expected_job, &expected_generation, expected_version)
                && print_authority.completion_token == expected_completion_token
                && print_authority.complete_once();
        if allowed {
            let _ = print_tx.send(status.map(|_| success));
        }
        Ok(())
    }));
    let output = HSTRING::from(request.raw_pdf_path.as_os_str().to_string_lossy().as_ref());
    test_checkpoint(authority, RendererCheckpoint::PrintStarted)?;
    // SAFETY: callback carries generation ownership; it reports only to this STA.
    unsafe { webview7.PrintToPdf(&output, &print_settings, &print_handler) }?;
    let printed_result = receive_with_pump(
        &print_rx,
        authority,
        PRINT_TIMEOUT,
        operation_deadline,
        Some(&controller),
    );
    #[cfg(test)]
    drop(withheld_print_sender);
    let printed = printed_result??;
    if !printed {
        let _ = unsafe { controller.Close() };
        return Err(RendererFailure::Native);
    }
    test_checkpoint(authority, RendererCheckpoint::PrintCompleted)?;
    let served_urls = resource_state
        .borrow()
        .served_urls
        .iter()
        .cloned()
        .collect();
    let denied_requests = resource_state.borrow().denied_requests;
    let _ = unsafe { controller.Close() };
    Ok(TextRenderEvidence {
        renderer_generation: authority.generation.clone(),
        runtime_version,
        served_urls,
        denied_requests,
    })
}

fn create_environment(
    request: &TextRenderRequest,
    authority: &Arc<CallbackAuthority>,
    operation_deadline: Instant,
) -> Result<ICoreWebView2Environment, RendererFailure> {
    let options = CoreWebView2EnvironmentOptions::default();
    // SAFETY: options are configured before the generated COM interface is shared.
    unsafe {
        options.set_additional_browser_arguments(RENDERER_BROWSER_ARGUMENTS.to_owned());
        options.set_allow_single_sign_on_using_os_primary_account(false);
        options.set_exclusive_user_data_folder_access(true);
        options.set_are_browser_extensions_enabled(false);
    }
    let (sender, receiver) = mpsc::channel();
    #[cfg(test)]
    let withheld_sender = sender.clone();
    let expected_job = authority.job_id.clone();
    let expected_generation = authority.generation.clone();
    let expected_completion_token = authority.completion_token.clone();
    let expected_version = authority.lifecycle_version;
    let callback_authority = authority.clone();
    let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |status, environment| {
            #[cfg(test)]
            if let Some(action) = test_callback_action(RendererCheckpoint::EnvironmentCallback) {
                match action {
                    RendererTestAction::WithholdCallback => return Ok(()),
                    RendererTestAction::FailCallback => {
                        let _ = sender.send(Err(windows::core::Error::from(
                            windows::Win32::Foundation::E_FAIL,
                        )));
                        return Ok(());
                    }
                    _ => {}
                }
            }
            if !callback_authority.owns_generation(
                &expected_job,
                &expected_generation,
                expected_version,
            ) || callback_authority.completion_token != expected_completion_token
            {
                return Ok(());
            }
            let result = status
                .and_then(|_| environment.ok_or_else(|| windows::core::Error::from(E_POINTER)));
            let _ = sender.send(result);
            Ok(())
        },
    ));
    let udf = HSTRING::from(
        request
            .user_data_directory
            .as_os_str()
            .to_string_lossy()
            .as_ref(),
    );
    // SAFETY: WebView2 receives generated COM options and a job-owned UDF.
    unsafe {
        webview2_com::Microsoft::Web::WebView2::Win32::CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            &udf,
            &ICoreWebView2EnvironmentOptions::from(options),
            &handler,
        )
    }?;
    let result = receive_with_pump(
        &receiver,
        authority,
        CREATION_TIMEOUT,
        operation_deadline,
        None,
    );
    #[cfg(test)]
    drop(withheld_sender);
    result?.map_err(RendererFailure::from)
}

fn create_controller(
    environment: &ICoreWebView2Environment,
    parent: HWND,
    authority: &Arc<CallbackAuthority>,
    operation_deadline: Instant,
) -> Result<ICoreWebView2Controller, RendererFailure> {
    let (sender, receiver) = mpsc::channel();
    #[cfg(test)]
    let withheld_sender = sender.clone();
    let expected_job = authority.job_id.clone();
    let expected_generation = authority.generation.clone();
    let expected_completion_token = authority.completion_token.clone();
    let expected_version = authority.lifecycle_version;
    let callback_authority = authority.clone();
    let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
        move |status, controller| {
            #[cfg(test)]
            if let Some(action) = test_callback_action(RendererCheckpoint::ControllerCallback) {
                match action {
                    RendererTestAction::WithholdCallback | RendererTestAction::FailCallback => {
                        if let Some(controller) = controller.as_ref() {
                            // SAFETY: the withheld/failed controller belongs to this callback STA.
                            let _ = unsafe { controller.Close() };
                        }
                        if action == RendererTestAction::FailCallback {
                            let _ = sender.send(Err(windows::core::Error::from(
                                windows::Win32::Foundation::E_FAIL,
                            )));
                        }
                        return Ok(());
                    }
                    _ => {}
                }
            }
            if !callback_authority.owns_generation(
                &expected_job,
                &expected_generation,
                expected_version,
            ) || callback_authority.completion_token != expected_completion_token
            {
                if let Some(controller) = controller {
                    // SAFETY: a stale late-created controller is closed on its callback STA.
                    let _ = unsafe { controller.Close() };
                }
                return Ok(());
            }
            let result = status
                .and_then(|_| controller.ok_or_else(|| windows::core::Error::from(E_POINTER)));
            let _ = sender.send(result);
            Ok(())
        },
    ));
    // SAFETY: parent is the exact hidden window owned by this STA.
    unsafe { environment.CreateCoreWebView2Controller(parent, &handler) }?;
    let result = receive_with_pump(
        &receiver,
        authority,
        CREATION_TIMEOUT,
        operation_deadline,
        None,
    );
    #[cfg(test)]
    drop(withheld_sender);
    result?.map_err(RendererFailure::from)
}

fn browser_version(environment: &ICoreWebView2Environment) -> Result<String, RendererFailure> {
    let mut value = windows::core::PWSTR::null();
    // SAFETY: generated getter initializes the owned COM string.
    unsafe { environment.BrowserVersionString(&mut value) }?;
    Ok(take_pwstr(value))
}

fn install_resource_boundary(
    environment: &ICoreWebView2Environment,
    webview: &ICoreWebView2,
    webview22: &ICoreWebView2_22,
    controller: &ICoreWebView2Controller,
    state: Rc<RefCell<ResourceState>>,
    callback_guard: CallbackGuard,
) -> Result<(), RendererFailure> {
    // SAFETY: the all-context/all-source filter is installed globally before navigation.
    unsafe {
        webview22.AddWebResourceRequestedFilterWithRequestSourceKinds(
            &HSTRING::from("*"),
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
            COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
        )?;
    }
    let environment = environment.clone();
    let controller = controller.clone();
    let request_state = state.clone();
    let request_guard = callback_guard.clone();
    let handler = WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
        if !request_guard.owns_current_generation() {
            let result = deny_stale_resource_request(&environment, &request_state, args);
            // SAFETY: this is the exact controller captured for the stale generation.
            let _ = unsafe { controller.Close() };
            return result;
        }
        let result = handle_resource_request(&environment, &request_state, args);
        if result.is_err() || request_state.borrow().fatal {
            request_state.borrow_mut().fatal = true;
            // SAFETY: this is the exact controller captured for this generation.
            let _ = unsafe { controller.Close() };
        }
        result
    }));
    let mut token = EventRegistrationToken::default();
    // SAFETY: handler is retained by WebView2 for the controller lifetime.
    unsafe { webview.add_WebResourceRequested(&handler, &mut token) }?;
    let received_state = state.clone();
    let received_guard = callback_guard;
    let received = WebResourceResponseReceivedEventHandler::create(Box::new(move |_, args| {
        if !received_guard.owns_current_generation() {
            return Ok(());
        }
        let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        let request = unsafe { args.Request()? };
        let uri = take_request_uri(&request)?;
        if matches!(
            uri.as_str(),
            DOCUMENT_URL | CSS_URL | NOTO_SANS_URL | NOTO_DEVANAGARI_URL | NOTO_TELUGU_URL
        ) {
            received_state.borrow_mut().received_urls.insert(uri);
        } else {
            received_state.borrow_mut().fatal = true;
        }
        Ok(())
    }));
    let webview2 = webview.cast::<ICoreWebView2_2>()?;
    // SAFETY: this completion accounting handler is installed before navigation.
    unsafe { webview2.add_WebResourceResponseReceived(&received, &mut token) }?;
    Ok(())
}

fn deny_stale_resource_request(
    environment: &ICoreWebView2Environment,
    state: &Rc<RefCell<ResourceState>>,
    args: Option<ICoreWebView2WebResourceRequestedEventArgs>,
) -> windows::core::Result<()> {
    let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
    {
        let mut state = state.borrow_mut();
        state.reserve_response(0)?;
        state.record_denied()?;
        state.fatal = true;
    }
    let retained = create_response(
        environment,
        Arc::from([]),
        403,
        "Forbidden",
        &response_headers("text/plain", 0)?,
    )?;
    unsafe { args.SetResponse(&retained.response)? };
    state.borrow_mut().responses.push(retained);
    Ok(())
}

fn handle_resource_request(
    environment: &ICoreWebView2Environment,
    state: &Rc<RefCell<ResourceState>>,
    args: Option<ICoreWebView2WebResourceRequestedEventArgs>,
) -> windows::core::Result<()> {
    let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
    let request = unsafe { args.Request()? };
    let uri = take_request_uri(&request)?;
    let method = take_request_method(&request)?;
    let mut context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL;
    unsafe { args.ResourceContext(&mut context)? };
    let args2 = args.cast::<ICoreWebView2WebResourceRequestedEventArgs2>()?;
    let mut source = COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_NONE;
    unsafe { args2.RequestedSourceKind(&mut source)? };
    let route = route_request(&uri, &method, context, source);
    let (status, reason, headers, bytes, served_url, denied) = match route {
        ResourceRoute::Allowed(kind) => {
            let length = state.borrow().resource_length(kind);
            state.borrow_mut().reserve_response(length)?;
            let (mime, bytes) = state.borrow().resource(kind);
            let headers = response_headers(mime, bytes.len())?;
            (200, "OK", headers, bytes, Some(uri), false)
        }
        ResourceRoute::WrongMethod => {
            let mut state = state.borrow_mut();
            state.reserve_response(0)?;
            state.record_denied()?;
            (
                405,
                "Method Not Allowed",
                response_headers("text/plain", 0)?,
                Arc::from([]),
                None,
                true,
            )
        }
        ResourceRoute::Denied => {
            let mut state = state.borrow_mut();
            state.reserve_response(0)?;
            state.record_denied()?;
            (
                403,
                "Forbidden",
                response_headers("text/plain", 0)?,
                Arc::from([]),
                None,
                true,
            )
        }
    };
    let retained = create_response(environment, bytes, status, reason, &headers)?;
    unsafe { args.SetResponse(&retained.response)? };
    let mut state = state.borrow_mut();
    if let Some(url) = served_url {
        state.served_urls.insert(url);
    }
    state.fatal |= denied;
    state.responses.push(retained);
    Ok(())
}

fn take_request_uri(request: &ICoreWebView2WebResourceRequest) -> windows::core::Result<String> {
    let mut value = windows::core::PWSTR::null();
    unsafe { request.Uri(&mut value)? };
    Ok(take_pwstr(value))
}

fn take_request_method(request: &ICoreWebView2WebResourceRequest) -> windows::core::Result<String> {
    let mut value = windows::core::PWSTR::null();
    unsafe { request.Method(&mut value)? };
    Ok(take_pwstr(value))
}

fn response_headers(mime: &str, length: usize) -> windows::core::Result<String> {
    if length > u32::MAX as usize {
        return Err(windows::core::Error::from(E_POINTER));
    }
    Ok(format!(
        "Content-Type: {mime}\r\nContent-Length: {length}\r\nX-Content-Type-Options: nosniff\r\nCache-Control: no-store\r\nContent-Security-Policy: {CONTENT_SECURITY_POLICY}\r\n"
    ))
}

fn create_response(
    environment: &ICoreWebView2Environment,
    bytes: Arc<[u8]>,
    status: i32,
    reason: &str,
    headers: &str,
) -> windows::core::Result<RetainedResponse> {
    if bytes.len() > u32::MAX as usize {
        return Err(windows::core::Error::from(E_POINTER));
    }
    // SAFETY: the slice is immutable and retained beside the stream and response.
    let stream = unsafe { SHCreateMemStream(Some(bytes.as_ref())) }
        .ok_or_else(|| windows::core::Error::from(E_POINTER))?;
    let mut stat = STATSTG::default();
    // SAFETY: `stat` is initialized and the stream is a generated COM projection.
    unsafe { stream.Stat(&mut stat, STATFLAG_NONAME)? };
    if stat.cbSize != bytes.len() as u64 {
        return Err(windows::core::Error::from(E_POINTER));
    }
    // SAFETY: all params are generated projections with bounded owned backing data.
    let response = unsafe {
        environment.CreateWebResourceResponse(
            &stream,
            status,
            &HSTRING::from(reason),
            &HSTRING::from(headers),
        )?
    };
    Ok(RetainedResponse {
        _bytes: bytes,
        _stream: stream,
        response,
    })
}

fn apply_settings_and_denials(
    webview: &ICoreWebView2,
    controller: &ICoreWebView2Controller,
    state: Rc<RefCell<ResourceState>>,
    callback_guard: CallbackGuard,
) -> Result<(), RendererFailure> {
    let settings = unsafe { webview.Settings() }?;
    // SAFETY: every setting is applied before navigation.
    unsafe {
        settings.SetIsScriptEnabled(false)?;
        settings.SetIsWebMessageEnabled(false)?;
        settings.SetAreHostObjectsAllowed(false)?;
        settings.SetAreDevToolsEnabled(false)?;
        settings.SetAreDefaultContextMenusEnabled(false)?;
        settings.SetAreDefaultScriptDialogsEnabled(false)?;
        settings.SetIsBuiltInErrorPageEnabled(false)?;
        settings.SetIsStatusBarEnabled(false)?;
        settings.SetIsZoomControlEnabled(false)?;
        settings
            .cast::<ICoreWebView2Settings3>()?
            .SetAreBrowserAcceleratorKeysEnabled(false)?;
    }
    let mut token = EventRegistrationToken::default();
    let navigation_state = state.clone();
    unsafe {
        let dom_state = state.clone();
        let dom_guard = callback_guard.clone();
        webview.cast::<ICoreWebView2_2>()?.add_DOMContentLoaded(
            &DOMContentLoadedEventHandler::create(Box::new(move |_, _| {
                if !dom_guard.owns_current_generation() {
                    return Ok(());
                }
                dom_state.borrow_mut().dom_content_loaded = true;
                Ok(())
            })),
            &mut token,
        )?;
        let navigation_guard = callback_guard.clone();
        webview.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
                let mut value = windows::core::PWSTR::null();
                args.Uri(&mut value)?;
                if take_pwstr(value) != DOCUMENT_URL {
                    args.SetCancel(true)?;
                    if navigation_guard.owns_current_generation() {
                        navigation_state.borrow_mut().fatal = true;
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
        let completed_state = state.clone();
        let completed_guard = callback_guard.clone();
        webview.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(move |_, args| {
                if !completed_guard.owns_current_generation() {
                    return Ok(());
                }
                let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
                let mut success = windows::core::BOOL::default();
                args.IsSuccess(&mut success)?;
                let mut state = completed_state.borrow_mut();
                state.navigation_completed = success.as_bool();
                state.fatal |= !success.as_bool();
                Ok(())
            })),
            &mut token,
        )?;
        let process_state = state.clone();
        let process_guard = callback_guard.clone();
        webview.add_ProcessFailed(
            &ProcessFailedEventHandler::create(Box::new(move |_, _| {
                if !process_guard.owns_current_generation() {
                    return Ok(());
                }
                process_state.borrow_mut().fatal = true;
                Ok(())
            })),
            &mut token,
        )?;
        let new_window_state = state.clone();
        let new_window_guard = callback_guard.clone();
        webview.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
                let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
                args.SetHandled(true)?;
                if new_window_guard.owns_current_generation() {
                    new_window_state.borrow_mut().fatal = true;
                }
                Ok(())
            })),
            &mut token,
        )?;
        let permission_state = state.clone();
        let permission_guard = callback_guard.clone();
        webview.add_PermissionRequested(
            &PermissionRequestedEventHandler::create(Box::new(move |_, args| {
                let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
                args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?;
                if permission_guard.owns_current_generation() {
                    permission_state.borrow_mut().fatal = true;
                }
                Ok(())
            })),
            &mut token,
        )?;
        let download_state = state.clone();
        let download_guard = callback_guard.clone();
        webview.cast::<ICoreWebView2_4>()?.add_DownloadStarting(
            &DownloadStartingEventHandler::create(Box::new(move |_, args| {
                let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
                args.SetCancel(true)?;
                args.SetHandled(true)?;
                if download_guard.owns_current_generation() {
                    download_state.borrow_mut().fatal = true;
                }
                Ok(())
            })),
            &mut token,
        )?;
        let client_certificate_state = state.clone();
        let client_certificate_guard = callback_guard.clone();
        webview
            .cast::<ICoreWebView2_5>()?
            .add_ClientCertificateRequested(
                &ClientCertificateRequestedEventHandler::create(Box::new(move |_, args| {
                    let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
                    args.SetCancel(true)?;
                    args.SetHandled(true)?;
                    if client_certificate_guard.owns_current_generation() {
                        client_certificate_state.borrow_mut().fatal = true;
                    }
                    Ok(())
                })),
                &mut token,
            )?;
        let authentication_state = state.clone();
        let authentication_guard = callback_guard.clone();
        webview
            .cast::<ICoreWebView2_10>()?
            .add_BasicAuthenticationRequested(
                &BasicAuthenticationRequestedEventHandler::create(Box::new(move |_, args| {
                    args.ok_or_else(|| windows::core::Error::from(E_POINTER))?
                        .SetCancel(true)?;
                    if authentication_guard.owns_current_generation() {
                        authentication_state.borrow_mut().fatal = true;
                    }
                    Ok(())
                })),
                &mut token,
            )?;
        let certificate_state = state.clone();
        let certificate_guard = callback_guard.clone();
        webview
            .cast::<ICoreWebView2_14>()?
            .add_ServerCertificateErrorDetected(
                &ServerCertificateErrorDetectedEventHandler::create(Box::new(move |_, args| {
                    args.ok_or_else(|| windows::core::Error::from(E_POINTER))?
                        .SetAction(COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_CANCEL)?;
                    if certificate_guard.owns_current_generation() {
                        certificate_state.borrow_mut().fatal = true;
                    }
                    Ok(())
                })),
                &mut token,
            )?;
        let accelerator_guard = callback_guard;
        controller.add_AcceleratorKeyPressed(
            &AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                let args = args.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
                args.SetHandled(true)?;
                let _ = accelerator_guard.owns_current_generation();
                Ok(())
            })),
            &mut token,
        )?;
    }
    Ok(())
}

fn receive_with_pump<T>(
    receiver: &Receiver<T>,
    authority: &Arc<CallbackAuthority>,
    timeout: Duration,
    operation_deadline: Instant,
    controller: Option<&ICoreWebView2Controller>,
) -> Result<T, RendererFailure> {
    let started = Instant::now();
    loop {
        match receiver.try_recv() {
            Ok(value) => return Ok(value),
            Err(TryRecvError::Disconnected) => return Err(RendererFailure::Native),
            Err(TryRecvError::Empty) => {}
        }
        if authority.cancellation.is_cancelled() && controller.is_some() {
            authority.invalidate();
            if let Some(controller) = controller {
                let _ = unsafe { controller.Close() };
            }
            return Err(RendererFailure::Cancelled);
        }
        if started.elapsed() >= timeout || Instant::now() >= operation_deadline {
            authority.invalidate();
            if let Some(controller) = controller {
                let _ = unsafe { controller.Close() };
            }
            return Err(RendererFailure::Timeout);
        }
        pump_messages()?;
        thread::sleep(PUMP_INTERVAL);
    }
}

fn pump_until(
    condition: impl Fn() -> bool,
    authority: &Arc<CallbackAuthority>,
    timeout: Duration,
    operation_deadline: Instant,
) -> Result<(), RendererFailure> {
    let started = Instant::now();
    loop {
        if condition() {
            return Ok(());
        }
        if authority.cancellation.is_cancelled() {
            authority.invalidate();
            return Err(RendererFailure::Cancelled);
        }
        if started.elapsed() >= timeout || Instant::now() >= operation_deadline {
            authority.invalidate();
            return Err(RendererFailure::Timeout);
        }
        pump_messages()?;
        thread::sleep(PUMP_INTERVAL);
    }
}

fn pump_messages() -> Result<(), RendererFailure> {
    let mut message = MSG::default();
    // SAFETY: the message structure is initialized and owned by this STA.
    unsafe {
        while PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) != 0 {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResourceKind {
    Html,
    Css,
    NotoSans,
    NotoDevanagari,
    NotoTelugu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResourceRoute {
    Allowed(ResourceKind),
    WrongMethod,
    Denied,
}

fn route_request(
    uri: &str,
    method: &str,
    context: COREWEBVIEW2_WEB_RESOURCE_CONTEXT,
    source: COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS,
) -> ResourceRoute {
    let kind = match uri {
        DOCUMENT_URL => ResourceKind::Html,
        CSS_URL => ResourceKind::Css,
        NOTO_SANS_URL => ResourceKind::NotoSans,
        NOTO_DEVANAGARI_URL => ResourceKind::NotoDevanagari,
        NOTO_TELUGU_URL => ResourceKind::NotoTelugu,
        _ => return ResourceRoute::Denied,
    };
    if method != "GET" {
        return ResourceRoute::WrongMethod;
    }
    let expected_context = match kind {
        ResourceKind::Html => COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT,
        ResourceKind::Css => COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET,
        ResourceKind::NotoSans | ResourceKind::NotoDevanagari | ResourceKind::NotoTelugu => {
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT
        }
    };
    if context != expected_context
        || source != COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_DOCUMENT
    {
        return ResourceRoute::Denied;
    }
    ResourceRoute::Allowed(kind)
}

struct RetainedResponse {
    _bytes: Arc<[u8]>,
    _stream: IStream,
    response: ICoreWebView2WebResourceResponse,
}

struct ControllerOwner(ICoreWebView2Controller);

impl Drop for ControllerOwner {
    fn drop(&mut self) {
        // SAFETY: the guard is created and dropped on the controller's owning STA.
        let _ = unsafe { self.0.Close() };
    }
}

struct ResourceState {
    html: Arc<[u8]>,
    css: Arc<[u8]>,
    used_scripts: BTreeSet<AdmittedScript>,
    served_urls: BTreeSet<String>,
    received_urls: BTreeSet<String>,
    responses: Vec<RetainedResponse>,
    navigation_completed: bool,
    dom_content_loaded: bool,
    fatal: bool,
    denied_requests: u32,
    response_count: usize,
    served_bytes: usize,
}

impl ResourceState {
    fn new(request: &TextRenderRequest) -> Self {
        Self {
            html: request.html.clone(),
            css: request.css.clone(),
            used_scripts: request.used_scripts.clone(),
            served_urls: BTreeSet::new(),
            received_urls: BTreeSet::new(),
            responses: Vec::new(),
            navigation_completed: false,
            dom_content_loaded: false,
            fatal: false,
            denied_requests: 0,
            response_count: 0,
            served_bytes: 0,
        }
    }

    fn resource_length(&self, kind: ResourceKind) -> usize {
        match kind {
            ResourceKind::Html => self.html.len(),
            ResourceKind::Css => self.css.len(),
            ResourceKind::NotoSans => NOTO_SANS_BYTES.len(),
            ResourceKind::NotoDevanagari => NOTO_DEVANAGARI_BYTES.len(),
            ResourceKind::NotoTelugu => NOTO_TELUGU_BYTES.len(),
        }
    }

    fn reserve_response(&mut self, length: usize) -> windows::core::Result<()> {
        let response_count = self
            .response_count
            .checked_add(1)
            .ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        let served_bytes = self
            .served_bytes
            .checked_add(length)
            .ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        if response_count > MAX_RETAINED_RESPONSES || served_bytes > TXT_MAX_SERVED_BYTES {
            return Err(windows::core::Error::from(E_POINTER));
        }
        self.response_count = response_count;
        self.served_bytes = served_bytes;
        Ok(())
    }

    fn record_denied(&mut self) -> windows::core::Result<()> {
        self.denied_requests = self
            .denied_requests
            .checked_add(1)
            .ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        Ok(())
    }

    fn resource(&self, kind: ResourceKind) -> (&'static str, Arc<[u8]>) {
        match kind {
            ResourceKind::Html => ("text/html; charset=utf-8", self.html.clone()),
            ResourceKind::Css => ("text/css; charset=utf-8", self.css.clone()),
            ResourceKind::NotoSans => ("font/ttf", Arc::from(NOTO_SANS_BYTES)),
            ResourceKind::NotoDevanagari => ("font/ttf", Arc::from(NOTO_DEVANAGARI_BYTES)),
            ResourceKind::NotoTelugu => ("font/ttf", Arc::from(NOTO_TELUGU_BYTES)),
        }
    }

    fn is_ready(&self) -> bool {
        if self.fatal
            || self.denied_requests != 0
            || !self.navigation_completed
            || !self.dom_content_loaded
            || !self.served_urls.contains(DOCUMENT_URL)
            || !self.served_urls.contains(CSS_URL)
            || !self.received_urls.contains(DOCUMENT_URL)
            || !self.received_urls.contains(CSS_URL)
        {
            return false;
        }
        self.used_scripts.iter().all(|script| {
            let url = match script {
                AdmittedScript::LatinCommon => NOTO_SANS_URL,
                AdmittedScript::Devanagari => NOTO_DEVANAGARI_URL,
                AdmittedScript::Telugu => NOTO_TELUGU_URL,
            };
            self.served_urls.contains(url) && self.received_urls.contains(url)
        })
    }
}

struct HiddenHostWindow {
    hwnd: windows_sys::Win32::Foundation::HWND,
    class_name: Vec<u16>,
    instance: windows_sys::Win32::Foundation::HINSTANCE,
}

impl HiddenHostWindow {
    fn create(generation: &str) -> Result<Self, RendererFailure> {
        let class_name = wide_z(OsStr::new(&format!(
            "DocumentStudioTxtRenderer-{generation}"
        )));
        let window_name = wide_z(OsStr::new("Document Studio private TXT renderer"));
        // SAFETY: null requests the current module handle.
        let instance = unsafe { GetModuleHandleW(null()) };
        if instance.is_null() {
            return Err(RendererFailure::Native);
        }
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(hidden_window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: null_mut(),
        };
        // SAFETY: class strings are terminated and live through unregistration.
        if unsafe { RegisterClassExW(&class) } == 0 {
            return Err(RendererFailure::Native);
        }
        // SAFETY: creates a non-visible, non-activating tool window owned by this STA.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                class_name.as_ptr(),
                window_name.as_ptr(),
                WS_OVERLAPPED,
                0,
                0,
                1200,
                1600,
                null_mut(),
                null_mut(),
                instance,
                null(),
            )
        };
        if hwnd.is_null() {
            let _ = unsafe { UnregisterClassW(class_name.as_ptr(), instance) };
            return Err(RendererFailure::Native);
        }
        Ok(Self {
            hwnd,
            class_name,
            instance,
        })
    }

    fn hwnd(&self) -> HWND {
        HWND(self.hwnd)
    }
}

impl Drop for HiddenHostWindow {
    fn drop(&mut self) {
        // SAFETY: only exact handles created by this owner are destroyed.
        unsafe {
            if !self.hwnd.is_null() {
                DestroyWindow(self.hwnd);
            }
            UnregisterClassW(self.class_name.as_ptr(), self.instance);
        }
    }
}

unsafe extern "system" fn hidden_window_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: default processing receives the unmodified generated callback parameters.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn wide_z(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn map_failure(failure: RendererFailure) -> OperationError {
    match failure {
        RendererFailure::Cancelled => cancelled_error(),
        RendererFailure::Timeout => OperationError::safe(
            "TXT_RENDERER_TIMEOUT",
            "The private TXT renderer timed out",
            "No output was published. The owned renderer was closed.",
            OperationStage::Execute,
            true,
        ),
        RendererFailure::Resource => OperationError::safe(
            "TXT_RESOURCE_BOUNDARY_FAILED",
            "The private TXT resource boundary failed",
            "An unexpected or failing resource was denied and no output was published.",
            OperationStage::Execute,
            false,
        ),
        RendererFailure::Native => renderer_error("TXT_RENDERER_FAILED"),
    }
}

fn cancelled_error() -> OperationError {
    OperationError::safe(
        "CANCELLED",
        "TXT-to-PDF conversion was cancelled",
        "No unpublished output was retained.",
        OperationStage::Execute,
        false,
    )
}

fn renderer_error(code: &str) -> OperationError {
    OperationError::safe(
        code,
        "The private TXT renderer failed",
        "No output was published.",
        OperationStage::Execute,
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn exact_resource_routes_are_fail_closed() {
        let document = route_request(
            DOCUMENT_URL,
            "GET",
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT,
            COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_DOCUMENT,
        );
        assert_eq!(document, ResourceRoute::Allowed(ResourceKind::Html));
        assert_eq!(
            route_request(
                DOCUMENT_URL,
                "POST",
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_DOCUMENT,
            ),
            ResourceRoute::WrongMethod
        );
        for uri in [
            "https://txt-renderer.document-studio.invalid/1/document.html?x=1",
            "https://txt-renderer.document-studio.invalid:443/1/document.html",
            "https://user@txt-renderer.document-studio.invalid/1/document.html",
            "http://localhost/",
            "https://example.com/",
            "file:///private.txt",
            "data:text/html,blocked",
        ] {
            assert_eq!(
                route_request(
                    uri,
                    "GET",
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT,
                    COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_DOCUMENT,
                ),
                ResourceRoute::Denied,
                "{uri}"
            );
        }
        assert_eq!(
            route_request(
                CSS_URL,
                "GET",
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_DOCUMENT,
            ),
            ResourceRoute::Denied
        );
        assert_eq!(
            route_request(
                CSS_URL,
                "GET",
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_SERVICE_WORKER,
            ),
            ResourceRoute::Denied
        );
    }

    #[test]
    fn stream_stat_proves_exact_lengths() {
        let environmentless = Arc::<[u8]>::from(&b"bounded"[..]);
        let stream = unsafe { SHCreateMemStream(Some(environmentless.as_ref())) }.expect("stream");
        let mut stat = STATSTG::default();
        unsafe { stream.Stat(&mut stat, STATFLAG_NONAME) }.expect("stat");
        assert_eq!(stat.cbSize, 7);
    }

    #[test]
    fn readiness_requires_dom_navigation_and_completed_expected_responses() {
        let mut state = ResourceState {
            html: Arc::from(&b"html"[..]),
            css: Arc::from(&b"css"[..]),
            used_scripts: BTreeSet::from([
                AdmittedScript::LatinCommon,
                AdmittedScript::Devanagari,
                AdmittedScript::Telugu,
            ]),
            served_urls: BTreeSet::new(),
            received_urls: BTreeSet::new(),
            responses: Vec::new(),
            navigation_completed: false,
            dom_content_loaded: false,
            fatal: false,
            denied_requests: 0,
            response_count: 0,
            served_bytes: 0,
        };
        assert!(!state.is_ready());
        state.navigation_completed = true;
        state.dom_content_loaded = true;
        for url in [
            DOCUMENT_URL,
            CSS_URL,
            NOTO_SANS_URL,
            NOTO_DEVANAGARI_URL,
            NOTO_TELUGU_URL,
        ] {
            state.served_urls.insert(url.to_owned());
        }
        assert!(!state.is_ready());
        for url in [
            DOCUMENT_URL,
            CSS_URL,
            NOTO_SANS_URL,
            NOTO_DEVANAGARI_URL,
            NOTO_TELUGU_URL,
        ] {
            state.received_urls.insert(url.to_owned());
        }
        assert!(state.is_ready());
        state.denied_requests = 1;
        assert!(!state.is_ready());
    }

    #[test]
    fn response_accounting_caps_cumulative_bytes_and_retained_count_before_allocation() {
        let mut state = ResourceState {
            html: Arc::from(&b"html"[..]),
            css: Arc::from(&b"css"[..]),
            used_scripts: BTreeSet::new(),
            served_urls: BTreeSet::new(),
            received_urls: BTreeSet::new(),
            responses: Vec::new(),
            navigation_completed: false,
            dom_content_loaded: false,
            fatal: false,
            denied_requests: 0,
            response_count: 0,
            served_bytes: TXT_MAX_SERVED_BYTES - 1,
        };
        state.reserve_response(1).expect("inclusive byte cap");
        assert_eq!(state.served_bytes, TXT_MAX_SERVED_BYTES);
        assert!(state.reserve_response(1).is_err());

        state.response_count = MAX_RETAINED_RESPONSES;
        state.served_bytes = 0;
        assert!(state.reserve_response(0).is_err());

        state.response_count = 0;
        state.served_bytes = usize::MAX;
        assert!(state.reserve_response(1).is_err());
    }

    #[test]
    fn callback_authority_rejects_stale_duplicate_and_cancelled_completions() {
        let registry = crate::app_state::CancellationRegistry::default();
        let token = registry.register("job");
        let authority = Arc::new(CallbackAuthority {
            job_id: "job".to_owned(),
            generation: "generation".to_owned(),
            completion_token: "one-shot".to_owned(),
            lifecycle_version: 7,
            active: AtomicBool::new(true),
            completed: AtomicBool::new(false),
            cancellation: token,
        });
        let guard = CallbackGuard::new(&authority);
        assert!(authority.is_current("job", "generation", 7));
        assert!(guard.owns_current_generation());
        assert!(!authority.is_current("replacement", "generation", 7));
        assert!(!authority.is_current("job", "new-generation", 7));
        assert!(!authority.is_current("job", "generation", 8));
        assert!(authority.complete_once());
        assert!(!authority.complete_once());
        registry.request("job");
        assert!(!authority.is_current("job", "generation", 7));
        assert!(!guard.owns_current_generation());
        authority.invalidate();
        assert!(!authority.active.load(Ordering::Acquire));
    }

    #[test]
    fn response_headers_are_no_store_nosniff_and_u32_bounded() {
        let headers = response_headers("text/plain", 7).unwrap();
        assert!(headers.contains("Content-Length: 7\r\n"));
        assert!(headers.contains("X-Content-Type-Options: nosniff\r\n"));
        assert!(headers.contains("Cache-Control: no-store\r\n"));
        assert!(headers.contains(CONTENT_SECURITY_POLICY));
        assert!(response_headers("text/plain", u32::MAX as usize + 1).is_err());
    }

    #[test]
    #[ignore = "runs the real hidden WebView2 renderer fault and cancellation matrix"]
    fn native_renderer_fault_matrix_closes_exact_generation_at_bounded_checkpoints() {
        crate::webview2_environment::enforce_webview2_environment_policy();
        let lane = tempfile::tempdir().unwrap();
        let normalized = crate::text_to_pdf::preflight_text(
            "English हिन्दी తెలుగు क्\u{200d}ष క్\u{200c}ష".as_bytes(),
        )
        .unwrap();
        let html: Arc<[u8]> = crate::text_to_pdf::canonical_html(&normalized.text)
            .unwrap()
            .into();
        let css: Arc<[u8]> = crate::text_to_pdf::canonical_css().unwrap().into();
        for (index, checkpoint, action, expected_code) in [
            (
                0,
                RendererCheckpoint::StaInitialized,
                RendererTestAction::Fail,
                "TXT_RENDERER_FAILED",
            ),
            (
                1,
                RendererCheckpoint::StaInitialized,
                RendererTestAction::Timeout,
                "TXT_RENDERER_TIMEOUT",
            ),
            (
                2,
                RendererCheckpoint::StaInitialized,
                RendererTestAction::Crash,
                "TXT_RENDERER_CRASHED",
            ),
            (
                3,
                RendererCheckpoint::EnvironmentCreated,
                RendererTestAction::Cancel,
                "CANCELLED",
            ),
            (
                4,
                RendererCheckpoint::ControllerCreated,
                RendererTestAction::Fail,
                "TXT_RENDERER_FAILED",
            ),
            (
                5,
                RendererCheckpoint::BoundaryInstalled,
                RendererTestAction::Cancel,
                "CANCELLED",
            ),
            (
                6,
                RendererCheckpoint::NavigationStarted,
                RendererTestAction::Fail,
                "TXT_RENDERER_FAILED",
            ),
            (
                7,
                RendererCheckpoint::ReadinessCompleted,
                RendererTestAction::Timeout,
                "TXT_RENDERER_TIMEOUT",
            ),
            (
                8,
                RendererCheckpoint::PrintStarted,
                RendererTestAction::Cancel,
                "CANCELLED",
            ),
            (
                9,
                RendererCheckpoint::PrintCompleted,
                RendererTestAction::Fail,
                "TXT_RENDERER_FAILED",
            ),
            (
                10,
                RendererCheckpoint::StaInitialized,
                RendererTestAction::Cancel,
                "CANCELLED",
            ),
            (
                11,
                RendererCheckpoint::ControllerCreated,
                RendererTestAction::Cancel,
                "CANCELLED",
            ),
            (
                12,
                RendererCheckpoint::NavigationStarted,
                RendererTestAction::Cancel,
                "CANCELLED",
            ),
            (
                13,
                RendererCheckpoint::ReadinessCompleted,
                RendererTestAction::Cancel,
                "CANCELLED",
            ),
            (
                14,
                RendererCheckpoint::PrintCompleted,
                RendererTestAction::Cancel,
                "CANCELLED",
            ),
            (
                15,
                RendererCheckpoint::EnvironmentCallback,
                RendererTestAction::WithholdCallback,
                "TXT_RENDERER_TIMEOUT",
            ),
            (
                16,
                RendererCheckpoint::EnvironmentCallback,
                RendererTestAction::FailCallback,
                "TXT_RENDERER_FAILED",
            ),
            (
                17,
                RendererCheckpoint::ControllerCallback,
                RendererTestAction::WithholdCallback,
                "TXT_RENDERER_TIMEOUT",
            ),
            (
                18,
                RendererCheckpoint::ControllerCallback,
                RendererTestAction::FailCallback,
                "TXT_RENDERER_FAILED",
            ),
            (
                19,
                RendererCheckpoint::PrintCallback,
                RendererTestAction::WithholdCallback,
                "TXT_RENDERER_TIMEOUT",
            ),
            (
                20,
                RendererCheckpoint::PrintCallback,
                RendererTestAction::FailCallback,
                "TXT_RENDERER_FAILED",
            ),
        ] {
            let root = lane.path().join(format!("case-{index}"));
            let udf = root.join("udf");
            fs::create_dir_all(&udf).unwrap();
            let raw_pdf_path = root.join("raw.pdf");
            let job_id = Uuid::new_v4().hyphenated().to_string();
            let generation = Uuid::new_v4().hyphenated().to_string();
            let registry = crate::app_state::CancellationRegistry::default();
            let token = registry.register(&job_id);
            *RENDERER_TEST_FAULT
                .get_or_init(|| Mutex::new(None))
                .lock()
                .unwrap() = Some((checkpoint, action));
            let error = render_text_pdf(
                TextRenderRequest {
                    job_id,
                    renderer_generation: generation,
                    lifecycle_version: 1,
                    user_data_directory: udf,
                    raw_pdf_path: raw_pdf_path.clone(),
                    html: html.clone(),
                    css: css.clone(),
                    used_scripts: normalized.used_scripts.clone(),
                    settings: TextToPdfSettings {
                        page_size: crate::text_to_pdf::TextPageSize::A4,
                        orientation: crate::text_to_pdf::TextOrientation::Portrait,
                    },
                    operation_deadline: Instant::now()
                        + if action == RendererTestAction::WithholdCallback {
                            match checkpoint {
                                RendererCheckpoint::PrintCallback => Duration::from_secs(15),
                                _ => Duration::from_secs(3),
                            }
                        } else {
                            Duration::from_secs(120)
                        },
                },
                token,
            )
            .unwrap_err();
            *RENDERER_TEST_FAULT.get().unwrap().lock().unwrap() = None;
            assert_eq!(error.code, expected_code, "{checkpoint:?} {action:?}");
            if raw_pdf_path.exists() {
                fs::remove_file(raw_pdf_path).unwrap();
            }
            let cleanup_deadline = Instant::now() + Duration::from_secs(10);
            loop {
                match fs::remove_dir_all(&root) {
                    Ok(()) => break,
                    Err(error) if Instant::now() < cleanup_deadline => {
                        let _ = error;
                        thread::sleep(Duration::from_millis(50));
                    }
                    Err(error) => {
                        panic!("owned renderer cleanup failed for {checkpoint:?}: {error}")
                    }
                }
            }
        }
    }
}
