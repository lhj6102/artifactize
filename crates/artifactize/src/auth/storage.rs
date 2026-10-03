use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Serialize, de::DeserializeOwned};

pub(super) struct Storage {
    pub directory: PathBuf,
}

impl Storage {
    pub fn new(state: Option<&Path>, repo: Option<&Path>) -> Result<Self, String> {
        Self::open(state, repo, true)
    }

    pub fn inspect(state: Option<&Path>, repo: Option<&Path>) -> Result<Self, String> {
        Self::open(state, repo, false)
    }

    fn open(state: Option<&Path>, repo: Option<&Path>, create: bool) -> Result<Self, String> {
        let directory = crate::store::state_dir(state)?.join("auth");
        let directory =
            crate::workspace::canonical_target(&directory).map_err(|e| e.to_string())?;
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        let input = repo
            .unwrap_or(&cwd)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let input = if input.is_file() {
            input.parent().unwrap()
        } else {
            &input
        };
        if repo.is_some() {
            crate::workspace::outside_workspace(input, &directory).map_err(|e| e.to_string())?;
        }
        for ancestor in input.ancestors().chain(directory.ancestors()) {
            if ancestor.join(".git").exists() || ancestor.join("artifactize.json").exists() {
                crate::workspace::outside_workspace(ancestor, &directory)
                    .map_err(|e| e.to_string())?;
            }
        }
        if create {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&directory)
                .map_err(|e| e.to_string())?;
        }
        let metadata = match directory.metadata() {
            Ok(metadata) => metadata,
            Err(error) if !create && error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self { directory });
            }
            Err(error) => return Err(error.to_string()),
        };
        // This is an application-owned directory, not the user's state root.
        if metadata.mode() & 0o077 != 0 {
            return Err("ChatGPT auth directory must have owner-only permissions (0700).".into());
        }
        Ok(Self { directory })
    }

    pub async fn lock(&self) -> Result<File, String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(self.directory.join("chatgpt.lock"))
            .map_err(|e| e.to_string())?;
        check_private_file(&file)?;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(file),
                Err(TryLockError::WouldBlock) => {
                    tokio::time::sleep(Duration::from_millis(25)).await
                }
                Err(TryLockError::Error(error)) => return Err(error.to_string()),
            }
        }
    }

    pub fn read<T: DeserializeOwned>(&self, name: &str) -> Result<Option<T>, String> {
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(self.directory.join(name))
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        check_private_file(&file)?;
        let mut data = Vec::new();
        file.take(1024 * 1024 + 1)
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        if data.len() > 1024 * 1024 {
            return Err("ChatGPT credential file is too large.".into());
        }
        serde_json::from_slice(&data)
            .map(Some)
            .map_err(|_| "Invalid ChatGPT credential file; run `artifactize login chatgpt`.".into())
    }

    pub fn save(&self, name: &str, value: &impl Serialize) -> Result<(), String> {
        let mut file =
            tempfile::NamedTempFile::new_in(&self.directory).map_err(|e| e.to_string())?;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
        serde_json::to_writer(&mut file, value)
            .map_err(|_| "Cannot encode ChatGPT credentials.".to_owned())?;
        file.flush().map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        file.persist(self.directory.join(name))
            .map_err(|e| e.error.to_string())?;
        self.sync()
    }

    pub fn remove_credentials(&self) -> Result<(), String> {
        match fs::remove_file(self.directory.join(super::CREDENTIALS)) {
            Ok(()) => self.sync(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }

    fn sync(&self) -> Result<(), String> {
        File::open(&self.directory)
            .and_then(|dir| dir.sync_all())
            .map_err(|e| e.to_string())
    }
}

fn check_private_file(file: &File) -> Result<(), String> {
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.mode() & 0o777 != 0o600 || metadata.nlink() != 1 {
        return Err(
            "ChatGPT auth files must be regular, single-link, owner-only files (0600).".into(),
        );
    }
    Ok(())
}
