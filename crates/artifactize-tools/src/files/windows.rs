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
    path::{Component, Path, PathBuf, Prefix},
    ptr, slice,
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
            ERROR_NO_MORE_FILES, HANDLE, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE,
            RtlNtStatusToDosError, STATUS_INVALID_PARAMETER, UNICODE_STRING,
        },
        Storage::FileSystem::{
            FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ,
            FILE_ID_BOTH_DIR_INFO, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
            FILE_SHARE_WRITE, FileAttributeTagInfo, FileIdBothDirectoryInfo,
            FileIdBothDirectoryRestartInfo, GetFileInformationByHandleEx, SYNCHRONIZE,
        },
        System::IO::IO_STATUS_BLOCK,
    },
};

use super::FileKind;

/// Reparse tags with this bit name another file: symbolic links, junctions and mount points.
const NAME_SURROGATE: u32 = 0x2000_0000;

/// Open without following a reparse point in the last component. Windows opens the reparse
/// point itself instead of failing, so it is refused after the open.
pub fn open_no_follow(options: &mut OpenOptions, path: &Path) -> io::Result<File> {
    let file = options
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
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

/// The absolute path of an existing file with every link resolved, without the `\\?\`
/// prefix when the plain path names the same file. A logical Artifact path joined to a plain
/// path keeps `/` as a separator, and child programs accept it as a working directory.
pub fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    let path = std::fs::canonicalize(path)?;
    Ok(plain(&path).unwrap_or(path))
}

/// `\\?\C:\a` as `C:\a` and `\\?\UNC\server\share\a` as `\\server\share\a`, when the Win32
/// layer would pass every component through unchanged and the result is below `MAX_PATH`.
fn plain(path: &Path) -> Option<PathBuf> {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return None;
    };
    let mut plain = match prefix.kind() {
        Prefix::VerbatimDisk(drive) => OsString::from(format!("{}:", char::from(drive))),
        Prefix::VerbatimUNC(server, share) => {
            let mut plain = OsString::from(r"\\");
            plain.push(server);
            plain.push(r"\");
            plain.push(share);
            plain
        }
        _ => return None,
    };
    if components.next() != Some(Component::RootDir) {
        return None;
    }
    plain.push(r"\");
    for (index, component) in components.enumerate() {
        let Component::Normal(name) = component else {
            return None;
        };
        if !plain_name(name) {
            return None;
        }
        if index > 0 {
            plain.push(r"\");
        }
        plain.push(name);
    }
    // Win32's legacy MAX_PATH includes the terminating NUL; longer paths must
    // retain their verbatim prefix rather than being rewritten to the plain form.
    const MAX_PLAIN_PATH_UNITS: usize = 260;
    (plain.encode_wide().count() < MAX_PLAIN_PATH_UNITS).then(|| PathBuf::from(plain))
}

/// A name the Win32 layer keeps as is: no trailing dot or space, no reserved character, and
/// no DOS device name such as `NUL` or `com1.txt`.
fn plain_name(name: &OsStr) -> bool {
    // Conservatively permit only short UTF-8 components when removing a
    // verbatim prefix; do not normalize names beyond this existing byte bound.
    const MAX_PLAIN_NAME_BYTES: usize = 255;
    let Some(name) = name.to_str() else {
        return false;
    };
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ');
    let device = match stem.to_ascii_uppercase().as_str() {
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" => true,
        port => {
            (port.starts_with("COM") || port.starts_with("LPT"))
                && port[3..].chars().count() == 1
                && port[3..]
                    .chars()
                    .all(|digit| "0123456789¹²³".contains(digit))
        }
    };
    !name.is_empty()
        && name.len() <= MAX_PLAIN_NAME_BYTES
        && !name.ends_with(['.', ' '])
        && !name
            .chars()
            .any(|c| c < ' ' || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
        && !device
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
            && name.len() <= usize::from(u16::MAX / 2)
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

/// Batch directory records in 64 KiB without unbounded allocation; u64 storage
/// gives the Windows record structs their required eight-byte alignment.
const DIRECTORY_BUFFER_WORDS: usize = 8 * 1024;

/// The entries of a pinned directory, read from its handle rather than a path that could
/// have been replaced by a link.
pub fn read_dir(directory: &File) -> io::Result<impl Iterator<Item = io::Result<DirEntry>> + '_> {
    Ok(ReadDir {
        directory,
        buffer: vec![0; DIRECTORY_BUFFER_WORDS],
        next: None,
        scan: super::directory_scan::DirectoryScan::First,
    })
}

pub struct DirEntry {
    name: OsString,
    kind: FileKind,
}

impl DirEntry {
    pub fn file_name(&self) -> OsString {
        self.name.clone()
    }

    pub fn file_type(&self) -> io::Result<FileKind> {
        Ok(self.kind)
    }
}

struct ReadDir<'a> {
    directory: &'a File,
    buffer: Vec<u64>,
    /// Byte offset of the next record in `buffer`.
    next: Option<usize>,
    scan: super::directory_scan::DirectoryScan,
}

impl Iterator for ReadDir<'_> {
    type Item = io::Result<DirEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(offset) = self.next {
                match self.record(offset) {
                    Ok(Some(entry)) => return Some(Ok(entry)),
                    Ok(None) => continue,
                    Err(error) => return Some(Err(error)),
                }
            }
            let class = match self.scan.query()? {
                super::directory_scan::Query::Restart => FileIdBothDirectoryRestartInfo,
                super::directory_scan::Query::Continue => FileIdBothDirectoryInfo,
            };
            // SAFETY: the buffer is writable for the length passed.
            let ok = unsafe {
                GetFileInformationByHandleEx(
                    self.directory.as_raw_handle(),
                    class,
                    self.buffer.as_mut_ptr().cast(),
                    mem::size_of_val(self.buffer.as_slice()) as u32,
                )
            };
            if ok == 0 {
                self.scan.finish();
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                    return None;
                }
                return Some(Err(error));
            }
            self.next = Some(0);
        }
    }
}

impl ReadDir<'_> {
    /// Decode the record at `offset` and advance past it; `.` and `..` decode to `None`.
    fn record(&mut self, offset: usize) -> io::Result<Option<DirEntry>> {
        let size = mem::size_of_val(self.buffer.as_slice());
        let header = mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
        if !offset.is_multiple_of(8) || offset + header > size {
            self.scan.finish();
            self.next = None;
            return Err(io::Error::other("malformed directory listing"));
        }
        // SAFETY: the record starts inside the buffer at an 8-byte boundary, which suits
        // FILE_ID_BOTH_DIR_INFO, and its fixed fields were checked to fit.
        let info = unsafe {
            self.buffer
                .as_ptr()
                .cast::<u8>()
                .add(offset)
                .cast::<FILE_ID_BOTH_DIR_INFO>()
        };
        // SAFETY: as above; these are plain integer fields.
        let (next, attributes, tag, length) = unsafe {
            (
                (*info).NextEntryOffset as usize,
                (*info).FileAttributes,
                // With FILE_ATTRIBUTE_REPARSE_POINT, this field holds the reparse tag.
                (*info).EaSize,
                (*info).FileNameLength as usize,
            )
        };
        if offset + header + length > size {
            self.scan.finish();
            self.next = None;
            return Err(io::Error::other("malformed directory listing"));
        }
        // SAFETY: the name's bytes were checked to lie inside the buffer, and UTF-16 units
        // are 2-byte aligned after the 8-byte-aligned header.
        let name = unsafe {
            slice::from_raw_parts((&raw const (*info).FileName).cast::<u16>(), length / 2)
        };
        self.next = (next != 0).then_some(offset + next);
        let dot = u16::from(b'.');
        if name == [dot] || name == [dot, dot] {
            return Ok(None);
        }
        Ok(Some(DirEntry {
            name: OsString::from_wide(name),
            kind: kind(attributes, tag),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_records_drain_then_continue_and_malformed_record_ends_iteration() {
        use super::super::directory_scan::{DirectoryScan, Query};
        let directory = tempfile::tempdir().unwrap();
        let handle = open_directory(directory.path()).unwrap();
        let mut reader = ReadDir {
            directory: &handle,
            buffer: vec![0; DIRECTORY_BUFFER_WORDS],
            next: None,
            scan: DirectoryScan::First,
        };
        assert_eq!(reader.scan.query(), Some(Query::Restart));
        let header = mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
        let next_offset = (header + 2).next_multiple_of(8);
        for (offset, next, character) in [(0, next_offset, b'a'), (next_offset, 0, b'b')] {
            // SAFETY: test records fit inside the u64-aligned buffer, just as OS records do.
            let info = unsafe {
                reader
                    .buffer
                    .as_mut_ptr()
                    .cast::<u8>()
                    .add(offset)
                    .cast::<FILE_ID_BOTH_DIR_INFO>()
            };
            // SAFETY: the fixed header and one UTF-16 name unit fit at the checked offsets.
            unsafe {
                (*info).NextEntryOffset = next as u32;
                (*info).FileNameLength = 2;
                (*info).FileAttributes = 0;
                (*info).FileName[0] = u16::from(character);
            }
        }
        reader.next = Some(0);
        assert_eq!(
            reader.next().unwrap().unwrap().file_name(),
            OsString::from("a")
        );
        assert_eq!(
            reader.next().unwrap().unwrap().file_name(),
            OsString::from("b")
        );
        assert_eq!(reader.next, None);
        assert_eq!(reader.scan.query(), Some(Query::Continue));
        reader.next = Some(1); // Invalid record alignment is terminal, not retried.
        assert!(reader.next().unwrap().is_err());
        assert_eq!(reader.scan, DirectoryScan::Done);
        assert!(reader.next().is_none());
        assert!(reader.next().is_none());
    }
}
