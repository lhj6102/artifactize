//! The artifactize desktop app: every live branch, its Runs and Current, and the waiting Human
//! reviews in one Tauri window.
//!
//! The Rust side owns no state of its own beyond UI snapshots: it reads through the library's
//! [`artifactize_core::monitor::live::Loader`] and acts through the same
//! [`artifactize_core::review::Job`]s as the terminal review. Commands and events are typed with
//! tauri-specta; [`bindings`] is the single list of what the frontend may call.

mod browse;
mod chat;
mod definitions;
mod detail;
#[cfg(feature = "e2e")]
mod e2e_input;
mod failure;
mod generation;
mod graph;
mod human;
mod ids;
mod launch;
mod release;
mod request;
mod settings;
mod status;
mod tools;
mod transcript;
mod watch;

mod viewer;
mod windows;

pub use failure::Failure;
pub use ids::{JobId, SignInId, TranscriptHandle};
pub use launch::{Launch, Mode};

use specta_typescript::Typescript;
use std::{path::Path, sync::Arc};
use tauri::Manager;
use tauri_specta::{Builder, collect_commands, collect_events};

/// Where the generated TypeScript bindings live, relative to this crate.
pub const BINDINGS: &str = "ui/src/ipc/bindings.ts";

/// Every command and event the frontend can use, with their TypeScript types.
pub fn bindings<R: tauri::Runtime>() -> Builder<R> {
    Builder::new()
        .commands(collect_commands![
            launch::launch,
            chat::chat_create,
            chat::chat_set_model,
            chat::chat_set_view,
            chat::chat_latest_for_branch,
            chat::chat_default_model,
            chat::chat_list,
            chat::chat_branches,
            chat::chat_mark_seen,
            chat::chat_open,
            chat::chat_send,
            chat::chat_stop,
            chat::chat_open_link,
            browse::browse,
            browse::redraw,
            browse::pane_complete,
            detail::detail,
            detail::detail_row,
            human::human_open,
            human::search::human_open_reference,
            human::search::human_search,
            human::search::human_search_complete,
            human::search::human_waiting_count,
            human::human_form,
            human::draft::human_draft,
            human::draft::human_draft_edit,
            human::draft::human_draft_remove_point,
            human::draft::human_draft_submit,
            human::human_edit_field,
            human::human_add_item,
            human::human_remove_item,
            human::human_claim,
            human::human_run,
            human::human_cancel,
            human::human_submit,
            human::human_release,
            graph::graph,
            graph::neighbours,
            graph::find::find,
            graph::find::favorites,
            graph::find::starred_artifacts,
            graph::find::star_artifact,
            graph::current::current_scan,
            graph::current::current_view,
            definitions::definitions,
            definitions::definition_edit,
            definitions::definition_check,
            tools::artifact_tools,
            tools::artifact_tool_run,
            viewer::viewer_context,
            viewer::viewer_entry,
            viewer::viewer_files,
            viewer::viewer_read_text,
            viewer::viewer_declarations,
            viewer::viewer_close,
            viewer::viewer_point,
            viewer::active_review_set,
            windows::viewer_window,
            windows::pet_show,
            windows::main_focus,
            windows::main_close,
            request::request_detail,
            release::release_notice,
            release::release_open,
            human::human_taken,
            human::human_release_taken,
            transcript::transcript_open,
            transcript::transcript_window,
            transcript::transcript_close,
            settings::providers_list,
            settings::provider_key_save,
            settings::provider_key_remove,
            settings::provider_custom_save,
            settings::provider_custom_remove,
            settings::codex_status,
            settings::codex_login_start,
            settings::codex_login_prompt,
            settings::codex_login_wait,
            settings::codex_login_paste,
            settings::codex_login_cancel,
            settings::codex_logout,
            settings::antigravity_status,
            settings::antigravity_login_start,
            settings::antigravity_login_prompt,
            settings::antigravity_login_wait,
            settings::antigravity_login_paste,
            settings::antigravity_login_cancel,
            settings::antigravity_logout,
            settings::claude_agent_status,
            settings::claude_agent_login_start,
            settings::claude_agent_login_prompt,
            settings::claude_agent_login_wait,
            settings::claude_agent_login_paste,
            settings::claude_agent_login_cancel,
            settings::claude_agent_logout,
            settings::models_list,
            settings::remote_status,
            settings::remote_login,
            settings::remote_logout,
            settings::remote_push,
            settings::signoff_grace,
            settings::signoff_grace_set,
        ])
        .events(collect_events![
            watch::StateChanged,
            watch::RunFinished,
            viewer::ActiveReviewChanged,
            watch::SessionChanged,
            human::log::HumanToolLog,
            human::draft::HumanDraftChanged,
            graph::current::CurrentChanged
        ])
        .constant("PRECEDENCE", graph::precedence())
        .constant(
            "REVIEW_DEFAULT_QUERY",
            artifactize_core::review::search::DEFAULT,
        )
}

/// Write the TypeScript bindings to `path`.
pub fn export(path: &Path) -> Result<(), String> {
    bindings::<tauri::Wry>()
        .export(
            Typescript::default().header("// Generated by artifactize-desktop; do not edit.\n"),
            path,
        )
        .map_err(|error| error.to_string())
}

/// The shared state behind every command, one per app.
pub struct App {
    pub launch: Launch,
    chat: artifactize_core::chat::Chat,
    chat_worktrees: watch::ChatWorktrees,
    browse: browse::Browse,
    human: human::Reviews,
    currents: Arc<graph::current::Currents>,
    transcripts: transcript::Transcripts,
    settings: settings::Settings,
    generation: generation::Generation,
    sites: tools::Sites,
    viewers: viewer::Sessions,
    windows: windows::Windows,
}

impl App {
    pub fn new(launch: Launch) -> Self {
        Self {
            browse: browse::Browse::new(launch.repo.clone()),
            chat: artifactize_core::chat::Chat::discover(launch.state.clone()),
            chat_worktrees: watch::ChatWorktrees::default(),
            launch,
            human: human::Reviews::default(),
            currents: Arc::default(),
            transcripts: transcript::Transcripts::default(),
            settings: settings::Settings::default(),
            generation: generation::Generation::default(),
            sites: tools::Sites::default(),
            viewers: viewer::Sessions::default(),
            windows: windows::Windows::default(),
        }
    }

    /// Current belongs to a live checkout, not to the presence of a Run in Browse.
    async fn current_workspace(
        &self,
        branch: &browse::BranchKey,
    ) -> Result<std::path::PathBuf, Failure> {
        let place = branch.to_place();
        if let Some(worktree) = self
            .chat
            .current_worktree(&place)
            .await
            .map_err(Failure::Read)?
        {
            return Ok(worktree);
        }
        let workspace = self.browse.branch(&place)?.workspace;
        place
            .checked_workspace(workspace)
            .await
            .map_err(Failure::Missing)
    }

    /// Note the state origin a read found and return the state's generation. A replaced state
    /// (the monitor's [`Origin::replaces`]) drops the Human caches, claims taken here, running
    /// jobs and Current scans of the old one.
    ///
    /// [`Origin::replaces`]: artifactize_core::monitor::Origin::replaces
    fn observe(&self, origin: artifactize_core::monitor::Origin) -> u32 {
        self.generation.observe(origin, || {
            self.human.reset();
            self.currents.reset();
            self.viewers.reset();
        })
    }

    /// A closing window cancels running Human tools, Current scans and a Codex sign-in in
    /// progress, as quitting the terminal UI does; the returned future also waits for Chat
    /// to record cancellation and release any active turn it owned.
    pub fn close(&self) -> impl std::future::Future<Output = ()> + use<> {
        self.human.cancel_all();
        let chat = self.chat.close();
        self.viewers.reset();
        let scans = self.currents.stop();
        let sign_ins = self.settings.close();
        async move {
            tokio::join!(chat, scans, sign_ins);
        }
    }
}

/// Build the Tauri app around `launch` on `builder`'s runtime; the caller runs it.
pub fn app<R: tauri::Runtime>(builder: tauri::Builder<R>, launch: Launch) -> tauri::Builder<R> {
    let specta = bindings::<R>();
    let state = launch.state.clone();
    let commands = specta.invoke_handler();
    builder
        .manage(App::new(launch))
        .invoke_handler(move |invoke| {
            #[cfg(feature = "e2e")]
            if invoke.message.command() == "e2e_input" {
                return e2e_input::handle(invoke);
            }
            commands(invoke)
        })
        .setup(move |app| {
            specta.mount_events(app);
            app.state::<App>().viewers.forward(app.handle().clone());
            app.state::<App>().windows.attach(app.handle().clone());
            let command_state = state.clone();
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(error) =
                    artifactize_core::platform::desktop_commands::setup(&command_state)
                {
                    eprintln!("cannot make the artifactize command available: {error}");
                }
                eprintln!("desktop command setup complete");
            });
            watch::spawn(app.handle().clone(), state);
            if let Some(events) = app.state::<App>().human.take_events() {
                human::log::forward(app.handle().clone(), events);
            }
            if let Some(changes) = app.state::<App>().currents.take_changes() {
                graph::current::forward(app.handle().clone(), changes);
            }
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol(
            windows::SCHEME,
            |context, request, responder| {
                responder.respond(windows::page(
                    context.app_handle(),
                    context.webview_label(),
                    &request,
                ));
            },
        )
        .on_window_event(windows::on_event)
}
