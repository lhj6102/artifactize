//! Restore facts only. Transcript envelopes are deserialized into metadata-only structs;
//! messages, tool calls and conversation text are skipped, never retained or interpreted.
use super::{Harness, RestoreOffer, TerminalCommand};
use crate::platform;
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
    pub session_id: Option<String>,
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
        let resume = self.harness.as_ref().map(|harness| match harness {
            Harness::ClaudeCode => TerminalCommand {
                program: "claude".into(),
                args: match &self.session_id {
                    Some(id) => vec!["--resume".into(), id.clone()],
                    None => vec!["--continue".into()],
                },
            },
            Harness::Codex => TerminalCommand {
                program: "codex".into(),
                args: vec![
                    "resume".into(),
                    self.session_id.clone().unwrap_or_else(|| "--last".into()),
                ],
            },
        });
        RestoreOffer {
            cwd: self.cwd.clone(),
            command: self.command.clone(),
            harness: self.harness.clone(),
            session_id: self.session_id.clone(),
            latest_session: resume.is_some() && self.session_id.is_none(),
            resume,
        }
    }
    pub fn observe(&mut self, foreground: platform::terminal::Foreground) -> bool {
        let agent = !foreground.shell && foreground.command.as_ref().and_then(harness).is_some();
        if let Some(cwd) = foreground.cwd {
            self.cwd = Some(cwd);
        }
        if !foreground.shell
            && let Some(command) = foreground.command
        {
            self.harness = harness(&command);
            self.command = Some(command);
        }
        agent
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

fn harness(command: &TerminalCommand) -> Option<Harness> {
    let program = Path::new(&command.program).file_stem()?.to_str()?;
    match program {
        "claude" => Some(Harness::ClaudeCode),
        "codex" => Some(Harness::Codex),
        "node" | "nodejs" | "bun" => {
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
        _ => None,
    }
}

// An exact resume/session argument plus matching metadata is proof. A matching cwd or newest
// timestamp alone is not: concurrent agents may be working in the very same directory.
fn session_id(harness: &Harness, command: &TerminalCommand, cwd: Option<&Path>) -> Option<String> {
    let requested = requested(harness, command)?;
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
        for entry in fs::read_dir(directory).ok()?.flatten() {
            let kind = entry.file_type().ok()?;
            if kind.is_dir() {
                directories.push(entry.path());
            } else if kind.is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with(&format!("{requested}.jsonl"))
            {
                let file = fs::File::open(entry.path()).ok()?;
                for line in io::BufRead::lines(io::BufReader::new(file)) {
                    let line = line.ok()?;
                    let record: Envelope = serde_json::from_str(&line).ok()?;
                    let (id, directory) = match harness {
                        Harness::ClaudeCode => (record.session_id, record.cwd),
                        // Only a session_meta record's payload is decoded at all.
                        Harness::Codex if record.kind.as_deref() == Some("session_meta") => {
                            let meta: SessionRecord = serde_json::from_str(&line).ok()?;
                            (meta.payload.id, meta.payload.cwd)
                        }
                        Harness::Codex => break,
                    };
                    if id.as_deref() == Some(requested)
                        && directory
                            .as_deref()
                            .is_some_and(|directory| platform::paths_equal(directory, cwd))
                    {
                        return id;
                    }
                }
            }
        }
    }
    None
}

/// The session a harness command names in that harness's own syntax: Claude Code's
/// `--resume ID`, `-r ID` or `--session-id ID` (also with `=`), and Codex's `resume ID`
/// subcommand. Arguments after `--` are operands, and a harness started through its
/// interpreter has its script first. The session's own metadata must still confirm the ID.
fn requested<'a>(harness: &Harness, command: &'a TerminalCommand) -> Option<&'a str> {
    let interpreted = Path::new(&command.program)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| matches!(stem, "node" | "nodejs" | "bun"));
    let args = command.args.get(usize::from(interpreted)..)?;
    let options = args.split(|arg| arg == "--").next().unwrap_or_default();
    let value = match harness {
        Harness::ClaudeCode => {
            options
                .iter()
                .enumerate()
                .find_map(|(at, arg)| match arg.as_str() {
                    "--resume" | "-r" | "--session-id" => options.get(at + 1).map(String::as_str),
                    other => other
                        .strip_prefix("--resume=")
                        .or_else(|| other.strip_prefix("--session-id=")),
                })
        }
        Harness::Codex => match options {
            [subcommand, id, ..] if subcommand == "resume" => Some(id.as_str()),
            _ => None,
        },
    }?;
    (!value.is_empty() && !value.starts_with('-')).then_some(value)
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    cwd: Option<PathBuf>,
}
#[derive(Deserialize)]
struct SessionRecord {
    payload: SessionMeta,
}
#[derive(Deserialize)]
struct SessionMeta {
    id: Option<String>,
    cwd: Option<PathBuf>,
}
