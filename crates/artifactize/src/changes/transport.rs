//! Small, deterministic endpoints scoped by canonical state path and the current OS user.
//! Existing paths are validated, never repaired with chmod or a replacement DACL.
use crate::platform;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
};
#[cfg(unix)]
#[path = "transport/unix.rs"]
mod os;
#[cfg(windows)]
#[path = "transport/windows.rs"]
mod os;
pub(super) use os::{Listener, Stream, validate_peer};

/// Use 24 SHA-256 hex characters (96 bits) for the shared runtime-directory namespace,
/// keeping Unix socket paths short. This is not authentication: owner/peer checks and
/// the registration handshake's complete identity remain authoritative on both platforms.
const RUNTIME_ID_PREFIX_HEX_CHARS: usize = 24;

#[derive(Clone)]
pub(super) struct Endpoint {
    pub identity: String,
    directory: PathBuf,
    pub(super) address: PathBuf,
}
impl Endpoint {
    pub fn new(state: &Path) -> io::Result<Self> {
        let state = crate::workspace::canonical_target(state)?;
        let user = os::user()?;
        let mut digest = Sha256::new();
        digest.update(user.as_bytes());
        digest.update([0]);
        digest.update(state.as_os_str().as_encoded_bytes());
        let identity = digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let directory = os::directory(&identity, &user)?;
        let address = os::address(&directory, &identity);
        let endpoint = Self {
            identity,
            directory,
            address,
        };
        endpoint.validate()?;
        Ok(endpoint)
    }
    fn validate(&self) -> io::Result<()> {
        os::validate_directory(&self.directory)
    }
    pub fn elect(&self) -> io::Result<Option<File>> {
        self.validate()?;
        let file = platform::open_no_follow(
            platform::private_options()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false),
            &self.directory.join("owner.lock"),
        )?;
        if !platform::is_private_file(&file)? || !os::owned(&file)? {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        // Stable inode: never unlink this pathname, even after a hub dies.
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(error)) => Err(error),
        }
    }
    pub fn listen(&self) -> io::Result<Listener> {
        self.validate()?;
        os::listen(&self.address)
    }
    pub async fn connect(&self) -> io::Result<Stream> {
        self.validate()?;
        os::connect(&self.address).await
    }
}
