//! Windows pinned handle-relative opens and listings refuse every reparse point.

use std::{
    ffi::{OsStr, OsString},
    fs::{File, OpenOptions},
    io, mem,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::{MetadataExt, OpenOptionsExt},
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Path, PathBuf},
    ptr,
};

use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_OPEN, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
        },
    },
    Win32::{
        Foundation::{
            HANDLE, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE, RtlNtStatusToDosError,
            STATUS_INVALID_PARAMETER, UNICODE_STRING,
        },
        Storage::FileSystem::{
            FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ,
            FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            FileAttributeTagInfo, GetFileInformationByHandleEx, GetFinalPathNameByHandleW,
            SYNCHRONIZE,
        },
        System::IO::IO_STATUS_BLOCK,
    },
};

use super::FileKind;

mod directory;
mod path;
pub use directory::read_dir;
pub use path::canonicalize;

/// Reparse tags with this bit name another file: symbolic links, junctions and mount points.
const NAME_SURROGATE: u32 = 0x2000_0000;

/// Open without following a reparse point in the last component. Windows opens the reparse
/// point itself instead of failing, so it is refused after the open.
pub fn open_no_follow(options: &mut OpenOptions, path: &Path) -> io::Result<File> {
    // Backup semantics let the same open reach a directory, as an ordinary Unix open does.
    let file = options
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(reparse_point());
    }
    Ok(file)
}

/// The filesystem has no FIFOs, so an ordinary open cannot block. Directories open too, as
/// on Unix, so the caller's type check reports them.
pub fn open_nonblocking(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
}

/// Open a directory by path, such as the root a scoped walk starts from.
pub fn open_directory(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
}

/// One path component as UTF-16: never empty, `.` or `..`, and without separators, stream
/// names or NUL, so a relative open cannot reach past the pinned directory.
pub struct EntryName(Vec<u16>);

impl EntryName {
    pub fn new(name: &OsStr) -> Option<Self> {
        let name: Vec<u16> = name.encode_wide().collect();
        let dots = |count| name.len() == count && name.iter().all(|&unit| unit == u16::from(b'.'));
        // UNICODE_STRING stores its byte length in u16; every UTF-16 unit takes two bytes.
        let valid = !name.is_empty()
            && name.len() <= usize::from(u16::MAX) / WIDE_UNIT_BYTES
            && !dots(1)
            && !dots(2)
            && !name
                .iter()
                .any(|&unit| [0, b'/', b':', b'\\'].map(u16::from).contains(&unit));
        valid.then_some(Self(name))
    }
}

/// Open one entry of a pinned directory read-only, refusing any reparse point.
pub fn open_entry(directory: &File, name: &EntryName) -> io::Result<File> {
    let file = open_relative(directory, &name.0, FILE_GENERIC_READ)?;
    if attribute_tag(&file)?.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(reparse_point());
    }
    Ok(file)
}

/// The type of one entry of a pinned directory, without following it.
pub fn entry_kind(directory: &File, name: &OsStr) -> io::Result<FileKind> {
    let name = EntryName::new(name).ok_or(io::ErrorKind::InvalidInput)?;
    let file = open_relative(directory, &name.0, FILE_READ_ATTRIBUTES | SYNCHRONIZE)?;
    let info = attribute_tag(&file)?;
    Ok(kind(info.FileAttributes, info.ReparseTag))
}

/// `NtCreateFile` relative to a pinned directory handle: the Windows `openat`. The name is a
/// single component, and a final reparse point is opened rather than followed.
fn open_relative(directory: &File, name: &[u16], access: u32) -> io::Result<File> {
    let length = u16::try_from(mem::size_of_val(name)).map_err(|_| io::ErrorKind::InvalidInput)?;
    let name = UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: name.as_ptr().cast_mut(),
    };
    let mut attributes = OBJECT_ATTRIBUTES {
        Length: mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: directory.as_raw_handle(),
        ObjectName: &name,
        // Case-insensitive like every Win32 open; OBJ_DONT_REPARSE also fails the open if
        // any reparse point would be followed.
        Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        ..Default::default()
    };
    let open = |attributes: &OBJECT_ATTRIBUTES, handle: &mut HANDLE| {
        let mut status = IO_STATUS_BLOCK::default();
        // SAFETY: every pointer refers to a live local; NtCreateFile writes only `handle`
        // and `status`, and does not keep the name.
        unsafe {
            NtCreateFile(
                handle,
                access,
                attributes,
                &mut status,
                ptr::null(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                FILE_OPEN,
                FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
                ptr::null(),
                0,
            )
        }
    };
    let mut handle = ptr::null_mut();
    let mut status = open(&attributes, &mut handle);
    if status == STATUS_INVALID_PARAMETER {
        // Windows before 10 1607 lacks OBJ_DONT_REPARSE; FILE_OPEN_REPARSE_POINT still keeps
        // the single component from being followed.
        attributes.Attributes = OBJ_CASE_INSENSITIVE;
        status = open(&attributes, &mut handle);
    }
    if status < 0 {
        // SAFETY: a pure status-code conversion.
        let error = unsafe { RtlNtStatusToDosError(status) };
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    // SAFETY: NtCreateFile succeeded, so `handle` is a new handle that nothing else owns.
    Ok(unsafe { File::from_raw_handle(handle) })
}

fn attribute_tag(file: &File) -> io::Result<FILE_ATTRIBUTE_TAG_INFO> {
    let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: the buffer is a FILE_ATTRIBUTE_TAG_INFO of the size passed.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            (&raw mut info).cast(),
            mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(info)
}

fn kind(attributes: u32, reparse_tag: u32) -> FileKind {
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        if reparse_tag & NAME_SURROGATE != 0 {
            FileKind::Symlink
        } else {
            FileKind::Other
        }
    } else if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        FileKind::Directory
    } else {
        FileKind::File
    }
}

/// The error for a refused reparse point, which `is_link_refusal` recognizes.
fn reparse_point() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, ReparsePoint)
}

#[derive(Debug)]
struct ReparsePoint;

impl std::fmt::Display for ReparsePoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("refusing a symbolic link, junction or other reparse point")
    }
}

impl std::error::Error for ReparsePoint {}

/// Whether `open_entry` or `open_no_follow` failed because the entry is a reparse point.
pub fn is_link_refusal(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|inner| inner.is::<ReparsePoint>())
}

/// Windows names are UTF-16; both UNICODE_STRING and directory records count bytes.
const WIDE_UNIT_BYTES: usize = mem::size_of::<u16>();

/// Require the opened handle's normalized, long entry name. Case and 8.3 aliases must not
/// bypass case-sensitive logical ownership; per-directory case-sensitive names still work.
pub fn exact_name(file: &File, name: &OsStr) -> io::Result<bool> {
    // Windows' extended path limit includes the NUL. A bounded buffer avoids trusting an
    // unbounded allocation size returned by a concurrently renamed entry.
    const MAX_PATH_UNITS: usize = 32_768;
    let mut path = vec![0; MAX_PATH_UNITS];
    // SAFETY: a live handle and a writable buffer of the stated size. Flags 0 requests the
    // normalized path, not the spelling used to open the handle.
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            path.as_mut_ptr(),
            path.len() as u32,
            0,
        )
    } as usize;
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    if length >= path.len() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let path = PathBuf::from(OsString::from_wide(&path[..length]));
    Ok(path.file_name() == Some(name))
}

/// Windows logical names remain conservative even if per-directory case sensitivity is
/// enabled: NtCreateFile here explicitly uses OBJ_CASE_INSENSITIVE.
pub fn case_sensitive(_directory: &File) -> io::Result<bool> {
    Ok(false)
}
