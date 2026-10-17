//! Native WebView differences for the test-only text-input and editing device.
use super::Input;
use tauri::{Runtime, WebviewWindow};
use tokio::sync::oneshot;

pub(super) async fn apply<R: Runtime>(
    window: &WebviewWindow<R>,
    input: Input,
) -> Result<(), String> {
    let (sender, receiver) = oneshot::channel();
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    window
        .with_webview(move |view| {
            #[cfg(target_os = "linux")]
            let pointer = {
                use gtk::glib::translate::ToGlibPtr;
                let pointer: *mut webkit2gtk::ffi::WebKitWebView = view.inner().to_glib_none().0;
                pointer.cast::<std::ffi::c_void>()
            };
            #[cfg(target_os = "macos")]
            let pointer = view.inner().cast::<std::ffi::c_void>();
            // SAFETY: Tauri calls with_webview on the native UI thread; the live WebView
            // and borrowed text remain valid until the synchronous engine adapter returns.
            let result = unsafe { native(pointer, input) };
            let _ = sender.send(result);
        })
        .map_err(|error| error.to_string())?;
    #[cfg(target_os = "windows")]
    window
        .with_webview(move |view| {
            // SAFETY: this callback runs in the controller's UI apartment.
            match unsafe { view.controller().CoreWebView2() } {
                Ok(core) => windows::apply(core, input, sender),
                Err(error) => {
                    let _ = sender.send(Err(error.to_string()));
                }
            }
        })
        .map_err(|error| error.to_string())?;
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = (window, input);
        let _ = sender.send(Err(
            "Native composition is unavailable on this platform".into()
        ));
    }
    receiver.await.map_err(|error| error.to_string())?
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
unsafe fn native(view: *mut std::ffi::c_void, input: Input) -> Result<(), String> {
    use std::ffi::{CStr, CString, c_char, c_void};
    unsafe extern "C" {
        fn arti_ime_set(view: *mut c_void, text: *const c_char) -> *const c_char;
        fn arti_ime_commit(view: *mut c_void, text: *const c_char) -> *const c_char;
        fn arti_ime_cancel(view: *mut c_void) -> *const c_char;
        fn arti_edit_copy(view: *mut c_void) -> *const c_char;
        fn arti_edit_paste(view: *mut c_void) -> *const c_char;
    }
    let text =
        |value| CString::new(value).map_err(|_| "Native composition text contains NUL".to_owned());
    // SAFETY: the caller supplies a live WebView on its UI thread; the native adapters
    // consume borrowed C strings synchronously and return only static error strings.
    let error = unsafe {
        match input {
            Input::Set { text: value } => arti_ime_set(view, text(value)?.as_ptr()),
            Input::Commit { text: value } => arti_ime_commit(view, text(value)?.as_ptr()),
            Input::Cancel => arti_ime_cancel(view),
            Input::Copy => arti_edit_copy(view),
            Input::Paste => arti_edit_paste(view),
        }
    };
    if error.is_null() {
        Ok(())
    } else {
        // SAFETY: every error is an adapter-owned static NUL-terminated string.
        Err(unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned())
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::{Input, oneshot};
    use serde_json::json;
    use std::{cell::RefCell, rc::Rc};
    use webview2_com::{
        CallDevToolsProtocolMethodCompletedHandler, Microsoft::Web::WebView2::Win32::ICoreWebView2,
    };
    use windows_core::{HSTRING, PCWSTR};

    type Completion = Rc<RefCell<Option<oneshot::Sender<Result<(), String>>>>>;
    fn finish(sender: &Completion, result: Result<(), String>) {
        if let Some(sender) = sender.borrow_mut().take() {
            let _ = sender.send(result);
        }
    }
    pub(super) fn apply(
        core: ICoreWebView2,
        input: Input,
        sender: oneshot::Sender<Result<(), String>>,
    ) {
        let (method, parameters) = match input {
            Input::Set { text } => (
                "Input.imeSetComposition",
                json!({"selectionStart":text.encode_utf16().count(),"selectionEnd":text.encode_utf16().count(),"text":text}),
            ),
            Input::Commit { text } => ("Input.insertText", json!({"text":text})),
            Input::Cancel => (
                "Input.imeSetComposition",
                json!({"text":"","selectionStart":0,"selectionEnd":0}),
            ),
            Input::Copy => (
                "Input.dispatchKeyEvent",
                json!({"type":"keyDown","key":"c","code":"KeyC","modifiers":2,"windowsVirtualKeyCode":67,"commands":["Copy"]}),
            ),
            Input::Paste => (
                "Input.dispatchKeyEvent",
                json!({"type":"keyDown","key":"v","code":"KeyV","modifiers":2,"windowsVirtualKeyCode":86,"commands":["Paste"]}),
            ),
        };
        let sender = Rc::new(RefCell::new(Some(sender)));
        let completed = sender.clone();
        let callback =
            CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |status, _result| {
                finish(&completed, status.map_err(|error| error.to_string()));
                Ok(())
            }));
        let method = HSTRING::from(method);
        let parameters = HSTRING::from(parameters.to_string());
        // SAFETY: the engine copies these strings and retains the callback. The call
        // and callback both run in this WebView2 controller's UI apartment.
        if let Err(error) = unsafe {
            core.CallDevToolsProtocolMethod(
                PCWSTR(method.as_ptr()),
                PCWSTR(parameters.as_ptr()),
                &callback,
            )
        } {
            finish(&sender, Err(error.to_string()));
        }
    }
}
