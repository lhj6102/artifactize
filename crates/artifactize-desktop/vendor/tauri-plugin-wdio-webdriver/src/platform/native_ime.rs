//! Native input-method device for the embedded test driver. Only the test plugin exposes
//! it: composition follows the engine's IME path, not JavaScript event constructors or
//! Tauri application commands. The application still handles every real DOM event itself.
use serde::Deserialize;
use tauri::{Runtime, WebviewWindow};
use tokio::sync::oneshot;

use crate::server::response::WebDriverErrorResponse;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Input {
    Set {
        text: String,
    },
    Key {
        key: String,
        #[serde(default)]
        preedit: Option<String>,
        #[serde(default)]
        control: bool,
        #[serde(default)]
        meta: bool,
    },
    Commit {
        text: String,
    },
    Cancel,
}

pub(crate) async fn apply<R: Runtime>(
    window: &WebviewWindow<R>,
    input: Input,
) -> Result<(), WebDriverErrorResponse> {
    let valid_text = |text: &str| !text.contains('\0');
    let valid = match &input {
        Input::Set { text } | Input::Commit { text } => valid_text(text),
        Input::Key { key, preedit, .. } => {
            (key == "Escape" || key.len() == 1 && key.as_bytes()[0].is_ascii_lowercase())
                && preedit.as_ref().map_or(true, |text| valid_text(text))
        }
        Input::Cancel => true,
    };
    if !valid {
        return Err(WebDriverErrorResponse::invalid_argument(
            "Native IME text must not contain NUL; keys are lowercase ASCII letters or Escape",
        ));
    }
    let (sender, receiver) = oneshot::channel::<Result<(), String>>();
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    window
        .with_webview(move |webview| {
            #[cfg(target_os = "linux")]
            let pointer = {
                use gtk::glib::translate::ToGlibPtr;
                let pointer: *mut webkit2gtk::ffi::WebKitWebView = webview.inner().to_glib_none().0;
                pointer.cast::<std::ffi::c_void>()
            };
            #[cfg(target_os = "macos")]
            let pointer = webview.inner().cast::<std::ffi::c_void>();
            // SAFETY: Tauri invokes this closure on the native UI thread. The pointer is
            // the live WebView; the C/AppKit adapter consumes each borrowed string now.
            let result = unsafe { native(pointer, input) };
            let _ = sender.send(result);
        })
        .map_err(|error| WebDriverErrorResponse::unknown_error(&error.to_string()))?;
    #[cfg(target_os = "windows")]
    window
        .with_webview(move |webview| {
            use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;
            // SAFETY: these transparent wrappers reference the same version-stable COM
            // controller, and this callback runs in its UI apartment.
            let core: Result<ICoreWebView2, _> =
                unsafe { super::windows::rebind_controller(&webview).CoreWebView2() };
            match core {
                Ok(core) => windows::dispatch(core, input, sender),
                Err(error) => {
                    let _ = sender.send(Err(error.to_string()));
                }
            }
        })
        .map_err(|error| WebDriverErrorResponse::unknown_error(&error.to_string()))?;
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = (window, input);
        let _ = sender.send(Err(
            "Native desktop IME is unavailable on this platform".into()
        ));
    }
    receiver
        .await
        .map_err(|error| WebDriverErrorResponse::unknown_error(&error.to_string()))?
        .map_err(|error| WebDriverErrorResponse::unknown_error(&error))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
unsafe fn native(view: *mut std::ffi::c_void, input: Input) -> Result<(), String> {
    use std::ffi::{c_char, c_void, CStr, CString};
    extern "C" {
        fn wdio_ime_set(view: *mut c_void, text: *const c_char) -> *const c_char;
        fn wdio_ime_key(
            view: *mut c_void,
            key: *const c_char,
            control: i32,
            meta: i32,
            preedit: *const c_char,
        ) -> *const c_char;
        fn wdio_ime_commit(view: *mut c_void, text: *const c_char) -> *const c_char;
        fn wdio_ime_cancel(view: *mut c_void) -> *const c_char;
    }
    let cstring = |text| CString::new(text).map_err(|_| "Native IME input contains NUL".to_owned());
    // SAFETY: the caller supplies a live native WebView on its UI thread; the
    // borrowed C strings remain alive until these synchronous adapter calls return.
    let error = unsafe {
        match input {
            Input::Set { text } => wdio_ime_set(view, cstring(text)?.as_ptr()),
            Input::Key {
                key,
                preedit,
                control,
                meta,
            } => {
                let preedit = preedit.map(&cstring).transpose()?;
                wdio_ime_key(
                    view,
                    cstring(key)?.as_ptr(),
                    i32::from(control),
                    i32::from(meta),
                    preedit
                        .as_ref()
                        .map_or(std::ptr::null(), |text| text.as_ptr()),
                )
            }
            Input::Commit { text } => wdio_ime_commit(view, cstring(text)?.as_ptr()),
            Input::Cancel => wdio_ime_cancel(view),
        }
    };
    if error.is_null() {
        Ok(())
    } else {
        // SAFETY: errors are adapter-owned static NUL-terminated messages.
        Err(unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned())
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::{oneshot, Input};
    use serde_json::{json, Value};
    use std::{cell::RefCell, collections::VecDeque, rc::Rc};
    use webview2_com::{
        CallDevToolsProtocolMethodCompletedHandler, Microsoft::Web::WebView2::Win32::ICoreWebView2,
    };
    use windows_core::{HSTRING, PCWSTR};

    type Completion = Rc<RefCell<Option<oneshot::Sender<Result<(), String>>>>>;
    pub fn dispatch(
        core: ICoreWebView2,
        input: Input,
        sender: oneshot::Sender<Result<(), String>>,
    ) {
        let methods = match input {
            Input::Set { text } => vec![(
                "Input.imeSetComposition",
                json!({"selectionStart":text.encode_utf16().count(),"selectionEnd":text.encode_utf16().count(),"text":text}),
            )],
            Input::Commit { text } => vec![("Input.insertText", json!({"text":text}))],
            Input::Cancel => vec![(
                "Input.imeSetComposition",
                json!({"text":"","selectionStart":0,"selectionEnd":0}),
            )],
            Input::Key {
                key,
                preedit,
                control,
                meta,
            } => {
                let code = if key == "Escape" {
                    "Escape".to_owned()
                } else {
                    format!("Key{}", key.to_uppercase())
                };
                let modifiers = if control { 2 } else { 0 } | if meta { 4 } else { 0 };
                // VK_PROCESSKEY is the native IME's key code. The physical code stays
                // available to layout-independent shortcuts, so this tests their guard.
                let mut methods = ["keyDown", "keyUp"]
                    .map(|kind| {
                        (
                            "Input.dispatchKeyEvent",
                            json!({
                                "type":kind,"key":key,"code":code,"windowsVirtualKeyCode":229,
                                "nativeVirtualKeyCode":229,"modifiers":modifiers
                            }),
                        )
                    })
                    .to_vec();
                if let Some(text) = preedit {
                    methods.push(("Input.imeSetComposition", json!({"selectionStart":text.encode_utf16().count(),"selectionEnd":text.encode_utf16().count(),"text":text})));
                }
                methods
            }
        };
        next(core, methods.into(), Rc::new(RefCell::new(Some(sender))));
    }
    fn finish(sender: &Completion, result: Result<(), String>) {
        if let Some(sender) = sender.borrow_mut().take() {
            let _ = sender.send(result);
        }
    }
    fn next(core: ICoreWebView2, mut methods: VecDeque<(&'static str, Value)>, sender: Completion) {
        let Some((method, parameters)) = methods.pop_front() else {
            finish(&sender, Ok(()));
            return;
        };
        let remaining_core = core.clone();
        let completion = sender.clone();
        let handler =
            CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |status, _result| {
                match status {
                    Ok(()) => next(remaining_core, methods, completion),
                    Err(error) => finish(&completion, Err(error.to_string())),
                }
                Ok(())
            }));
        let method = HSTRING::from(method);
        let parameters = HSTRING::from(parameters.to_string());
        // SAFETY: all calls and callbacks stay in WebView2's UI apartment. WebView2
        // copies method parameters during the call and retains the COM callback.
        if let Err(error) = unsafe {
            core.CallDevToolsProtocolMethod(
                PCWSTR(method.as_ptr()),
                PCWSTR(parameters.as_ptr()),
                &handler,
            )
        } {
            finish(&sender, Err(error.to_string()));
        }
    }
}
