//! Compile-only proof for the owner-approved G04E1 Windows COM surface.
//!
//! Production renderer code uses the same generated interfaces. This module
//! deliberately performs no environment creation, navigation, printing, or
//! resource response at runtime.

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Environment, ICoreWebView2Environment6, ICoreWebView2WebResourceResponse,
    ICoreWebView2_22, ICoreWebView2_7, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
    COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
};
use webview2_com::{PrintToPdfCompletedHandler, WebResourceRequestedEventHandler};
use windows::core::PCWSTR;
use windows::Win32::System::Com::IStream;
use windows::Win32::UI::Shell::SHCreateMemStream;

/// Type-checks every generated interface and method required by the renderer.
///
/// The function is never invoked by Document Studio. Keeping it in the
/// production crate prevents the dependency edges from being test-only.
#[doc(hidden)]
pub unsafe fn assert_g04e1_webview2_surface(
    environment: &ICoreWebView2Environment,
    environment6: &ICoreWebView2Environment6,
    webview22: &ICoreWebView2_22,
    webview7: &ICoreWebView2_7,
    response: &ICoreWebView2WebResourceResponse,
) -> windows::core::Result<()> {
    let bytes = [0_u8; 1];
    let stream: IStream =
        unsafe { SHCreateMemStream(Some(&bytes)) }.ok_or_else(windows::core::Error::from_win32)?;

    let mut stat = windows::Win32::System::Com::STATSTG::default();
    unsafe { stream.Stat(&mut stat, windows::Win32::System::Com::STATFLAG_NONAME) }?;

    let reason = PCWSTR::null();
    let headers = PCWSTR::null();
    let _: ICoreWebView2WebResourceResponse =
        unsafe { environment.CreateWebResourceResponse(&stream, 200, reason, headers) }?;

    unsafe {
        webview22.AddWebResourceRequestedFilterWithRequestSourceKinds(
            PCWSTR::null(),
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
            COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
        )
    }?;

    let print_settings = unsafe { environment6.CreatePrintSettings() }?;
    let completed = PrintToPdfCompletedHandler::create(Box::new(|_, _| Ok(())));
    unsafe { webview7.PrintToPdf(PCWSTR::null(), &print_settings, &completed) }?;

    let requested = WebResourceRequestedEventHandler::create(Box::new(|_, _| Ok(())));
    let _ = requested;
    let _ = response;
    Ok(())
}
