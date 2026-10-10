use crate::platform;
use std::{
    fs::{self, File},
    io,
    os::windows::{fs::MetadataExt, io::AsRawHandle},
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::windows::named_pipe::{NamedPipeClient, NamedPipeServer, ServerOptions},
};
use windows_sys::Win32::{
    Foundation::{GENERIC_READ, GENERIC_WRITE},
    Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OVERLAPPED, READ_CONTROL, SECURITY_IDENTIFICATION,
        SECURITY_SQOS_PRESENT,
    },
};

pub(super) fn user() -> io::Result<String> {
    platform::user_identity()
}
pub(super) fn owned(file: &File) -> io::Result<bool> {
    platform::is_private_file(file)
}
pub(super) fn validate_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || !platform::is_private_dir(path)?
    {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(())
}
pub(super) fn directory(identity: &str, _user: &str) -> io::Result<PathBuf> {
    let directory = std::env::temp_dir().join(format!(
        "artifactize-ipc-{}",
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
pub(super) fn address(_directory: &Path, identity: &str) -> PathBuf {
    PathBuf::from(format!(r"\\.\pipe\artifactize-changes-{identity}"))
}
fn instance(address: &Path, first: bool) -> io::Result<NamedPipeServer> {
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true)
        .access_system_security(false);
    platform::private_pipe(&options, address)
}
pub(crate) struct Listener {
    address: PathBuf,
    pending: Option<NamedPipeServer>,
}
pub(super) fn listen(address: &Path) -> io::Result<Listener> {
    Ok(Listener {
        address: address.into(),
        pending: Some(instance(address, true)?),
    })
}
impl Listener {
    pub async fn accept(&mut self) -> io::Result<Stream> {
        self.pending
            .as_ref()
            .expect("listening instance")
            .connect()
            .await?;
        // Keep a listening instance alive before handing the connected one to a client task.
        let next = instance(&self.address, false)?;
        let connected = self.pending.replace(next).expect("connected instance");
        Ok(Stream::Server(connected))
    }
}
// The private pipe DACL and reject_remote_clients enforce peer access at connect.
pub(crate) fn validate_peer(_stream: &Stream) -> io::Result<()> {
    Ok(())
}
pub(super) async fn connect(address: &Path) -> io::Result<Stream> {
    use std::os::windows::{fs::OpenOptionsExt, io::IntoRawHandle};
    // Check the exact connected pipe object, not a second instance that could race it.
    // Identification prevents the server from impersonating this client.
    let file = File::options()
        .access_mode(GENERIC_READ | GENERIC_WRITE | READ_CONTROL)
        .custom_flags(FILE_FLAG_OVERLAPPED | SECURITY_IDENTIFICATION | SECURITY_SQOS_PRESENT)
        .open(address)?;
    if !platform::is_owner_only(file.as_raw_handle())? {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    // SAFETY: the overlapped pipe handle is transferred exactly once to Tokio.
    let client = unsafe { NamedPipeClient::from_raw_handle(file.into_raw_handle()) }?;
    Ok(Stream::Client(client))
}
pub(crate) enum Stream {
    Server(NamedPipeServer),
    Client(NamedPipeClient),
}
impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Server(stream) => Pin::new(stream).poll_read(cx, buffer),
            Self::Client(stream) => Pin::new(stream).poll_read(cx, buffer),
        }
    }
}
impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Server(stream) => Pin::new(stream).poll_write(cx, bytes),
            Self::Client(stream) => Pin::new(stream).poll_write(cx, bytes),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Server(stream) => Pin::new(stream).poll_flush(cx),
            Self::Client(stream) => Pin::new(stream).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Server(stream) => Pin::new(stream).poll_shutdown(cx),
            Self::Client(stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}
