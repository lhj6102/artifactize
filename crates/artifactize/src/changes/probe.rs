//! data_version is meaningful only on the same connection. Keep one read-only worker connection
//! until the database's OS file identity changes; also inspect the old open file for replacement.
use crate::{platform, store::DATABASE};
use std::{
    fs::File,
    path::{Path, PathBuf},
};
use tokio_rusqlite::Connection;

#[derive(PartialEq, Eq)]
struct Identity {
    first: u64,
    second: u64,
}
#[cfg(unix)]
fn identity(file: &File) -> Result<Identity, String> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    Ok(Identity {
        first: metadata.dev(),
        second: metadata.ino(),
    })
}
#[cfg(windows)]
fn identity(file: &File) -> Result<Identity, String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: an open file handle and a correctly sized writable output.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(Identity {
        first: u64::from(info.dwVolumeSerialNumber),
        second: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    })
}
struct Open {
    file: File,
    identity: Identity,
    connection: Connection,
    version: i64,
}
pub(super) struct Probe {
    state: PathBuf,
    open: Option<Open>,
}
impl Probe {
    pub fn new(state: PathBuf) -> Self {
        Self { state, open: None }
    }
    pub async fn changed(&mut self) -> bool {
        // Errors request a safe read retry at low frequency, never a hot loop.
        self.check().await.unwrap_or(true)
    }
    async fn check(&mut self) -> Result<bool, String> {
        crate::store::check_probe_files(&self.state)?;
        let path = self.state.join(DATABASE);
        let file = match platform::open_no_follow(File::options().read(true), &path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(self.open.take().is_some());
            }
            Err(error) => {
                self.open = None;
                return Err(error.to_string());
            }
        };
        if !file
            .metadata()
            .map_err(|error| error.to_string())?
            .is_file()
        {
            return Err("State database is not a regular file.".into());
        }
        let current = identity(&file)?;
        if let Some(open) = &mut self.open
            && current == open.identity
            && identity(&open.file)? == current
        {
            let version = data_version(&open.connection).await?;
            let changed = version != open.version;
            open.version = version;
            return Ok(changed);
        }
        let connection = open(&path).await?;
        // Do not retain a connection if its pathname was replaced while opening it.
        let check = platform::open_no_follow(File::options().read(true), &path)
            .map_err(|error| error.to_string())?;
        if identity(&check)? != current {
            self.open = None;
            return Ok(true);
        }
        let version = data_version(&connection).await?;
        self.open = Some(Open {
            file,
            identity: current,
            connection,
            version,
        });
        Ok(true)
    }
}
async fn open(path: &Path) -> Result<Connection, String> {
    let connection = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .await
    .map_err(|error| error.to_string())?;
    connection
        .call(|db| db.busy_timeout(crate::store::SQLITE_BUSY_TIMEOUT))
        .await
        .map_err(|error| error.to_string())?;
    Ok(connection)
}
async fn data_version(connection: &Connection) -> Result<i64, String> {
    connection
        .call(|db| db.pragma_query_value(None, "data_version", |row| row.get(0)))
        .await
        .map_err(|error| error.to_string())
}
