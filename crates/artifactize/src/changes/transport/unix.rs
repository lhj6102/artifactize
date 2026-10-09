use crate::platform;
use std::{
    fs::{self, File},
    io,
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
};
use tokio::net::{UnixListener, UnixStream};
pub(crate) type Stream = UnixStream;
pub(crate) struct Listener {
    listener: UnixListener,
}
pub(super) fn user() -> io::Result<String> {
    // SAFETY: geteuid has no preconditions and does not mutate process identity.
    Ok(unsafe { libc::geteuid() }.to_string())
}
pub(super) fn owned(file: &File) -> io::Result<bool> {
    Ok(file.metadata()?.uid().to_string() == user()?)
}
pub(super) fn validate_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.uid().to_string() != user()?
        || metadata.mode() & platform::GROUP_OTHER_BITS != 0
    {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(())
}
pub(super) fn directory(identity: &str, user: &str) -> io::Result<PathBuf> {
    // The standard OS temporary root is shared but sticky. The 0700 child is both short
    // enough for sockaddr_un and owned by this user. No environment-chosen endpoint path.
    let root = Path::new("/tmp");
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.is_dir() || metadata.mode() & libc::S_ISVTX == 0 {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    let directory = root.join(format!(
        "artifactize-ipc-{user}-{}",
        &identity[..super::RUNTIME_ID_PREFIX_HEX_CHARS]
    ));
    match platform::create_private_dir(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    validate_directory(&directory)?;
    Ok(directory)
}
pub(super) fn address(directory: &Path, _identity: &str) -> PathBuf {
    directory.join("hub.sock")
}
fn validate_socket(address: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(address)?;
    if !metadata.file_type().is_socket() || metadata.uid().to_string() != user()? {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(())
}
pub(super) fn listen(address: &Path) -> io::Result<Listener> {
    // Only an elected owner reaches here; a crashed predecessor's socket can be removed.
    match validate_socket(address) {
        Ok(()) => fs::remove_file(address)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(address)?;
    Ok(Listener { listener })
}
pub(super) async fn connect(address: &Path) -> io::Result<Stream> {
    validate_socket(address)?;
    let stream = UnixStream::connect(address).await?;
    if stream.peer_cred()?.uid().to_string() != user()? {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(stream)
}
impl Listener {
    pub async fn accept(&mut self) -> io::Result<Stream> {
        let (stream, _) = self.listener.accept().await?;
        if stream.peer_cred()?.uid().to_string() != user()? {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(stream)
    }
}
