//! The Tool log as Tauri events: the library's runs log their lines to one channel per app,
//! the library's [`ToolLog`] of each review takes them in, and this sends the logs they
//! changed to the frontend. A log for a review nobody has open is dropped by the frontend;
//! nothing is saved.
use crate::App;
use crate::ids::{RequestKey, ToolRunKey};
use artifactize_core::review::log::{Entry, Events, Stream, ToolLog};
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
    pub logs: Vec<RequestLog>,
}

/// The Tool log of one review: one entry per run, in the order they started.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RequestLog {
    pub request: RequestKey,
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

/// `log` of the review `request`, as the frontend shows it.
pub fn request_log(request: RequestKey, log: &ToolLog) -> RequestLog {
    RequestLog {
        request,
        entries: log.entries().iter().map(entry).collect(),
    }
}

/// Forward the app's Tool log to every window until the app exits.
pub fn forward<R: tauri::Runtime>(app: tauri::AppHandle<R>, mut events: Events) {
    tauri::async_runtime::spawn(async move {
        while let Some(first) = events.recv().await {
            let mut batch = vec![first];
            while batch.len() < BATCH
                && let Ok(event) = events.try_recv()
            {
                batch.push(event);
            }
            let logs = app.state::<App>().human.logged(batch);
            // The app is closing; what runs still print is discarded.
            if (HumanToolLog { logs }).emit(&app).is_err() {
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
    fn a_review_log_reaches_the_frontend_with_each_run_its_lines_and_end() {
        let (sink, mut events) = channel();
        let request = "run-1-1".parse().unwrap();
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
        let sent = serde_json::to_value(request_log(request.into(), &log)).unwrap();
        assert_eq!(sent["request"], "run-1-1");
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
