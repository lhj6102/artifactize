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
        Ok(())
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
            let harness = harness(&command);
            if self.command.as_ref() != Some(&command) || self.harness != harness {
                self.session_id = harness
                    .as_ref()
                    .and_then(|h| session_id(h, &command, self.cwd.as_deref()));
                self.harness = harness;
                self.command = Some(command);
            }
        }
        agent
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
    let requested = command.args.windows(2).find_map(|pair| {
        matches!(pair[0].as_str(), "--resume" | "--session-id" | "resume")
            .then_some(pair[1].as_str())
    })?;
    if requested.starts_with('-') {
        return None;
    }
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
                let records = serde_json::Deserializer::from_reader(io::BufReader::new(file))
                    .into_iter::<Envelope>();
                for record in records {
                    let record = record.ok()?;
                    let (id, directory) = match harness {
                        Harness::ClaudeCode => (record.session_id, record.cwd),
                        Harness::Codex if record.kind.as_deref() == Some("session_meta") => {
                            let metadata = record.payload?;
                            (metadata.id, metadata.cwd)
                        }
                        Harness::Codex => break,
                    };
                    if id.as_deref() == Some(requested) && directory.as_deref() == cwd {
                        return id;
                    }
                }
            }
        }
    }
    None
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    cwd: Option<PathBuf>,
    payload: Option<SessionMeta>,
}
#[derive(Deserialize)]
struct SessionMeta {
    id: Option<String>,
    cwd: Option<PathBuf>,
}
