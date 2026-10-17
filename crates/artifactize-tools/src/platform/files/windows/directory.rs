//! Handle-pinned Windows directory queries and bounded native record decoding.
use super::super::directory_scan::{DirectoryScan, Query};
use super::{FileKind, WIDE_UNIT_BYTES, kind};
use std::{
    ffi::OsString,
    fs::File,
    io, mem,
    os::windows::{ffi::OsStringExt, io::AsRawHandle},
    slice,
};
use windows_sys::Win32::{
    Foundation::ERROR_NO_MORE_FILES,
    Storage::FileSystem::{
        FILE_ID_BOTH_DIR_INFO, FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo,
        GetFileInformationByHandleEx,
    },
};

/// Batch directory records in 64 KiB without unbounded allocation; u64 storage
/// gives the Windows record structs their required eight-byte alignment.
const DIRECTORY_BUFFER_WORDS: usize = 8 * 1024;
/// FileIdBothDirectoryInfo records start on an eight-byte boundary.
const DIRECTORY_RECORD_ALIGNMENT: usize = 8;

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
    scan: DirectoryScan,
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

// These tests exercise Windows-native directory query buffers, unavailable on other OSes.
#[cfg(all(test, windows))]
mod tests {
    use super::super::open_directory;
    use super::*;
    use std::ptr;

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
