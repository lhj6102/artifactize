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
            FileIdBothDirectoryRestartInfo, GetFileInformationByHandleEx,
            GetFinalPathNameByHandleW, SYNCHRONIZE,
        },
        System::IO::IO_STATUS_BLOCK,
    },
};

use super::{
    FileKind,
    directory_scan::{DirectoryScan, Query},
};

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
        port => port
            .strip_prefix("COM")
            .or_else(|| port.strip_prefix("LPT"))
            .is_some_and(|number| {
                number.chars().count() == 1
                    && number.chars().all(|digit| "0123456789¹²³".contains(digit))
            }),
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

/// Batch directory records in 64 KiB without unbounded allocation; u64 storage
/// gives the Windows record structs their required eight-byte alignment.
const DIRECTORY_BUFFER_WORDS: usize = 8 * 1024;
/// FileIdBothDirectoryInfo records start on an eight-byte boundary.
const DIRECTORY_RECORD_ALIGNMENT: usize = 8;
/// Windows names are UTF-16; both UNICODE_STRING and directory records count bytes.
const WIDE_UNIT_BYTES: usize = mem::size_of::<u16>();

/// The entries of a pinned directory, read from its handle rather than a path that could
/// have been replaced by a link.
pub fn read_dir(directory: &File) -> io::Result<impl Iterator<Item = io::Result<DirEntry>> + '_> {
    Ok(ReadDir::new(move |query, buffer: &mut [u64]| {
        query_directory(directory, query, buffer)
    }))
}

/// The native-query seam: choose restart/continuation and fill one aligned record page.
fn query_directory(directory: &File, query: Query, buffer: &mut [u64]) -> io::Result<()> {
    let class = match query {
        Query::Restart => FileIdBothDirectoryRestartInfo,
        Query::Continue => FileIdBothDirectoryInfo,
    };
    // SAFETY: the buffer is writable for the length passed and u64-aligned.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            directory.as_raw_handle(),
            class,
            buffer.as_mut_ptr().cast(),
            mem::size_of_val(buffer) as u32,
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
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

struct ReadDir<Q> {
    query: Q,
    buffer: Vec<u64>,
    /// Byte offset of the next record in `buffer`.
    next: Option<usize>,
    scan: super::directory_scan::DirectoryScan,
}

impl<Q> Iterator for ReadDir<Q>
where
    Q: FnMut(Query, &mut [u64]) -> io::Result<()>,
{
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
            let query = self.scan.query()?;
            if let Err(error) = (self.query)(query, &mut self.buffer) {
                self.scan.finish();
                if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                    return None;
                }
                return Some(Err(error));
            }
            self.next = Some(0);
        }
    }
}

impl<Q> ReadDir<Q> {
    fn new(query: Q) -> Self {
        Self {
            query,
            buffer: vec![0; DIRECTORY_BUFFER_WORDS],
            next: None,
            scan: DirectoryScan::First,
        }
    }

    /// Decode the record at `offset` and advance past it; `.` and `..` decode to `None`.
    fn record(&mut self, offset: usize) -> io::Result<Option<DirEntry>> {
        let size = mem::size_of_val(self.buffer.as_slice());
        let header = mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
        if !offset.is_multiple_of(DIRECTORY_RECORD_ALIGNMENT) || offset + header > size {
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
        if !length.is_multiple_of(WIDE_UNIT_BYTES) || offset + header + length > size {
            self.scan.finish();
            self.next = None;
            return Err(io::Error::other("malformed directory listing"));
        }
        // SAFETY: the name's bytes were checked to lie inside the buffer, and UTF-16 units
        // are 2-byte aligned after the 8-byte-aligned header.
        let name = unsafe {
            slice::from_raw_parts(
                (&raw const (*info).FileName).cast::<u16>(),
                length / WIDE_UNIT_BYTES,
            )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_directory_iteration_continues_after_a_full_record_page() {
        // Six hundred real names exceed the native 64 KiB query buffer, so collecting
        // all of them without a duplicate proves continuation instead of a restart.
        const ENTRIES: usize = 600;
        let directory = crate::test_os::tempdir();
        let expected: std::collections::BTreeSet<_> = (0..ENTRIES)
            .map(|index| format!("entry-{index:04}.txt"))
            .collect();
        for name in &expected {
            std::fs::write(directory.path().join(name), "").unwrap();
        }
        let handle = open_directory(directory.path()).unwrap();
        let mut entries = read_dir(&handle).unwrap();
        let observed: Vec<_> = entries
            .by_ref()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(observed.len(), ENTRIES);
        assert_eq!(
            observed
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>(),
            expected
        );
        assert!(entries.next().is_none());
        assert!(entries.next().is_none());
    }

    /// Encode supplied native records, not iterator cursor/state, into a query response.
    fn page(names: &[&str]) -> Vec<u64> {
        let mut buffer = vec![0; DIRECTORY_BUFFER_WORDS];
        let header = mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
        let mut offset = 0;
        for (index, name) in names.iter().enumerate() {
            let name: Vec<_> = name.encode_utf16().collect();
            let record_bytes = (header + mem::size_of_val(name.as_slice()))
                .next_multiple_of(DIRECTORY_RECORD_ALIGNMENT);
            assert!(offset + record_bytes <= mem::size_of_val(buffer.as_slice()));
            // SAFETY: the complete native header/name fit in an eight-byte-aligned page.
            unsafe {
                let info = buffer
                    .as_mut_ptr()
                    .cast::<u8>()
                    .add(offset)
                    .cast::<FILE_ID_BOTH_DIR_INFO>();
                (*info).NextEntryOffset = if index + 1 == names.len() {
                    0
                } else {
                    record_bytes as u32
                };
                (*info).FileNameLength = mem::size_of_val(name.as_slice()) as u32;
                ptr::copy_nonoverlapping(
                    name.as_ptr(),
                    (&raw mut (*info).FileName).cast::<u16>(),
                    name.len(),
                );
            }
            offset += record_bytes;
        }
        buffer
    }

    #[test]
    fn injected_native_pages_drain_then_continue_without_restarting_or_requerying_eof() {
        let mut replies = std::collections::VecDeque::from([
            (Query::Restart, Some(page(&[".", "..", "a", "b"]))),
            (Query::Continue, Some(page(&["c"]))),
            (Query::Continue, None),
        ]);
        let mut entries = ReadDir::new(move |query, buffer: &mut [u64]| {
            let (expected, response) = replies.pop_front().expect("query after end-of-directory");
            assert_eq!(query, expected, "native query restart/continuation request");
            match response {
                Some(page) => {
                    buffer.copy_from_slice(&page);
                    Ok(())
                }
                None => Err(io::Error::from_raw_os_error(ERROR_NO_MORE_FILES as i32)),
            }
        });
        let observed: Vec<_> = entries
            .by_ref()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            observed,
            [
                OsString::from("a"),
                OsString::from("b"),
                OsString::from("c")
            ]
        );
        assert!(entries.next().is_none());
        assert!(entries.next().is_none());
    }

    #[test]
    fn injected_native_query_errors_and_malformed_records_end_iteration() {
        let mut query_error = ReadDir::new(|_, _: &mut [u64]| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "query refused",
            ))
        });
        assert_eq!(
            query_error.next().unwrap().err().unwrap().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(query_error.next().is_none());
        assert!(query_error.next().is_none());

        // Odd UTF-16 byte count and a name longer than the supplied native page.
        for length in [3, mem::size_of::<u64>() * DIRECTORY_BUFFER_WORDS + 2] {
            let mut response = page(&["a"]);
            // SAFETY: the response contains a complete, aligned FILE_ID_BOTH_DIR_INFO header.
            unsafe {
                (*response.as_mut_ptr().cast::<FILE_ID_BOTH_DIR_INFO>()).FileNameLength =
                    length as u32;
            }
            let mut response = Some(response);
            let mut entries = ReadDir::new(move |_, buffer: &mut [u64]| {
                buffer.copy_from_slice(&response.take().expect("query after malformed record"));
                Ok(())
            });
            assert_eq!(
                entries.next().unwrap().err().unwrap().to_string(),
                "malformed directory listing"
            );
            assert!(entries.next().is_none());
            assert!(entries.next().is_none());
        }

        // A valid first record followed by an unaligned native offset must yield one
        // entry, one error, then terminate without another query.
        let mut response = page(&["a"]);
        // SAFETY: as above; the injected offset is data to validate, not dereferenced here.
        unsafe {
            (*response.as_mut_ptr().cast::<FILE_ID_BOTH_DIR_INFO>()).NextEntryOffset = 1;
        }
        let mut response = Some(response);
        let mut entries = ReadDir::new(move |_, buffer: &mut [u64]| {
            buffer.copy_from_slice(&response.take().expect("query after malformed offset"));
            Ok(())
        });
        assert_eq!(entries.next().unwrap().unwrap().file_name(), "a");
        assert_eq!(
            entries.next().unwrap().err().unwrap().to_string(),
            "malformed directory listing"
        );
        assert!(entries.next().is_none());
        assert!(entries.next().is_none());
    }
}
