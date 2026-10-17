//! The Tool log as Tauri events: the library's runs log their lines to one channel per app,
//! and this forwards them to the frontend in batches. A line for a review nobody has open is
//! dropped by the frontend; nothing is saved.
use crate::ids::{RequestKey, ToolRunKey};
use artifactize_core::review::log::{End, Event, Events, Kind, Stream};
use serde::Serialize;
use specta::Type;
use tauri_specta::Event as _;

/// Lines sent in one event at most, so a program printing fast reaches the frontend in a few
/// large updates rather than one per line.
const BATCH: usize = 256;

/// Lines of Human tool runs, oldest first.
#[derive(Debug, Clone, Serialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct HumanToolLog {
    pub lines: Vec<LogLine>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    /// The review the run belongs to.
    pub request: RequestKey,
    /// The run, unique while the app runs: later lines find their own entry.
    pub run: ToolRunKey,
    pub line: Line,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Line {
    /// A run started, at milliseconds since the Unix epoch.
    Start {
        at: f64,
        operation: String,
        artifact: String,
    },
    Stdout {
        text: String,
    },
    Stderr {
        text: String,
    },
    /// The run's end line, `■ exit 0` or how it ended.
    End {
        text: String,
        error: bool,
    },
}

fn line(event: Event) -> LogLine {
    let end = |end: End| Line::End {
        text: end.text(),
        error: end.error(),
    };
    LogLine {
        request: event.request.into(),
        run: event.run.into(),
        line: match event.kind {
            Kind::Start {
                at,
                operation,
                artifact,
            } => Line::Start {
                at: (at.unix_timestamp_nanos() / 1_000_000) as f64,
                operation,
                artifact,
            },
            Kind::Line {
                stream: Stream::Stdout,
                text,
            } => Line::Stdout { text },
            Kind::Line {
                stream: Stream::Stderr,
                text,
            } => Line::Stderr { text },
            Kind::End(ended) => end(ended),
        },
    }
}

/// Forward the app's Tool log to every window until the app exits.
pub fn forward<R: tauri::Runtime>(app: tauri::AppHandle<R>, mut events: Events) {
    tauri::async_runtime::spawn(async move {
        while let Some(first) = events.recv().await {
            let mut lines = vec![line(first)];
            while lines.len() < BATCH
                && let Ok(event) = events.try_recv()
            {
                lines.push(line(event));
            }
            // The app is closing; what runs still print is discarded.
            let batch = HumanToolLog { lines };
            if batch.emit(&app).is_err() {
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use artifactize_core::review::log::{Stream, channel};
    use serde_json::json;

    #[test]
    fn each_logged_line_reaches_the_frontend_with_its_review_run_and_kind() {
        let (sink, mut events) = channel();
        let request = "run-1-1".parse().unwrap();
        let run = sink.start(&request, "open", "notes");
        run.text(Stream::Stdout, "out\n");
        run.text(Stream::Stderr, "err\n");
        run.end(End::Failed("gone".into()));
        let mut started = events.try_recv().unwrap();
        let Kind::Start { at, .. } = &mut started.kind else {
            panic!("{started:?}");
        };
        // Check the wire timestamp against a fixed instant, not the machine's clock.
        *at = time::OffsetDateTime::from_unix_timestamp_nanos(1_234_567_890).unwrap();
        let lines: Vec<_> = std::iter::once(started)
            .chain(std::iter::from_fn(|| events.try_recv().ok()))
            .map(|event| serde_json::to_value(line(event)).unwrap())
            .collect();
        let [start, out, err, end] = lines.as_slice() else {
            panic!("{lines:?}");
        };
        assert_eq!(start["request"], "run-1-1");
        assert_eq!(start["run"], out["run"]);
        assert_eq!(start["line"]["kind"], "start");
        assert_eq!(start["line"]["at"], json!(1234.0));
        assert_eq!(
            (
                start["line"]["operation"].clone(),
                start["line"]["artifact"].clone()
            ),
            (json!("open"), json!("notes"))
        );
        assert_eq!(out["line"], json!({"kind": "stdout", "text": "out"}));
        assert_eq!(err["line"], json!({"kind": "stderr", "text": "err"}));
        assert_eq!(
            end["line"],
            json!({"kind": "end", "text": "■ error: gone", "error": true})
        );
    }
}
