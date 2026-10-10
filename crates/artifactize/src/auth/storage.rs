use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde::{Serialize, de::DeserializeOwned};

use crate::platform;

/// Allow complete token envelopes while bounding memory for malformed credential
/// files, consistently for application-owned storage and read-only Codex imports.
pub(super) const MAX_CREDENTIAL_BYTES: usize = 1024 * 1024;

/// Whose tokens a storage holds, which names it in a refusal.
#[derive(Clone, Copy)]
pub(super) enum Tokens {
    Codex,
    Remote,
}

/// `$STATE/auth` (canonical) and the repository enclosing it, if any: tokens are never
/// stored inside a git work tree, an artifactize workspace or the `--repo` folder.
pub(super) struct Location {
    pub directory: PathBuf,
    enclosure: Option<(&'static str, PathBuf)>,
}

impl Location {
    pub fn find(state: Option<&Path>, repo: Option<&Path>) -> Result<Self, String> {
        let directory = crate::store::state_dir(state)?.join("auth");
        let directory =
            crate::workspace::canonical_target(&directory).map_err(|e| e.to_string())?;
        let mut marked = None;
        for ancestor in directory.ancestors() {
            let kind =
                if platform::entry_exists(&ancestor.join(".git")).map_err(|e| e.to_string())? {
                    Some("git work tree")
                } else if crate::workspace::has_artifact_marker(ancestor)
                    .map_err(|error| error.to_string())?
                {
                    Some("artifactize workspace")
                } else {
                    None
                };
            if let Some(kind) = kind {
                marked = Some((kind, ancestor.to_owned()));
                break;
            }
        }
        let enclosure = match (marked, repo) {
            (Some(marked), _) => Some(marked),
            (None, Some(repo)) => {
                let repo = platform::canonicalize(repo).map_err(|e| e.to_string())?;
                let repo = match repo.parent() {
                    Some(parent) if repo.is_file() => parent.to_owned(),
                    _ => repo,
                };
                platform::is_within(&directory, &repo).then_some(("reviewed repository", repo))
            }
            (None, None) => None,
        };
        Ok(Self {
            directory,
            enclosure,
        })
    }

    /// Why tokens may not be stored here, if they may not.
    pub fn refusal(&self, tokens: Tokens) -> Option<String> {
        let (kind, path) = self.enclosure.as_ref()?;
        let (storage, variable) = match tokens {
            Tokens::Codex => ("Codex sign-in storage", "ARTIFACTIZE_CODEX_AUTH_FILE"),
            Tokens::Remote => ("Remote token storage", "ARTIFACTIZE_REMOTE_TOKEN"),
        };
        Some(format!(
            "{storage} {} is inside the {kind} {}; artifactize keeps tokens outside repositories. Use a state directory outside it, or set {variable}.",
            crate::platform::path_text(&self.directory),
            crate::platform::path_text(path)
        ))
    }
}

/// An owner-only directory under `$STATE/auth/` of 0600, single-link, no-follow files,
/// replaced atomically. On Windows, owner-only means a protected DACL for the current user.
pub(super) struct Storage {
    pub directory: PathBuf,
}

impl Storage {
    pub fn new(state: Option<&Path>, repo: Option<&Path>, tokens: Tokens) -> Result<Self, String> {
        Self::open(state, repo, tokens, true)
    }

    pub fn inspect(
        state: Option<&Path>,
        repo: Option<&Path>,
        tokens: Tokens,
    ) -> Result<Self, String> {
        Self::open(state, repo, tokens, false)
    }

    fn open(
        state: Option<&Path>,
        repo: Option<&Path>,
        tokens: Tokens,
        create: bool,
    ) -> Result<Self, String> {
        let location = Location::find(state, repo)?;
        if let Some(refusal) = location.refusal(tokens) {
            return Err(refusal);
        }
        let directory = location.directory;
        if create {
            platform::create_private_dir_all(&directory).map_err(|e| e.to_string())?;
        }
        let private = match platform::is_private_dir(&directory) {
            Ok(private) => private,
            Err(error) if !create && error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self { directory });
            }
            Err(error) => return Err(error.to_string()),
        };
        // This is an application-owned directory, not the user's state root.
        if !private {
            return Err("Auth directory must have owner-only permissions (0700).".into());
        }
        Ok(Self { directory })
    }

    /// Hold `<name>.lock` until the returned file drops, serializing credential
    /// refreshes across processes.
    pub async fn lock(&self, name: &str) -> Result<File, String> {
        let file = platform::open_no_follow(
            platform::private_options()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false),
            &self.directory.join(format!("{name}.lock")),
        )
        .map_err(|e| e.to_string())?;
        check_private_file(&file)?;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(file),
                Err(TryLockError::WouldBlock) => {
                    tokio::time::sleep(platform::FILE_LOCK_RETRY_INTERVAL).await
                }
                Err(TryLockError::Error(error)) => return Err(error.to_string()),
            }
        }
    }

    pub fn read<T: DeserializeOwned>(&self, name: &str) -> Result<Option<T>, String> {
        let file = match platform::open_no_follow(
            OpenOptions::new().read(true),
            &self.directory.join(name),
        ) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        check_private_file(&file)?;
        let mut data = Vec::new();
        // One sentinel byte detects an oversized file without reading it in full.
        file.take(MAX_CREDENTIAL_BYTES as u64 + 1)
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        if data.len() > MAX_CREDENTIAL_BYTES {
            return Err("Credential file is too large.".into());
        }
        serde_json::from_slice(&data)
            .map(Some)
            .map_err(|_| format!("Invalid credential file {name}; sign in again."))
    }

    pub fn save(&self, name: &str, value: &impl Serialize) -> Result<(), String> {
        let mut file =
            tempfile::NamedTempFile::new_in(&self.directory).map_err(|e| e.to_string())?;
        platform::restrict_file(file.as_file()).map_err(|e| e.to_string())?;
        serde_json::to_writer(&mut file, value)
            .map_err(|_| "Cannot encode credentials.".to_owned())?;
        file.flush().map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        // std's rename also replaces a file that another process is still reading, which a
        // plain MoveFileEx, as `persist` uses on Windows, refuses.
        let temporary = file.into_temp_path().keep().map_err(|e| e.to_string())?;
        if let Err(error) = fs::rename(&temporary, self.directory.join(name)) {
            let _ = fs::remove_file(&temporary);
            return Err(error.to_string());
        }
        self.sync()
    }

    pub fn remove(&self, name: &str) -> Result<(), String> {
        match fs::remove_file(self.directory.join(name)) {
            Ok(()) => self.sync(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }

    fn sync(&self) -> Result<(), String> {
        platform::sync_dir(&self.directory).map_err(|e| e.to_string())
    }
}

fn check_private_file(file: &File) -> Result<(), String> {
    if !platform::is_private_file(file).map_err(|e| e.to_string())? {
        return Err("Auth files must be regular, single-link, owner-only files (0600).".into());
    }
    Ok(())
}
