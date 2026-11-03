//! Restore facts only. Transcript envelopes are deserialized into metadata-only structs;
//! messages, tool calls and conversation text are skipped, never retained or interpreted.
use super::{Harness, RestoreOffer, ResumeSession, TerminalCommand};
use crate::platform;
use crate::types::SessionId;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Metadata {
    #[serde(with = "crate::platform::path_serde::option")]
    pub cwd: Option<PathBuf>,
    pub command: Option<TerminalCommand>,
    pub harness: Option<Harness>,
    pub session_id: Option<SessionId>,
}
impl Metadata {
    pub fn read(identity: &Path) -> io::Result<Option<Self>> {
        let file = match platform::open_no_follow(
            platform::private_options().read(true),
            &identity.join("artifactize-terminal.json"),
        ) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        serde_json::from_reader(file)
            .map(Some)
            .map_err(io::Error::other)
    }
    pub fn save(&self, identity: &Path) -> io::Result<()> {
        // Rename makes a reboot/crash leave either complete metadata version, never half JSON.
        let mut file = tempfile::NamedTempFile::new_in(identity)?;
        platform::restrict_file(file.as_file())?;
        serde_json::to_writer(file.as_file_mut(), self)?;
        file.as_file().sync_all()?;
        file.persist(identity.join("artifactize-terminal.json"))
            .map_err(io::Error::other)?;
        // The renamed entry itself must survive a reboot, not only the file's contents.
        platform::sync_dir(identity)
    }
    pub fn offer(&self) -> RestoreOffer {
        match (&self.cwd, &self.command, &self.harness) {
            (Some(cwd), Some(command), Some(harness)) => {
                let session = match &self.session_id {
                    Some(id) => ResumeSession::Exact { id: id.clone() },
                    None => ResumeSession::Latest,
                };
                RestoreOffer::Harness {
                    cwd: cwd.clone(),
                    command: command.clone(),
                    harness: harness.clone(),
                    resume: harness.resume(&session),
                    session,
                }
            }
            (Some(cwd), Some(command), None) => RestoreOffer::Command {
                cwd: cwd.clone(),
                command: command.clone(),
            },
            _ => RestoreOffer::Shell {
                cwd: self.cwd.clone(),
            },
        }
    }
    /// Record the foreground program; whether it is an agent, a harness, is returned.
    pub fn observe(&mut self, foreground: platform::terminal::Foreground) -> bool {
        if let Some(cwd) = foreground.cwd {
            self.cwd = Some(cwd);
        }
        if foreground.shell {
            return false;
        }
        let Some(command) = foreground.command else {
            return false;
        };
        let command = invoked(command, self.cwd.as_deref());
        self.harness = harness(&command);
        self.command = Some(command);
        self.harness.is_some()
    }
    /// Prove the session again for the current command and directory: an ID proven for one
    /// directory is never offered for another.
    pub fn certify(&mut self) {
        self.session_id = match (&self.harness, &self.command) {
            (Some(harness), Some(command)) => session_id(harness, command, self.cwd.as_deref()),
            _ => None,
        };
    }
}

/// Programs a script's shebang names to run it: npm installs `claude` and `codex` as Node.js
/// scripts, and other installed tools are Python, Ruby or Perl scripts started the same way.
const INTERPRETERS: [&str; 7] = ["node", "nodejs", "bun", "deno", "python", "ruby", "perl"];

/// The harness a program is, by the name it was run as.
fn harness(command: &TerminalCommand) -> Option<Harness> {
    match Path::new(&command.program).file_stem()?.to_str()? {
        "claude" => Some(Harness::ClaudeCode),
        "codex" => Some(Harness::Codex),
        _ => None,
    }
}

fn is_interpreter(command: &TerminalCommand) -> bool {
    Path::new(&command.program)
        .file_stem()
        .and_then(|stem| stem.to_str())
        // A versioned interpreter (`python3`, `python3.12`) is still that interpreter.
        .map(|stem| stem.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.'))
        .is_some_and(|name| INTERPRETERS.contains(&name))
}

/// The harness an interpreter runs as its script, if any.
fn interpreted(command: &TerminalCommand) -> Option<Harness> {
    if !is_interpreter(command) {
        return None;
    }
    let script = Path::new(command.args.first()?);
    match script.file_stem()?.to_str()? {
        "claude" => Some(Harness::ClaudeCode),
        "codex" => Some(Harness::Codex),
        "cli"
            if script
                .components()
                .any(|part| part.as_os_str() == "claude-code") =>
        {
            Some(Harness::ClaudeCode)
        }
        _ => None,
    }
}

/// The command as the person ran it. A script started through its shebang shows up as its
/// interpreter running the script's path, as installed `claude` and `codex` do; the person
/// ran the script by its name. A harness is named as the harness, as its resume commands
/// are; another script by its file name when that name finds the very same file on `PATH`.
/// Anything else is kept exactly as it ran.
fn invoked(command: TerminalCommand, cwd: Option<&Path>) -> TerminalCommand {
    let Some((script, args)) = command.args.split_first() else {
        return command;
    };
    if !is_interpreter(&command) {
        return command;
    }
    let program = interpreted(&command)
        .map(|harness| harness.program().to_owned())
        .or_else(|| by_name(Path::new(script), cwd?));
    match program {
        Some(program) => TerminalCommand {
            program,
            args: args.to_vec(),
        },
        None => command,
    }
}

/// The file name `PATH` finds `script` by, if it finds this very file.
fn by_name(script: &Path, cwd: &Path) -> Option<String> {
    if !script.is_absolute() {
        return None;
    }
    let name = script.file_name()?;
    let found = platform::program::resolve(name, cwd).ok()?;
    (platform::canonicalize(&found).ok()? == platform::canonicalize(script).ok()?)
        .then(|| name.to_str().map(str::to_owned))
        .flatten()
}

// An exact resume/session argument plus matching metadata is proof. A matching cwd or newest
// timestamp alone is not: concurrent agents may be working in the very same directory.
fn session_id(
    harness: &Harness,
    command: &TerminalCommand,
    cwd: Option<&Path>,
) -> Option<SessionId> {
    let requested: SessionId = requested(harness, command)?.parse().ok()?;
    // An unknown directory proves nothing: the session must name the very directory.
    let cwd = cwd?;
    let home = platform::home_directory()?;
    let root = match harness {
        Harness::ClaudeCode => platform::environment::var("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".claude"))
            .join("projects"),
        Harness::Codex => platform::environment::var("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"))
            .join("sessions"),
    };
    let mut directories = vec![root];
    while let Some(directory) = directories.pop() {
        // Links and Windows reparse points are classified, never followed.
        let opened = platform::open_directory(&directory).ok()?;
        for entry in platform::read_dir(&opened).ok()? {
            let entry = entry.ok()?;
            let name = entry.file_name();
            let path = directory.join(&name);
            let kind = entry.file_type().ok()?;
            if kind == platform::FileKind::Directory {
                directories.push(path);
            } else if kind == platform::FileKind::File
                && name
                    .to_string_lossy()
                    .ends_with(&format!("{requested}.jsonl"))
            {
                let file = io::BufReader::new(
                    platform::open_no_follow(fs::OpenOptions::new().read(true), &path).ok()?,
                );
                let proven = proves(harness, file, &requested, cwd, platform::PathRules::NATIVE);
                if proven {
                    return Some(requested);
                }
            }
        }
    }
    None
}

/// Whether a harness's session records prove `requested` for the directory `cwd`. Records
/// stream through metadata-only structs: serde skips every other field as it parses, so no
/// message or tool-call content is ever held. A record's directory is the harness's own
/// spelling of it, compared by `rules` and then by the directory it opens.
pub(super) fn proves(
    harness: &Harness,
    records: impl io::Read,
    requested: &SessionId,
    cwd: &Path,
    rules: platform::PathRules,
) -> bool {
    let proves = |id: &Option<SessionId>, directory: &Option<PathBuf>| {
        id.as_ref() == Some(requested)
            && directory.as_deref().is_some_and(|directory| {
                rules.same(directory, cwd) || platform::paths_equal(directory, cwd)
            })
    };
    match harness {
        Harness::ClaudeCode => serde_json::Deserializer::from_reader(records)
            .into_iter::<ClaudeRecord>()
            .map_while(Result::ok)
            .any(|record| proves(&record.session_id, &record.cwd)),
        // A rollout opens with its session_meta record; nothing after it is read.
        Harness::Codex => serde_json::Deserializer::from_reader(records)
            .into_iter::<CodexRecord>()
            .next()
            .and_then(Result::ok)
            .and_then(|record| record.0)
            .is_some_and(|meta| proves(&meta.id, &meta.cwd)),
    }
}

/// Recognize only a complete, unambiguous resume invocation. Unknown options may consume
/// the following token, so searching later arguments for a flag would invent certainty.
/// The session's own metadata must still confirm this ID and its directory.
fn requested<'a>(harness: &Harness, command: &'a TerminalCommand) -> Option<&'a str> {
    let value = match (harness, command.args.as_slice()) {
        (Harness::ClaudeCode, [flag, id])
            if matches!(flag.as_str(), "--resume" | "-r" | "--session-id") =>
        {
            Some(id.as_str())
        }
        (Harness::ClaudeCode, [argument]) => argument
            .strip_prefix("--resume=")
            .or_else(|| argument.strip_prefix("--session-id=")),
        (Harness::Codex, [subcommand, id]) if subcommand == "resume" => Some(id.as_str()),
        _ => None,
    }?;
    (!value.is_empty() && !value.starts_with('-')).then_some(value)
}

#[derive(Deserialize)]
struct ClaudeRecord {
    #[serde(rename = "sessionId")]
    session_id: Option<SessionId>,
    cwd: Option<PathBuf>,
}

/// A Codex rollout record's session metadata, kept only when the record says it is
/// `session_meta`, whichever of its fields comes first. A payload is read only through
/// [`SessionMeta`]'s two fields, so whatever else it carries is skipped unread.
struct CodexRecord(Option<SessionMeta>);
impl<'de> Deserialize<'de> for CodexRecord {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = CodexRecord;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a Codex rollout record")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut session = false;
                let mut meta = None;
                while let Some(key) = map.next_key::<std::borrow::Cow<'de, str>>()? {
                    match key.as_ref() {
                        "type" => {
                            session =
                                map.next_value::<std::borrow::Cow<'de, str>>()? == "session_meta"
                        }
                        "payload" => meta = Some(map.next_value::<SessionMeta>()?),
                        _ => {
                            map.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(CodexRecord(meta.filter(|_| session)))
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}
#[derive(Deserialize)]
struct SessionMeta {
    id: Option<SessionId>,
    cwd: Option<PathBuf>,
}
