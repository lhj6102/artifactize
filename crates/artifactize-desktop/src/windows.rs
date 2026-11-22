//! The app's windows beside the main one: an Artifact Viewer per viewer session (J-VIEWER) and
//! the pet overlay (J-PET), and the `viewer:` scheme that serves a plugin viewer's page into its
//! sandboxed frame with a policy of its own. Closing the main window closes the others.
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};

use tauri::{
    LogicalSize, Manager, PhysicalPosition, Runtime, WebviewUrl, WebviewWindowBuilder,
    http::{Request, Response, StatusCode, header},
};

use crate::{App, Failure, ids::RequestKey};

/// The main window's label, as `tauri.conf.json` declares it.
pub(crate) const MAIN: &str = "main";
/// The pet overlay's label.
const PET: &str = "pet";
/// A viewer window's label prefix; the label ends in its viewer session.
const VIEWER: &str = "viewer-";
/// The scheme a plugin viewer's page is served on.
pub(crate) const SCHEME: &str = "viewer";
/// A plugin viewer's page may run its own inline scripts and styles, and nothing it fetches:
/// what it reads comes through the viewer bridge.
const PAGE_POLICY: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; frame-src 'self' about: data: blob:; base-uri 'none'; form-action 'none'";
/// The pet overlay's size, in logical pixels, and its distance from the work area's corner.
const PET_SIZE: (f64, f64) = (340.0, 420.0);
const PET_MARGIN: f64 = 16.0;

/// What the commands ask of the windows.
enum Action {
    Viewer { session: u32, title: String },
    Pet(Option<String>),
    FocusMain,
    CloseMain,
}

type Act = Box<dyn Fn(&App, Action) -> Result<(), String> + Send + Sync>;

/// The windows, reached from the commands through the app's handle once the app is set up.
#[derive(Default)]
pub(crate) struct Windows {
    action: OnceLock<Act>,
    closing: AtomicBool,
}

impl Windows {
    pub(crate) fn attach<R: Runtime>(&self, handle: tauri::AppHandle<R>) {
        let _ = self.action.set(Box::new(move |app, action| {
            act(&handle, app, action).map_err(|error| error.to_string())
        }));
    }

    pub(crate) fn is_closing(&self) -> bool {
        self.closing.load(Ordering::Acquire)
    }

    fn act(&self, app: &App, action: Action) -> Result<(), Failure> {
        let act = self
            .action
            .get()
            .ok_or_else(|| Failure::Action("The app has no windows yet.".into()))?;
        act(app, action).map_err(Failure::Action)
    }
}

fn act<R: Runtime>(handle: &tauri::AppHandle<R>, app: &App, action: Action) -> tauri::Result<()> {
    match action {
        Action::Viewer { session, title } => viewer(handle, app, session, title),
        Action::Pet(request) => pet(handle, request),
        Action::CloseMain => handle
            .get_webview_window(MAIN)
            .map_or(Ok(()), |main| main.destroy()),
        Action::FocusMain => match handle.get_webview_window(MAIN) {
            Some(main) => {
                let _ = main.unminimize();
                main.set_focus()
            }
            None => Ok(()),
        },
    }
}

/// A window already showing the same Artifact target in the same viewer comes to the front, and
/// the new session ends; else a new window opens for the session.
pub(super) fn viewer<R: Runtime>(
    handle: &tauri::AppHandle<R>,
    app: &App,
    session: u32,
    title: String,
) -> tauri::Result<()> {
    // The same Artifact of the same worktree in the same viewer, whatever the windows' titles.
    let open = handle.webview_windows().into_iter().find(|(label, _)| {
        label
            .strip_prefix(VIEWER)
            .and_then(|shown| shown.parse().ok())
            .is_some_and(|shown| shown != session && app.viewers.same_place(shown, session))
    });
    if let Some((_, window)) = open {
        app.viewers.drop_session(session);
        let _ = window.unminimize();
        return window.set_focus();
    }
    WebviewWindowBuilder::new(
        handle,
        format!("{VIEWER}{session}"),
        WebviewUrl::App(format!("index.html?viewer={session}").into()),
    )
    .title(title)
    .inner_size(1100.0, 760.0)
    .min_inner_size(480.0, 320.0)
    .build()
    .map(|_| ())
}

/// The pet overlay for the active review `request`, at the bottom right of the main window's
/// screen's work area above every window, or hidden for `None`.
fn pet<R: Runtime>(handle: &tauri::AppHandle<R>, request: Option<String>) -> tauri::Result<()> {
    let shown = handle.get_webview_window(PET);
    let Some(request) = request else {
        return shown.map_or(Ok(()), |pet| pet.hide());
    };
    let pet = match shown {
        Some(pet) => {
            if let Some(url) = pet_destination(pet.url()?, &request) {
                pet.hide()?;
                pet.navigate(url)?;
            }
            pet
        }
        None => WebviewWindowBuilder::new(
            handle,
            PET,
            WebviewUrl::App(format!("index.html?pet={request}").into()),
        )
        .title("Artifactize review")
        .inner_size(PET_SIZE.0, PET_SIZE.1)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build()?,
    };
    let screen = handle
        .get_webview_window(MAIN)
        .and_then(|main| main.current_monitor().ok().flatten())
        .or_else(|| pet.primary_monitor().ok().flatten());
    if let Some(screen) = screen {
        let area = screen.work_area();
        let scale = screen.scale_factor();
        let x =
            area.position.x + area.size.width as i32 - ((PET_SIZE.0 + PET_MARGIN) * scale) as i32;
        let y =
            area.position.y + area.size.height as i32 - ((PET_SIZE.1 + PET_MARGIN) * scale) as i32;
        pet.set_size(LogicalSize::new(PET_SIZE.0, PET_SIZE.1))?;
        pet.set_position(PhysicalPosition::new(x, y))?;
    }
    pet.show()
}

/// Keep the existing review mounted unless this show names a different one. The URL
/// supplies the target even if the webview missed a change before installing its listeners.
fn pet_destination(mut url: tauri::Url, request: &str) -> Option<tauri::Url> {
    if url
        .query_pairs()
        .any(|(key, value)| key == "pet" && value == request)
    {
        return None;
    }
    url.query_pairs_mut().clear().append_pair("pet", request);
    Some(url)
}

/// Open the Artifact Viewer of viewer session `session`, titled `title`; one already showing the
/// same Artifact in the same viewer comes to the front instead.
#[tauri::command]
#[specta::specta]
pub async fn viewer_window(
    app: tauri::State<'_, App>,
    session: u32,
    title: String,
) -> Result<(), Failure> {
    let ready = async {
        if let Some(review) = app.viewers.pending_review(session) {
            let view = artifactize_core::store::read_request_as(
                &app.launch.state,
                review.as_str(),
                artifactize_core::store::Shown::Requests,
            )
            .await
            .map_err(Failure::Read)?;
            if let Some(reason) =
                artifactize_core::review::review_refusal(Some(&view), &app.launch.reviewer)
            {
                return Err(Failure::Action(reason));
            }
        }
        let prepared = app.viewers.get(session)?;
        crate::failure::joined(tokio::task::spawn_blocking(move || prepared.check_scope()).await)?
            .map_err(Failure::Read)
    }
    .await;
    // Take the handoff once, including on a failed read, to end its log and release the job.
    let pending = app
        .viewers
        .take_launch(session)
        .ok_or_else(|| Failure::Missing("Viewer launch is no longer available.".into()))?;
    let result = pending
        .open(|| ready.and_then(|()| app.windows.act(&app, Action::Viewer { session, title })));
    if result.is_err() {
        app.viewers.drop_session(session);
    }
    result
}

/// Show the pet overlay for the active review `request`, or hide it for `None`.
#[tauri::command]
#[specta::specta]
pub async fn pet_show(
    app: tauri::State<'_, App>,
    request: Option<RequestKey>,
) -> Result<(), Failure> {
    let request = request.map(|request| request.into_inner().as_str().to_owned());
    app.windows.act(&app, Action::Pet(request))
}

/// Bring the main window to the front: the pet overlay leads back to its review there.
#[tauri::command]
#[specta::specta]
pub async fn main_focus(app: tauri::State<'_, App>) -> Result<(), Failure> {
    app.windows.act(&app, Action::FocusMain)
}

/// Close only after the accepted quit has settled this app's owned Chat turns.
#[tauri::command]
#[specta::specta]
pub async fn main_close(app: tauri::State<'_, App>) -> Result<(), Failure> {
    if app.windows.closing.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    let result = async {
        let _settling = app.human.claim_gate.lock().await;
        if !crate::human::live_claims(&app).await?.is_empty() {
            return Err(Failure::Action(
                "Release the review claims before closing this window.".into(),
            ));
        }
        app.chat.stop_running().await.map_err(Failure::Action)?;
        app.close().await;
        app.windows.act(&app, Action::CloseMain)
    }
    .await;
    if result.is_err() {
        app.windows.closing.store(false, Ordering::Release);
    }
    result
}

/// Tauri still sends `tauri://close-requested` to the main webview. Its Leaving flow
/// checks claims and running Chat turns, then calls `main_close` to settle and destroy the
/// window. Prevent the native default even before the webview has installed its listener.
pub(crate) fn on_event<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    match event {
        tauri::WindowEvent::CloseRequested { api, .. } if window.label() == MAIN => {
            api.prevent_close();
        }
        tauri::WindowEvent::Destroyed => destroyed(window),
        _ => {}
    }
}

/// What a window's destruction ends: the main window takes the app's work and every other window
/// with it; a viewer window its viewer session.
fn destroyed<R: Runtime>(window: &tauri::Window<R>) {
    let label = window.label();
    if label == MAIN {
        for (other, webview) in window.app_handle().webview_windows() {
            if other != MAIN {
                let _ = webview.destroy();
            }
        }
    } else if let Some(session) = label
        .strip_prefix(VIEWER)
        .and_then(|session| session.parse().ok())
    {
        window.state::<App>().viewers.drop_session(session);
    }
}

/// Serve viewer session `/{session}`'s plugin page under its own policy.
pub(crate) fn page<R: Runtime>(
    handle: &tauri::AppHandle<R>,
    request: &Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let session = request
        .uri()
        .path()
        .trim_start_matches('/')
        .parse::<u32>()
        .ok();
    let html = session.and_then(|session| handle.state::<App>().viewers.web_page(session));
    let Some(html) = html else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(b"This Artifact Viewer is closed.".to_vec())
            .expect("a static response");
    };
    Response::builder()
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header("Content-Security-Policy", PAGE_POLICY)
        .body(html.into_bytes())
        .expect("a page response")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reused_pet_window_loads_the_review_it_is_shown_for() {
        let old = tauri::Url::parse("https://tauri.localhost/index.html?pet=run-a-1").unwrap();
        assert!(pet_destination(old.clone(), "run-a-1").is_none());
        let current = pet_destination(old, "run-b-2").unwrap();
        assert_eq!(
            current.as_str(),
            "https://tauri.localhost/index.html?pet=run-b-2"
        );
    }
}
