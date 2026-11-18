//! Scoped read-only Agent tools.
mod fixed;
mod input;
mod metadata;
pub use fixed::{
    Context, call_fixed, fixed_description, fixed_schema, is_url, validate_args, validate_target,
};
pub use input::Input;
pub use metadata::{description, input_schema};

mod output;
mod scan;
use output::{Entry, EntryKind, Output};

/// Bound the compiled regex automaton to limit search memory.
const REGEX_BYTES: usize = 2 * 1024 * 1024;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
};

use globset::{GlobBuilder, GlobMatcher};
use tokio_util::sync::CancellationToken;

use crate::{
    files::{self as platform, FileKind},
    scope::{self, ArtifactId, InputPath, Location, OpenError, Scope},
};

use crate::{Content, ToolResult, image};

/// Bound one read's contribution to the Agent context while preserving complete lines.
pub const READ_BYTES: usize = 64 * 1024;
/// Cap serialized tool output even when match text or logical paths are very long.
pub const RESULT_BYTES: usize = 512 * 1024;
/// Reserve 1 KiB for serialized list/glob/grep keys, brackets and page metadata before
/// accumulating entries; the final serialized-result check still enforces RESULT_BYTES.
const RESULT_ENVELOPE_BYTES: usize = 1024;
/// Keep list/search pages small enough for a review; pagination exposes later entries.
const MAX_RESULTS: usize = 200;
/// Bound directory fan-out and traversal work regardless of the requested result count.
const MAX_ENTRIES: usize = 10_000;
/// Skip individual oversized files so one input cannot monopolize a search.
pub const SEARCH_FILE_BYTES: usize = 8 * 1024 * 1024;
/// Bound total search I/O across files; exhausting the budget reports truncation.
const SEARCH_BYTES: usize = 64 * 1024 * 1024;
/// Patterns are user input rather than paths, but share their bounded schema size.
const MAX_PATTERN_UNITS: usize = scope::MAX_PATH_UNITS;
/// An ordinary read starts with eighty lines; owners can page further or request
/// up to five hundred without scanning an unbounded number of lines in one call.
const DEFAULT_READ_LINES: usize = 80;
const MAX_READ_LINES: usize = 500;

pub fn call(
    input: Input,
    root: &Path,
    scope: &Scope,
    owner: &ArtifactId,
    cancellation: &CancellationToken,
) -> ToolResult {
    let reader = Reader {
        root,
        scope,
        owner,
        cancellation,
    };
    let result = (|| {
        reader.check_cancelled()?;
        let data = match input {
            Input::Read(input) => {
                reader.read(reader.path(&input.path, false)?, input.offset, input.limit)
            }
            Input::List(input) => {
                reader.list(reader.path(&input.path, true)?, input.offset, input.limit)
            }
            Input::Glob(input) => reader.glob(reader.path(&input.path, true)?, &input.pattern),
            Input::Grep(input) => reader.grep(reader.path(&input.path, true)?, &input),
            Input::ViewImage(input) => return reader.view_image(reader.path(&input.path, false)?),
        }?;
        if serde_json::to_vec(&data).unwrap().len() > RESULT_BYTES {
            return Err("Built-in result exceeds 512 KiB; narrow the path or range.".into());
        }
        Ok(Content::Json {
            data: serde_json::to_value(data).expect("builtin output is JSON"),
        })
    })();
    match result {
        Ok(content) => ToolResult::success(content),
        Err(message) => ToolResult::error(message),
    }
}

struct Reader<'a> {
    root: &'a Path,
    scope: &'a Scope,
    owner: &'a ArtifactId,
    cancellation: &'a CancellationToken,
}

impl Reader<'_> {
    fn path<'p>(
        &self,
        path: &'p input::LogicalPath,
        allow_dot: bool,
    ) -> Result<InputPath<&'p str>, String> {
        path.resolve(self.scope, self.owner, allow_dot)
    }

    fn check_cancelled(&self) -> Result<(), String> {
        if self.cancellation.is_cancelled() {
            Err("Agent tool call was cancelled.".into())
        } else {
            Ok(())
        }
    }

    fn location<'a, 'p>(&'a self, path: InputPath<&'p str>) -> Result<Location<'a, 'p>, String> {
        self.scope
            .locate(self.owner, path)
            .map_err(|error| error.to_string())
    }

    /// Open the resolved location, naming the original logical path if the entry is absent.
    fn open(&self, path: InputPath<&str>, location: &Location<'_, '_>) -> Result<File, String> {
        location.open(self.root).map_err(|error| match error {
            OpenError::NotFound => missing(self.owner, path.as_str()),
            error => error.to_string(),
        })
    }

    fn view_image(&self, path: InputPath<&str>) -> Result<Content, String> {
        let location = self.location(path)?;
        let bytes = image::read(self.open(path, &location)?)?;
        self.check_cancelled()?;
        image::normalize(&bytes, None)
    }

    fn read(&self, path: InputPath<&str>, offset: usize, limit: usize) -> Result<Output, String> {
        let location = self.location(path)?;
        let file = self.open(path, &location)?;
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("Reading requires a regular file; list the directory first.".into());
        }
        let viewed_path = location
            .artifact
            .file_name()
            .unwrap_or(location.path.as_str());
        let file = crate::artf::reader(file, viewed_path).map_err(|error| error.to_string())?;
        let mut reader = BufReader::new(file);
        let mut scan = scan::Read::new(offset, limit);
        let end = loop {
            self.check_cancelled()?;
            let bytes = reader.fill_buf().map_err(|error| error.to_string())?;
            if bytes.is_empty() {
                break scan::ReadEnd::Complete;
            }
            match scan.push(bytes)? {
                scan::ReadStep::More(consumed) => reader.consume(consumed),
                scan::ReadStep::Full(consumed) => {
                    reader.consume(consumed);
                    break if reader
                        .fill_buf()
                        .map_err(|error| error.to_string())?
                        .is_empty()
                    {
                        scan::ReadEnd::Complete
                    } else {
                        scan::ReadEnd::Bounded
                    };
                }
                scan::ReadStep::Bounded => break scan::ReadEnd::Bounded,
            }
        };
        Ok(Output::Read(output::Read {
            artifact_id: self.owner.clone(),
            resolved_artifact_id: location.artifact_id.clone(),
            path: path.as_str().to_owned(),
            rows: scan.finish(end)?,
        }))
    }

    fn entries(&self, path: InputPath<&str>) -> Result<Vec<(Entry, InputPath<String>)>, String> {
        self.check_cancelled()?;
        let location = self.location(path)?;
        let file = self.open(path, &location)?;
        if !file.metadata().map_err(|e| e.to_string())?.is_dir() {
            return Err("Listing requires a directory.".into());
        }
        let owner = location.artifact;
        let mut entries = BTreeMap::new();
        if let Some(name) = owner.file_name() {
            let target = scope::open_child(&file, std::ffi::OsStr::new(name))
                .map_err(|error| error.to_string())?;
            if !target.metadata().map_err(|e| e.to_string())?.is_file() {
                return Err("File Artifact target must remain a regular file.".into());
            }
            entries.insert(name.to_owned(), EntryKind::File);
        } else {
            // Enumerate the pinned directory, not a path that could have been replaced by a link.
            // An aspect lists only what leads to or lies in its selection.
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
                let native = entry.file_name();
                let Some(name) = platform::logical_entry_name(&native) else {
                    continue;
                };
                let Some(entry_path) = location.path.child(name) else {
                    continue;
                };
                if !owner.admits(entry_path.as_str()) || self.excludes(owner, entry_path.as_str()) {
                    continue;
                }
                let kind = entry
                    .file_type()
                    .map_err(|_| "Cannot inspect Artifact entry.")?;
                let kind = match kind {
                    FileKind::File => EntryKind::File,
                    FileKind::Directory => EntryKind::Directory,
                    FileKind::Symlink => EntryKind::Symlink,
                    FileKind::Other => EntryKind::Other,
                };
                entries.insert(name.to_owned(), kind);
            }
        }
        Ok(entries
            .into_iter()
            .filter_map(|(name, kind)| {
                let child = path.child(&name)?;
                Some((
                    Entry {
                        kind,
                        path: child.as_str().to_owned(),
                        name,
                    },
                    child,
                ))
            })
            .collect())
    }

    /// A folder Artifact nested in `owner` at `path` that this review does not admit: its
    /// files belong to neither, so listings and searches pass over it.
    fn excludes(&self, owner: &scope::Artifact, path: &str) -> bool {
        owner
            .nested
            .iter()
            .any(|(prefix, id)| prefix.as_str() == path && !self.scope.artifacts.contains_key(id))
    }

    fn list(&self, path: InputPath<&str>, offset: usize, limit: usize) -> Result<Output, String> {
        let entries = self.entries(path)?;
        scan::list(
            self.owner.clone(),
            path.as_str(),
            entries.into_iter().map(|(entry, _)| entry).collect(),
            offset,
            limit,
        )
        .map(Output::List)
    }

    fn files(&self, path: InputPath<&str>) -> Result<(BTreeSet<InputPath<String>>, bool), String> {
        let location = self.location(path)?;
        if self
            .open(path, &location)?
            .metadata()
            .map_err(|e| e.to_string())?
            .is_file()
        {
            return Ok((BTreeSet::from([path.owned()]), false));
        }
        let mut pending = vec![path.owned()];
        let mut files = BTreeSet::new();
        let mut seen = 0;
        let mut truncated = false;
        while let Some(path) = pending.pop() {
            self.check_cancelled()?;
            for (entry, child) in self.entries(path.borrowed())? {
                if seen == MAX_ENTRIES {
                    truncated = true;
                    break;
                }
                seen += 1;
                match entry.kind {
                    EntryKind::File => {
                        files.insert(child);
                    }
                    EntryKind::Directory => pending.push(child),
                    EntryKind::Symlink | EntryKind::Other => {}
                }
            }
            if truncated {
                break;
            }
        }
        Ok((files, truncated))
    }

    fn glob(&self, path: InputPath<&str>, matcher: &GlobMatcher) -> Result<Output, String> {
        let location = self.location(path)?;
        if !self
            .open(path, &location)?
            .metadata()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            return Err("Glob path must be a directory.".into());
        }
        let (files, truncated) = self.files(path)?;
        let mut scan = scan::Glob::new(path.as_str(), matcher, truncated);
        for file in files {
            self.check_cancelled()?;
            match scan.push(file.as_str()) {
                scan::Selection::Skip => continue,
                scan::Selection::Full => break,
                scan::Selection::Added => {}
            }
            // The selected file can have changed since directory enumeration.
            if !self
                .open(file.borrowed(), &self.location(file.borrowed())?)?
                .metadata()
                .map_err(|error| error.to_string())?
                .is_file()
            {
                return Err("Glob target is no longer a regular file.".into());
            }
        }
        Ok(Output::Glob(scan.finish()))
    }

    fn grep(&self, path: InputPath<&str>, input: &input::Grep) -> Result<Output, String> {
        let (files, truncated) = self.files(path)?;
        let mut scan = scan::Grep::new(path.as_str(), input, truncated);
        'files: for file in files {
            self.check_cancelled()?;
            if !scan.allows(file.as_str()) {
                continue;
            }
            let input = self.open(file.borrowed(), &self.location(file.borrowed())?)?;
            let metadata = input.metadata().map_err(|error| error.to_string())?;
            if !metadata.is_file() {
                return Err("Grep requires regular files.".into());
            }
            let limit = match scan.file(metadata.len()) {
                scan::FileRead::Read(limit) => limit,
                scan::FileRead::Skip => continue,
                scan::FileRead::Full => break,
            };
            let mut contents = Vec::new();
            input
                .take(limit)
                .read_to_end(&mut contents)
                .map_err(|_| "Cannot read Artifact file.")?;
            match scan.received(contents.len()) {
                scan::Received::Skip => continue,
                scan::Received::Full => break,
                scan::Received::Text => {}
            }
            let contents = if file.as_str().ends_with(".artf") {
                crate::artf::definition(&contents)
            } else {
                std::borrow::Cow::Borrowed(contents.as_slice())
            };
            let Some(text) = scan::text(&contents) else {
                continue;
            };
            for (index, line) in text.split_inclusive('\n').enumerate() {
                self.check_cancelled()?;
                if !scan.line(file.as_str(), index, line) {
                    break 'files;
                }
            }
        }
        Ok(Output::Grep(scan.finish()))
    }
}

/// A missing path, with a hint when a model prefixed it with the Artifact's own name.
fn missing(owner: &ArtifactId, path: &str) -> String {
    match path.strip_prefix(owner.as_str()) {
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
