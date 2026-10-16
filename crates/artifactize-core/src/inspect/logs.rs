//! Bounded runtime evidence queries. Full captures stay on disk, never in a tool result.
use crate::{store, types::RequestId};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Cursor, Read},
    path::Path,
};

/// Keep two streams, including JSON escaping and MCP's text mirror, below 64 KiB.
const STREAM_BYTES: usize = 10 * 1024;
/// One query is a diagnostic excerpt, not a replacement for the saved log.
const MAX_LINES: usize = 200;
/// A short default tail supplies diagnostic context without filling an agent context.
const DEFAULT_TAIL: usize = 50;
/// Bound one unusually long line independently of the stream page.
const LINE_BYTES: usize = 2048;

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Stream {
    Stdout,
    Stderr,
    #[default]
    Both,
}

#[derive(Debug, Clone, Default, Deserialize, clap::Args)]
#[serde(deny_unknown_fields)]
pub struct LogQuery {
    /// Select stdout, stderr, or both (default).
    #[arg(long, value_enum, default_value = "both")]
    #[serde(default)]
    pub stream: Stream,
    /// Last N matching lines; defaults to 50 when no range is given (maximum 200).
    #[arg(long, conflicts_with_all = ["start", "end"])]
    pub tail: Option<usize>,
    /// First line, inclusive and one-based, in the original retained stream.
    #[arg(long, conflicts_with = "tail")]
    pub start: Option<usize>,
    /// Last line, inclusive and one-based.
    #[arg(long, conflicts_with = "tail")]
    pub end: Option<usize>,
    /// Literal, case-sensitive search; line numbers remain original.
    #[arg(long)]
    pub search: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Evidence {
    Complete,
    Partial,
    SavedAudit,
    Missing,
    NotRuntime,
}
#[derive(Debug, Serialize)]
pub struct Line {
    pub number: usize,
    pub text: String,
    pub clipped: bool,
}
#[derive(Debug, Serialize)]
pub struct LogExcerpt {
    pub stream: Stream,
    pub evidence: Evidence,
    pub note: String,
    pub lines: Vec<Line>,
    /// Continue a forward range/search at this original line; null for a tail query.
    pub next_start: Option<usize>,
}
#[derive(Debug, Serialize)]
pub struct Logs {
    pub request: RequestId,
    pub producer: RequestId,
    pub streams: Vec<LogExcerpt>,
}

pub async fn logs(state: &Path, id: &RequestId, query: LogQuery) -> Result<Logs, String> {
    validate(&query)?;
    let view = store::read_request(state, id).await?;
    // Reused/joined results point at the execution's original request, not this Run's output.
    let producer = view
        .request
        .provenance
        .as_ref()
        .map_or(id, |p| &p.request_id)
        .clone();
    let source = if view.request.origin.is_some() {
        None
    } else if producer == *id {
        Some(view.clone())
    } else {
        // Remote mirrors need not carry the original local request.
        store::read_request(state, &producer).await.ok()
    };
    let runtime = matches!(
        view.request.result(),
        Some(store::ExecutionResult::Runtime(_))
    ) || matches!(
        view.request.profile,
        crate::config::StoredProfile::Runtime { .. }
    );
    let state = state.to_path_buf();
    let id = id.clone();
    crate::task::joined(
        tokio::task::spawn_blocking(move || {
            let streams = match query.stream {
                Stream::Both => vec![Stream::Stdout, Stream::Stderr],
                stream => vec![stream],
            }
            .into_iter()
            .map(|stream| excerpt(&state, source.as_ref(), &view, runtime, stream, &query))
            .collect::<Result<_, _>>()?;
            Ok(Logs {
                request: id,
                producer,
                streams,
            })
        })
        .await,
    )
    .map_err(|e| e.to_string())?
}

fn validate(query: &LogQuery) -> Result<(), String> {
    if query.tail.is_some_and(|n| n == 0 || n > MAX_LINES)
        || query.start == Some(0)
        || query.end == Some(0)
        || query.start.zip(query.end).is_some_and(|(a, b)| a > b)
        || (query.tail.is_some() && (query.start.is_some() || query.end.is_some()))
        || query
            .search
            .as_ref()
            .is_some_and(|s| s.is_empty() || s.len() > LINE_BYTES)
    {
        return Err("Use positive line numbers, tail 1–200, an ordered range, and a nonempty search up to 2048 bytes; tail and range are exclusive.".into());
    }
    Ok(())
}

fn excerpt(
    state: &Path,
    source: Option<&store::RequestView>,
    view: &store::RequestView,
    runtime: bool,
    stream: Stream,
    query: &LogQuery,
) -> Result<LogExcerpt, String> {
    let absent = |evidence, note: &str| LogExcerpt {
        stream,
        evidence,
        note: note.into(),
        lines: vec![],
        next_start: None,
    };
    if !runtime {
        return Ok(absent(
            Evidence::NotRuntime,
            "No runtime stdout/stderr; inspect request/session evidence.",
        ));
    }
    let name = match stream {
        Stream::Stdout => "stdout",
        Stream::Stderr => "stderr",
        Stream::Both => unreachable!(),
    };
    if let Some(directory) = source.and_then(|v| v.request.run_dir.as_ref()) {
        let path = directory.join(format!("runtime.{name}"));
        // Saved paths are not authority to read outside this state's Run output.
        if path.exists() {
            let root =
                crate::platform::canonicalize(&state.join("runs")).map_err(|e| e.to_string())?;
            let canonical = crate::platform::canonicalize(&path).map_err(|e| e.to_string())?;
            if !canonical.starts_with(&root) {
                return Err("Runtime log is outside saved Run output.".into());
            }
            let file = std::fs::File::open(&canonical).map_err(|e| e.to_string())?;
            let complete = path.with_extension(format!("{name}.complete")).is_file();
            // A live producer may keep appending: query only the bytes present at open.
            let length = file.metadata().map_err(|e| e.to_string())?.len();
            let (lines, next_start) = select(BufReader::new(file.take(length)), query)?;
            return Ok(LogExcerpt {
                stream,
                evidence: if complete {
                    Evidence::Complete
                } else {
                    Evidence::Partial
                },
                note: if complete {
                    "Full local stream retained until Run output is pruned."
                } else {
                    "Capture is active or ended without confirmed EOF; retained bytes only."
                }
                .into(),
                lines,
                next_start,
            });
        }
    }
    let result = view
        .request
        .result()
        .or_else(|| source.and_then(|v| v.request.result()));
    if let Some(store::ExecutionResult::Runtime(result)) = result {
        let audit = match stream {
            Stream::Stdout => result.stdout.value(),
            _ => result.stderr.value(),
        };
        if let Some(text) = audit {
            let (lines, next_start) = select(Cursor::new(text.as_bytes()), query)?;
            return Ok(LogExcerpt { stream, evidence: Evidence::SavedAudit,
                note: match result.truncated.value() {
                    Some(false) => "Full capture unavailable (older, remote or pruned); saved cleaned audit reports no truncation.",
                    _ => "Full capture unavailable (older, remote or pruned); audit may contain only the first 128 KiB. Missing tail cannot be recovered.",
                }.into(), lines, next_start });
        }
    }
    Ok(absent(
        Evidence::Missing,
        "No retained stream or saved audit: not started, remote, older or pruned evidence.",
    ))
}

/// Scan bounded chunks even for newline-free output. Search overlaps chunks so a
/// literal crossing a buffer boundary is found without retaining the entire line.
fn select(
    mut reader: impl BufRead,
    query: &LogQuery,
) -> Result<(Vec<Line>, Option<usize>), String> {
    let tail = query
        .tail
        .or_else(|| (query.start.is_none() && query.end.is_none()).then_some(DEFAULT_TAIL));
    let mut selected = VecDeque::new();
    let mut bytes = 0;
    let mut number = 0;
    loop {
        let mut text = Vec::new();
        let mut suffix = VecDeque::new();
        let mut overlap = Vec::new();
        let mut length = 0;
        let mut matched = query.search.is_none();
        loop {
            let chunk = reader.fill_buf().map_err(|e| e.to_string())?;
            if chunk.is_empty() {
                break;
            }
            let end = chunk.iter().position(|&b| b == b'\n').map(|n| n + 1);
            let chunk = &chunk[..end.unwrap_or(chunk.len())];
            length += chunk.len();
            text.extend_from_slice(
                &chunk[..chunk.len().min(LINE_BYTES.saturating_sub(text.len()))],
            );
            suffix.extend(chunk.iter().copied());
            suffix.drain(..suffix.len().saturating_sub(LINE_BYTES / 2));
            if let Some(search) = &query.search {
                overlap.extend_from_slice(chunk);
                matched |= overlap
                    .windows(search.len())
                    .any(|w| w == search.as_bytes());
                let keep = overlap.len().saturating_sub(search.len().saturating_sub(1));
                overlap.drain(..keep);
            }
            let consumed = chunk.len();
            reader.consume(consumed);
            if end.is_some() {
                break;
            }
        }
        if length == 0 {
            break;
        }
        number += 1;
        if query.end.is_some_and(|end| number > end) {
            break;
        }
        if number < query.start.unwrap_or(1) || !matched {
            continue;
        }
        let text = if length > LINE_BYTES {
            text.truncate(LINE_BYTES / 2);
            text.extend_from_slice(b"\n[... line clipped ...]\n");
            text.extend(suffix);
            text
        } else {
            text
        };
        let text = String::from_utf8_lossy(&crate::runtime::clean_output(&text))
            .trim_end_matches(['\r', '\n'])
            .to_owned();
        let line = Line {
            number,
            text,
            clipped: length > LINE_BYTES,
        };
        let size = serde_json::to_vec(&line).map_err(|e| e.to_string())?.len();
        if tail.is_none() && (selected.len() == MAX_LINES || bytes + size > STREAM_BYTES) {
            return Ok((selected.into(), Some(number)));
        }
        bytes += size;
        selected.push_back(line);
        while selected.len() > tail.unwrap_or(MAX_LINES) || bytes > STREAM_BYTES {
            let removed = selected.pop_front().expect("nonempty page");
            bytes -= serde_json::to_vec(&removed).expect("line serializes").len();
        }
    }
    Ok((selected.into(), None))
}
