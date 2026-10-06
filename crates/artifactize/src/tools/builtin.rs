//! Scoped read-only Agent tools.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
};

use globset::{GlobBuilder, GlobMatcher};
use regex::RegexBuilder;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    config::Builtin,
    platform::{self, FileKind},
    scope::{self, OpenError, Scope, ScopedPath},
};

use super::{Content, ToolResult, image};

const READ_BYTES: usize = 64 * 1024;
const RESULT_BYTES: usize = 512 * 1024;
const MAX_RESULTS: usize = 200;
const MAX_ENTRIES: usize = 10_000;
const SEARCH_FILE_BYTES: usize = 8 * 1024 * 1024;
const SEARCH_BYTES: usize = 64 * 1024 * 1024;

pub(crate) fn description(builtin: Builtin) -> &'static str {
    match builtin {
        Builtin::Read => {
            "Read UTF-8 complete lines in {artifactName}. path is a relative logical file path, including child/mount paths. Paths are relative to {artifactName} itself: use \"notes.md\", not \"{artifactName}/notes.md\". offset is 1-based (default 1); limit defaults to 80, maximum 500. Returns numbered lines preserving LF/CRLF/BOM, up to 64 KiB, startLine/endLine/lineCount, totalLines when known, truncated and nextOffset. No symlinks or binary text."
        }
        Builtin::List => {
            "List {artifactName} at a relative logical path (default root). Paths are relative to {artifactName} itself: use \"notes.md\", not \"{artifactName}/notes.md\". Sorted entries include name, path and kind; mounts and family instance catalogs are included. offset is 0-based; limit defaults to and cannot exceed 200. Returns totalEntries, truncated and nextOffset. Symlinks and special files are listed but never followed. Directories are limited to 10,000 entries."
        }
        Builtin::Glob => {
            "Find files in {artifactName} with a relative glob pattern (* within a path component, ** across directories). path is a relative logical directory, default root; patterns are relative to it. Paths are relative to {artifactName} itself: use \"notes.md\", not \"{artifactName}/notes.md\". Returns up to 200 sorted logical paths and truncated. Includes hidden files, mounts and family instance paths; ignores no files by git rules. Symlinks/special files are skipped, mount cycles are not repeated, and traversal is limited to 10,000 entries."
        }
        Builtin::Grep => {
            "Search UTF-8 files in {artifactName} with a Rust regex, one match per matching line. path is a relative logical file or directory (default root); glob filters paths relative to it. Paths are relative to {artifactName} itself: use \"notes.md\", not \"{artifactName}/notes.md\". caseInsensitive defaults to false; maxResults defaults to and cannot exceed 200. Returns matches with path, line and text, plus truncated. Skips binary/invalid UTF-8 and symlinks; includes hidden files. Caps: 10,000 traversed entries, 8 MiB per file, 64 MiB searched, 512 KiB result. Skipped oversized files or bounded results set truncated."
        }
        Builtin::ViewImage => {
            "View one image in {artifactName} at a relative logical file path, including child/mount paths. Paths are relative to {artifactName} itself: use \"image.png\", not \"{artifactName}/image.png\". Detects PNG, JPEG or WebP by bytes, not extension; returns an embedded image block. Requires a nonempty regular file up to 4 MiB; no symlinks, GIF, BMP or animated PNG."
        }
    }
}

pub(crate) fn input_schema(builtin: Builtin) -> Value {
    let path = json!({"type":"string","maxLength":4096,"description":"Logical path relative to the tool's Artifact, without the Artifact's name; no absolute paths, dot components, backslashes or symlinks. Empty means the root."});
    let pattern = json!({"type":"string","minLength":1,"maxLength":4096});
    let integer =
        |min, max, default| json!({"type":"integer","minimum":min,"maximum":max,"default":default});
    let (properties, required) = match builtin {
        Builtin::Read => (
            json!({"path":path,"offset":integer(1, 9_007_199_254_740_991u64, 1),"limit":integer(1,500,80)}),
            vec!["path"],
        ),
        Builtin::List => (
            json!({"path":path,"offset":integer(0,9_007_199_254_740_991,0),"limit":integer(1,200,200)}),
            vec![],
        ),
        Builtin::Glob => (json!({"pattern":pattern,"path":path}), vec!["pattern"]),
        Builtin::Grep => (
            json!({"pattern":pattern,"path":path,"glob":pattern,"caseInsensitive":{"type":"boolean","default":false},"maxResults":integer(1,200,200)}),
            vec!["pattern"],
        ),
        Builtin::ViewImage => (json!({"path":path}), vec!["path"]),
    };
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

pub(super) fn call(
    builtin: Builtin,
    root: &Path,
    scope: &Scope<'_>,
    owner: &str,
    args: &Value,
    cancellation: &CancellationToken,
) -> ToolResult {
    let reader = Reader {
        root,
        scope,
        owner,
        cancellation,
    };
    let path = args["path"].as_str().unwrap_or("");
    let result = (|| {
        reader.check_cancelled()?;
        let data = match builtin {
            Builtin::Read => {
                reader.read(path, number(args, "offset", 1), number(args, "limit", 80))
            }
            Builtin::List => {
                reader.list(path, number(args, "offset", 0), number(args, "limit", 200))
            }
            Builtin::Glob => reader.glob(path, args["pattern"].as_str().unwrap()),
            Builtin::Grep => reader.grep(path, args),
            Builtin::ViewImage => return reader.view_image(path),
        }?;
        if serde_json::to_vec(&data).unwrap().len() > RESULT_BYTES {
            return Err("Built-in result exceeds 512 KiB; narrow the path or range.".into());
        }
        Ok(Content::Json { data })
    })();
    match result {
        Ok(content) => ToolResult {
            content: vec![content],
            is_error: false,
        },
        Err(message) => ToolResult::error(message),
    }
}

fn number(args: &Value, name: &str, default: usize) -> usize {
    args[name].as_f64().map_or(default, |value| value as usize)
}

struct Reader<'a> {
    root: &'a Path,
    scope: &'a Scope<'a>,
    owner: &'a str,
    cancellation: &'a CancellationToken,
}

impl Reader<'_> {
    fn check_cancelled(&self) -> Result<(), String> {
        if self.cancellation.is_cancelled() {
            Err("Agent tool call was cancelled.".into())
        } else {
            Ok(())
        }
    }

    fn location(&self, path: &str, listing: bool) -> Result<ScopedPath, String> {
        if listing {
            self.scope.resolve_listing(self.owner, path)
        } else {
            self.scope.resolve_path(self.owner, path)
        }
        .map_err(|e| e.to_string())
    }

    /// Open the resolved `location` of the logical `path`, which a missing entry names.
    fn open(&self, path: &str, location: &ScopedPath) -> Result<File, String> {
        let artifact = self.scope.artifacts[location.artifact_id.as_str()];
        scope::open_input(self.root, artifact, &location.path).map_err(|error| match error {
            OpenError::NotFound => missing(self.owner, path),
            error => error.to_string(),
        })
    }

    fn view_image(&self, path: &str) -> Result<Content, String> {
        let location = self.location(path, false)?;
        let bytes = image::read(self.open(path, &location)?)?;
        self.check_cancelled()?;
        image::normalize(&bytes, None)
    }

    fn read(&self, path: &str, offset: usize, limit: usize) -> Result<Value, String> {
        let location = self.location(path, false)?;
        let file = self.open(path, &location)?;
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("Reading requires a regular file; list the directory first.".into());
        }
        let mut reader = BufReader::new(file);
        let mut lines = Vec::new();
        let mut line = Vec::new();
        let mut current = 1;
        let mut returned_bytes = 0;
        let mut complete = false;
        let mut partial_line = false;
        loop {
            self.check_cancelled()?;
            let bytes = reader.fill_buf().map_err(|e| e.to_string())?;
            if bytes.is_empty() {
                if !line.is_empty() {
                    lines.push(text_line(current, &line)?);
                }
                complete = true;
                break;
            }
            let end = bytes
                .iter()
                .position(|byte| *byte == b'\n')
                .map(|index| index + 1);
            let fragment = &bytes[..end.unwrap_or(bytes.len())];
            if current >= offset {
                if returned_bytes + line.len() + fragment.len() > READ_BYTES {
                    if lines.is_empty() {
                        return Err(format!(
                            "Artifact line {current} exceeds the 65536-byte read limit."
                        ));
                    }
                    break;
                }
                if fragment.contains(&0) {
                    return Err("Binary text (NUL) cannot be read.".into());
                }
                line.extend_from_slice(fragment);
            }
            let consumed = fragment.len();
            partial_line = end.is_none();
            reader.consume(consumed);
            if end.is_some() {
                if current >= offset {
                    lines.push(text_line(current, &line)?);
                    returned_bytes += line.len();
                    line.clear();
                }
                current += 1;
                if lines.len() == limit || returned_bytes == READ_BYTES {
                    complete = reader.fill_buf().map_err(|e| e.to_string())?.is_empty();
                    break;
                }
            }
        }
        let count = lines.len();
        let mut result = json!({
            "artifactId":self.owner,"resolvedArtifactId":location.artifact_id,"path":path,
            "lines":lines,"startLine":offset,"endLine":if count == 0 { None } else { Some(offset + count - 1) },
            "lineCount":count,"truncated":!complete,"nextOffset":if complete { None } else { Some(offset + count) }
        });
        if complete {
            let total = current - 1 + usize::from(partial_line);
            result["totalLines"] = json!(total);
        }
        Ok(result)
    }

    fn entries(&self, path: &str) -> Result<Vec<Value>, String> {
        self.check_cancelled()?;
        let location = self.location(path, true)?;
        let file = self.open(path, &location)?;
        if !file.metadata().map_err(|e| e.to_string())?.is_dir() {
            return Err("Listing requires a directory.".into());
        }
        let owner = self.scope.artifacts[location.artifact_id.as_str()];
        let mut families: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (child, id) in &owner.children {
            if self
                .scope
                .artifacts
                .get(id.as_str())
                .is_some_and(|artifact| artifact.family.is_some())
                && let Some(folder) = child.strip_suffix(&format!("/{id}"))
            {
                families.entry(folder.into()).or_default().push(id.clone());
            }
        }
        let mut entries = BTreeMap::new();
        if let Some(instances) = families.get(&location.path) {
            for id in instances {
                entries.insert(
                    id.clone(),
                    json!({"name":id,"kind":"instance","artifactId":id}),
                );
            }
        } else {
            // Enumerate the pinned directory, not a path that could have been replaced by a link.
            let directory =
                platform::read_dir(&file).map_err(|_| "Cannot list Artifact directory.")?;
            for entry in directory {
                self.check_cancelled()?;
                if entries.len() >= MAX_ENTRIES {
                    return Err(
                        "Directory exceeds the 10,000-entry listing limit; choose a narrower path."
                            .into(),
                    );
                }
                let entry = entry.map_err(|_| "Cannot list Artifact entry.")?;
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| "Artifact paths must be UTF-8.")?;
                let kind = entry
                    .file_type()
                    .map_err(|_| "Cannot inspect Artifact entry.")?;
                let relative = join(&location.path, &name);
                let value = if kind == FileKind::Directory
                    && let Some(instances) = families.get(&relative)
                {
                    json!({"name":name,"kind":"family","instances":instances})
                } else {
                    json!({"name":name,"kind":match kind {
                        FileKind::File => "file",
                        FileKind::Directory => "directory",
                        FileKind::Symlink => "symlink",
                        FileKind::Other => "other",
                    }})
                };
                entries.insert(name, value);
            }
            if location.path.is_empty() {
                for (alias, id) in &owner.mounts {
                    self.check_cancelled()?;
                    if entries.len() == MAX_ENTRIES {
                        return Err("Directory exceeds the 10,000-entry listing limit; choose a narrower path.".into());
                    }
                    if !self.scope.artifacts.contains_key(id.as_str()) {
                        return Err("Mount is outside this eval's scope.".into());
                    }
                    if entries
                        .insert(
                            alias.clone(),
                            json!({"name":alias,"kind":"mount","artifactId":id}),
                        )
                        .is_some()
                    {
                        return Err("Logical mount conflicts with a physical entry.".into());
                    }
                }
            }
        }
        Ok(entries
            .into_iter()
            .map(|(name, mut value)| {
                value["path"] = json!(join(path, &name));
                value
            })
            .collect())
    }

    fn list(&self, path: &str, offset: usize, limit: usize) -> Result<Value, String> {
        let entries = self.entries(path)?;
        let total = entries.len();
        let mut bytes = 1024 + serde_json::to_vec(path).unwrap().len();
        let entries: Vec<_> = entries
            .into_iter()
            .skip(offset)
            .take(limit)
            .take_while(|entry| {
                bytes += serde_json::to_vec(entry).unwrap().len() + 1;
                bytes <= RESULT_BYTES
            })
            .collect();
        if entries.is_empty() && offset < total {
            return Err("Listing entry exceeds 512 KiB; list the family catalog directly.".into());
        }
        let next = offset + entries.len();
        Ok(
            json!({"artifactId":self.owner,"path":path,"entries":entries,"totalEntries":total,"truncated":next<total,"nextOffset":if next<total {Some(next)} else {None}}),
        )
    }

    fn files(&self, path: &str) -> Result<(BTreeSet<String>, bool), String> {
        let location = self.location(path, true)?;
        if self
            .open(path, &location)?
            .metadata()
            .map_err(|e| e.to_string())?
            .is_file()
        {
            return Ok((BTreeSet::from([path.into()]), false));
        }
        let mut pending = vec![(path.to_owned(), Vec::new())];
        let mut files = BTreeSet::new();
        let mut seen = 0;
        let mut truncated = false;
        while let Some((path, mut ancestors)) = pending.pop() {
            self.check_cancelled()?;
            let location = self.location(&path, true)?;
            let key = (location.artifact_id, location.path);
            if ancestors.contains(&key) {
                continue;
            }
            ancestors.push(key);
            for entry in self.entries(&path)? {
                if seen == MAX_ENTRIES {
                    truncated = true;
                    break;
                }
                seen += 1;
                let child = entry["path"].as_str().unwrap();
                match entry["kind"].as_str().unwrap() {
                    "file" => {
                        files.insert(child.to_owned());
                    }
                    "directory" | "family" | "mount" | "instance" => {
                        pending.push((child.into(), ancestors.clone()))
                    }
                    _ => {}
                }
            }
            if truncated {
                break;
            }
        }
        Ok((files, truncated))
    }

    fn glob(&self, path: &str, pattern: &str) -> Result<Value, String> {
        let matcher = glob(pattern)?;
        let location = self.location(path, true)?;
        if !self
            .open(path, &location)?
            .metadata()
            .map_err(|e| e.to_string())?
            .is_dir()
        {
            return Err("Glob path must be a directory.".into());
        }
        let (files, mut truncated) = self.files(path)?;
        let mut matches = Vec::new();
        let mut bytes = 1024 + serde_json::to_vec(path).unwrap().len();
        for file in files {
            self.check_cancelled()?;
            if !matcher.is_match(relative(path, &file)) {
                continue;
            }
            bytes += serde_json::to_vec(&file).unwrap().len() + 1;
            if matches.len() == MAX_RESULTS || bytes > RESULT_BYTES {
                truncated = true;
                break;
            }
            // Recheck entries that may have changed since enumeration.
            if !self
                .open(&file, &self.location(&file, false)?)?
                .metadata()
                .map_err(|e| e.to_string())?
                .is_file()
            {
                return Err("Glob target is no longer a regular file.".into());
            }
            matches.push(file);
        }
        Ok(json!({"path":path,"files":matches,"truncated":truncated}))
    }

    fn grep(&self, path: &str, args: &Value) -> Result<Value, String> {
        let regex = RegexBuilder::new(args["pattern"].as_str().unwrap())
            .case_insensitive(args["caseInsensitive"].as_bool().unwrap_or(false))
            .size_limit(2 * 1024 * 1024)
            .build()
            .map_err(|_| "Invalid or oversized regex pattern.")?;
        let filter = args["glob"].as_str().map(glob).transpose()?;
        let (files, mut truncated) = self.files(path)?;
        let mut matches = Vec::new();
        let mut searched = 0;
        let mut bytes = 1024 + serde_json::to_vec(path).unwrap().len();
        let limit = number(args, "maxResults", MAX_RESULTS);
        'files: for file in files {
            self.check_cancelled()?;
            if filter
                .as_ref()
                .is_some_and(|filter| !filter.is_match(relative(path, &file)))
            {
                continue;
            }
            let input = self.open(&file, &self.location(&file, false)?)?;
            let metadata = input.metadata().map_err(|e| e.to_string())?;
            if !metadata.is_file() {
                return Err("Grep requires regular files.".into());
            }
            if metadata.len() > SEARCH_FILE_BYTES as u64 {
                truncated = true;
                continue;
            }
            if searched + metadata.len() as usize > SEARCH_BYTES {
                truncated = true;
                break;
            }
            let mut contents = Vec::new();
            input
                .take((SEARCH_FILE_BYTES.min(SEARCH_BYTES - searched) + 1) as u64)
                .read_to_end(&mut contents)
                .map_err(|_| "Cannot read Artifact file.")?;
            searched += contents.len();
            if searched > SEARCH_BYTES {
                truncated = true;
                break;
            }
            if contents.len() > SEARCH_FILE_BYTES {
                truncated = true;
                continue;
            }
            if contents.contains(&0) {
                continue;
            }
            let Ok(contents) = std::str::from_utf8(&contents) else {
                continue;
            };
            for (index, line) in contents.split_inclusive('\n').enumerate() {
                self.check_cancelled()?;
                let line = line.strip_suffix('\n').unwrap_or(line);
                let line = line.strip_suffix('\r').unwrap_or(line);
                if !regex.is_match(line) {
                    continue;
                }
                let entry = json!({"path":file,"line":index+1,"text":line});
                bytes += serde_json::to_vec(&entry).unwrap().len() + 1;
                if matches.len() == limit || bytes > RESULT_BYTES {
                    truncated = true;
                    break 'files;
                }
                matches.push(entry);
            }
        }
        Ok(json!({"path":path,"matches":matches,"truncated":truncated}))
    }
}

/// A missing path, with a hint when a model prefixed it with the Artifact's own name.
fn missing(owner: &str, path: &str) -> String {
    match path.strip_prefix(owner) {
        Some("") => format!(
            "No {path:?} in Artifact {owner}; paths are relative to the Artifact, without its name (its root is \"\")."
        ),
        Some(rest) if rest.starts_with('/') => format!(
            "No {path:?} in Artifact {owner}; paths are relative to the Artifact (for example {:?}), without its name.",
            &rest[1..]
        ),
        _ => format!("No {path:?} in Artifact {owner}."),
    }
}

fn text_line(number: usize, line: &[u8]) -> Result<Value, String> {
    let text = std::str::from_utf8(line)
        .map_err(|_| "Artifact contains invalid UTF-8 in the requested lines.")?;
    Ok(json!({"number":number,"text":text}))
}

fn join(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.into()
    } else {
        format!("{base}/{name}")
    }
}

fn relative<'a>(base: &str, file: &'a str) -> &'a str {
    if file == base {
        file.rsplit('/').next().unwrap()
    } else if base.is_empty() {
        file
    } else {
        file.strip_prefix(base).unwrap().strip_prefix('/').unwrap()
    }
}

fn glob(pattern: &str) -> Result<GlobMatcher, String> {
    if pattern.starts_with('/')
        || pattern.contains(['\\', '\0', ':'])
        || pattern
            .split('/')
            .any(|part| part == "." || part == ".." || part.is_empty())
    {
        return Err("Glob pattern must be relative without traversal components.".into());
    }
    GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .map(|glob| glob.compile_matcher())
        .map_err(|_| "Invalid glob pattern.".into())
}

#[cfg(test)]
mod tests;
