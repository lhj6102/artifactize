//! Small, deterministic endpoints scoped by canonical state path and the current OS user.
//! Existing paths are validated, never repaired with chmod or a replacement DACL.
use crate::platform::{self, ipc as os};
pub(super) use os::{Listener, Stream, validate_peer};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub(super) struct Endpoint {
    pub identity: crate::config::EndpointId,
    directory: PathBuf,
    pub(super) address: PathBuf,
}
impl Endpoint {
    pub fn new(state: &Path) -> io::Result<Self> {
        let state = platform::paths::ipc_identity(state)?;
        let user = os::user()?;
        let mut digest = Sha256::new();
        digest.update(user.as_bytes());
        digest.update([0]);
        digest.update(state);
        let identity: crate::config::EndpointId = digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
            .parse()
            .expect("SHA-256 endpoint identity");
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
    /// The private directory that holds the election lock.
    pub(super) fn directory(&self) -> &Path {
        &self.directory
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
        match crate::platform::paths::try_lock(&file) {
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
