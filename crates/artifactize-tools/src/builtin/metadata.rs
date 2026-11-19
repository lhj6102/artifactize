//! Provider-facing descriptions and argument schemas.

use crate::{Builtin, scope};
use serde_json::{Value, json};

use super::{DEFAULT_READ_LINES, FIRST_LINE, MAX_PATTERN_UNITS, MAX_READ_LINES, MAX_RESULTS};

pub fn description(builtin: Builtin) -> &'static str {
    match builtin {
        Builtin::Graph => {
            "Show the current project Artifact graph, including evals and input-to-consumer relations."
        }
        Builtin::Section => "Read a Markdown section in {artifactName} by its exact heading.",
        Builtin::Help => "Read a program's --help output.",
        Builtin::Open => "Open a scoped file or URL in its default application.",
        Builtin::Read => {
            "Read UTF-8 complete lines in {artifactName}. path is a relative logical file path, including child paths. Paths are relative to {artifactName} itself: use \"notes.md\", not \"{artifactName}/notes.md\". offset is 1-based (default 1); limit defaults to 80, maximum 500. Returns numbered lines preserving LF/CRLF/BOM, up to 64 KiB, startLine/endLine/lineCount, totalLines when known, truncated and nextOffset. No symlinks or binary text."
        }
        Builtin::List => {
            "List {artifactName} at a relative logical path (default root). Paths are relative to {artifactName} itself: use \"notes.md\", not \"{artifactName}/notes.md\". Sorted entries include name, path and kind. offset is 0-based; limit defaults to and cannot exceed 200. Returns totalEntries, truncated and nextOffset. Symlinks and special files are listed but never followed. Directories are limited to 10,000 entries."
        }
        Builtin::Glob => {
            "Find files in {artifactName} with a relative glob pattern (* within a path component, ** across directories). path is a relative logical directory, default root; patterns are relative to it. Paths are relative to {artifactName} itself: use \"notes.md\", not \"{artifactName}/notes.md\". Returns up to 200 sorted logical paths and truncated. Includes hidden files; ignores no files by git rules. Symlinks/special files are skipped, and traversal is limited to 10,000 entries."
        }
        Builtin::Grep => {
            "Search UTF-8 files in {artifactName} with a Rust regex, one match per matching line. path is a relative logical file or directory (default root); glob filters paths relative to it. Paths are relative to {artifactName} itself: use \"notes.md\", not \"{artifactName}/notes.md\". caseInsensitive defaults to false; maxResults defaults to and cannot exceed 200. Returns matches with path, line and text, plus truncated. Skips binary/invalid UTF-8 and symlinks; includes hidden files. Caps: 10,000 traversed entries, 8 MiB per file, 64 MiB searched, 512 KiB result. Skipped oversized files or bounded results set truncated."
        }
        Builtin::ViewImage => {
            "View one image in {artifactName} at a relative logical file path, including child paths. Paths are relative to {artifactName} itself: use \"image.png\", not \"{artifactName}/image.png\". Detects PNG, JPEG or WebP by bytes, not extension; returns an embedded image block. Requires a nonempty regular file up to 4 MiB; no symlinks, GIF, BMP or animated PNG."
        }
    }
}

pub fn input_schema(builtin: Builtin) -> Value {
    let path = json!({
        "type":"string",
        "maxLength":scope::MAX_PATH_UNITS,
        "description":"Logical path relative to the tool's Artifact, without the Artifact's name; no absolute paths, dot components, backslashes or symlinks. Empty means the root.",
    });
    let pattern = json!({"type":"string","minLength":1,"maxLength":MAX_PATTERN_UNITS});
    let integer =
        |min, max, default| json!({"type":"integer","minimum":min,"maximum":max,"default":default});
    let (properties, required) = match builtin {
        Builtin::Read => (
            json!({
                "path":path,
                "offset":integer(FIRST_LINE as u64, crate::MAX_SAFE_JSON_INTEGER, FIRST_LINE as u64),
                "limit":integer(1, MAX_READ_LINES as u64, DEFAULT_READ_LINES as u64),
            }),
            vec!["path"],
        ),
        Builtin::List => (
            json!({
                "path":path,
                "offset":integer(0, crate::MAX_SAFE_JSON_INTEGER, 0),
                "limit":integer(1, MAX_RESULTS as u64, MAX_RESULTS as u64),
            }),
            vec![],
        ),
        Builtin::Graph => (json!({}), vec![]),
        Builtin::Glob => (json!({"pattern":pattern,"path":path}), vec!["pattern"]),
        Builtin::Grep => (
            json!({
                "pattern":pattern,
                "path":path,
                "glob":pattern,
                "caseInsensitive":{"type":"boolean","default":false},
                "maxResults":integer(1, MAX_RESULTS as u64, MAX_RESULTS as u64),
            }),
            vec!["pattern"],
        ),
        Builtin::ViewImage => (json!({"path":path}), vec!["path"]),
        Builtin::Section => (json!({"heading":pattern}), vec!["heading"]),
        Builtin::Help | Builtin::Open => (json!({}), vec![]),
    };
    json!({
        "type":"object",
        "properties":properties,
        "required":required,
        "additionalProperties":false,
    })
}
