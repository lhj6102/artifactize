//! The Tool log as Tauri events: the library's runs log their lines to one channel per app,
//! the library's [`ToolLog`] of each review (or each Artifact whose tool runs without one)
//! takes them in, and this sends the logs they changed to the frontend. A log nobody has open
//! is dropped by the frontend; nothing is saved.
use crate::App;
use crate::browse::PathKey;
use crate::ids::{RequestKey, ToolRunKey};
use artifactize_core::review::log::{Entry, Events, Key, Stream, ToolLog};
use serde::Serialize;
use specta::Type;
use tauri::Manager;
use tauri_specta::Event as _;

/// Events taken in before one update at most, so a program printing fast reaches the
/// frontend in a few large updates rather than one per line.
const BATCH: usize = 256;

/// The Tool logs that changed, each whole.
#[derive(Debug, Clone, Serialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct HumanToolLog {
    pub logs: Vec<KeyedLog>,
}

/// Whose Tool log it is: a review's, or an Artifact's in a worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LogKey {
    Review {
        request: RequestKey,
    },
    Artifact {
        workspace: PathKey,
        branch: crate::browse::BranchKey,
        artifact: String,
    },
}

impl From<&Key> for LogKey {
    fn from(key: &Key) -> Self {
        match key {
            Key::Review(request) => Self::Review {
                request: request.clone().into(),
            },
            Key::Artifact {
                repo,
                branch,
                artifact,
            } => Self::Artifact {
                workspace: PathKey::new(repo),
                branch: crate::browse::BranchKey::new(branch),
                artifact: artifact.to_string(),
            },
        }
    }
}

/// One Tool log: one entry per run, in the order they started.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct KeyedLog {
    pub key: LogKey,
    pub entries: Vec<LogEntry>,
}

/// One tool run: its start line's facts, what it printed as it ran, and its end line once it
/// ended. A launched program's lines keep arriving after its tool returned.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// The run, unique while the app runs.
    pub run: ToolRunKey,
    /// Its start, at milliseconds since the Unix epoch.
    pub at: f64,
    pub operation: String,
    pub artifact: String,
    pub lines: Vec<LogText>,
    /// The run's end line, `■ exit 0` or how it ended.
    pub end: Option<LogText>,
}

/// One line of a run: what it printed, red when it went to stderr or the run failed.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LogText {
    pub text: String,
    pub error: bool,
}

fn entry(entry: &Entry) -> LogEntry {
    LogEntry {
        run: entry.run.into(),
        at: (entry.at.unix_timestamp_nanos() / 1_000_000) as f64,
        operation: entry.operation.clone(),
        artifact: entry.artifact.clone(),
        lines: entry
            .lines
            .iter()
            .map(|(stream, text)| LogText {
                text: text.clone(),
                error: *stream == Stream::Stderr,
            })
            .collect(),
        end: entry.end.as_ref().map(|end| LogText {
            text: end.text(),
            error: end.error(),
        }),
    }
}

/// `log` of `key`, as the frontend shows it.
pub fn keyed_log(key: &Key, log: &ToolLog) -> KeyedLog {
    KeyedLog {
        key: key.into(),
        entries: log.entries().iter().map(entry).collect(),
    }
}

/// Forward the app's Tool log to every window until the app exits.
pub fn forward<R: tauri::Runtime>(app: tauri::AppHandle<R>, mut events: Events) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<App>();
        loop {
            let first = tokio::select! {
                biased;
                () = state.human.wait_for_review_status() => {
                    let logs = state.human.clear_settled_logs(&state.launch.state).await;
                    if !logs.is_empty() && (HumanToolLog { logs }).emit(&app).is_err() {
                        return;
                    }
                    continue;
                }
                event = events.recv() => event,
            };
            let Some(first) = first else { return };
            let mut batch = vec![first];
            while batch.len() < BATCH
                && let Ok(event) = events.try_recv()
            {
                batch.push(event);
            }
            let starts_review = batch.iter().any(|event| {
                matches!(event.key, Key::Review(_))
                    && matches!(
                        event.kind,
                        artifactize_core::review::log::Kind::Start { .. }
                    )
            });
            let mut logs = state.human.logged(batch);
            // A delayed Start can be the first event since another process submitted.
            // Recheck then, and otherwise let the state watcher reconcile settlements.
            if starts_review {
                let cleared = state.human.clear_settled_logs(&state.launch.state).await;
                logs.retain(|log| !cleared.iter().any(|clear| clear.key == log.key));
                logs.extend(cleared);
            }
            // The app is closing; what runs still print is discarded.
            if !logs.is_empty() && (HumanToolLog { logs }).emit(&app).is_err() {
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use artifactize_core::review::log::{End, Kind, channel};
    use serde_json::json;

    #[test]
    fn artifact_logs_do_not_share_entries_when_only_the_branch_differs() {
        use artifactize_core::monitor::live::{Head, Place};
        let reviews = crate::human::Reviews::default();
        let mut events = reviews.take_events().unwrap();
        for name in ["main", "other"] {
            let branch = Place::Git {
                common: "repository".into(),
                head: Head::Branch(name.to_owned().try_into().unwrap()),
            };
            let run = reviews.log().start(
                Key::Artifact {
                    repo: "same-worktree".into(),
                    branch,
                    artifact: "notes".parse().unwrap(),
                },
                "read",
                "notes",
            );
            run.text(Stream::Stdout, name);
            run.end(End::Done);
        }
        let updates = reviews.logged(std::iter::from_fn(|| events.try_recv().ok()).collect());
        assert_eq!(updates.len(), 2);
        for (shown, name) in updates.into_iter().zip(["main", "other"]) {
            let shown = serde_json::to_value(shown).unwrap();
            assert_eq!(shown["key"]["branch"]["branch"], name);
            assert_eq!(shown["entries"].as_array().unwrap().len(), 1);
            assert_eq!(shown["entries"][0]["lines"][0]["text"], name);
        }
    }

    #[test]
    fn a_submission_clears_the_review_log_and_drops_delayed_tool_output() {
        let reviews = crate::human::Reviews::default();
        let mut events = reviews.take_events().unwrap();
        let request: artifactize_core::types::RequestId = "run-1-1".parse().unwrap();
        let launched = reviews.log().start(&request, "open", "notes");
        let visible = reviews.logged(vec![events.try_recv().unwrap()]);
        assert_eq!(visible[0].entries.len(), 1);
        let cleared = reviews.clear_log(&request).unwrap();
        assert_eq!(cleared.key, visible[0].key);
        assert!(cleared.entries.is_empty());
        launched.text(Stream::Stdout, "late output\n");
        let _queued = reviews.log().start(&request, "render", "notes");
        assert!(
            reviews
                .logged(vec![events.try_recv().unwrap(), events.try_recv().unwrap()])
                .is_empty()
        );
    }

    #[test]
    fn a_review_log_reaches_the_frontend_with_each_run_its_lines_and_end() {
        let (sink, mut events) = channel();
        let request: artifactize_core::types::RequestId = "run-1-1".parse().unwrap();
        let run = sink.start(&request, "open", "notes");
        run.text(Stream::Stdout, "out\n");
        run.text(Stream::Stderr, "err\n");
        run.end(End::Failed("gone".into()));
        let mut log = ToolLog::default();
        while let Ok(mut event) = events.try_recv() {
            // Check the wire timestamp against a fixed instant, not the machine's clock.
            if let Kind::Start { at, .. } = &mut event.kind {
                *at = time::OffsetDateTime::from_unix_timestamp_nanos(1_234_567_890).unwrap();
            }
            log.apply(event);
        }
        let sent = serde_json::to_value(keyed_log(&Key::from(&request), &log)).unwrap();
        assert_eq!(sent["key"], json!({"kind": "review", "request": "run-1-1"}));
        let [entry] = sent["entries"].as_array().unwrap().as_slice() else {
            panic!("{sent}");
        };
        assert_eq!(entry["at"], json!(1234.0));
        assert_eq!(
            (entry["operation"].clone(), entry["artifact"].clone()),
            (json!("open"), json!("notes"))
        );
        assert_eq!(
            entry["lines"],
            json!([{"text": "out", "error": false}, {"text": "err", "error": true}])
        );
        assert_eq!(
            entry["end"],
            json!({"text": "■ error: gone", "error": true})
        );
    }
}
