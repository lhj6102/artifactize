//! Handle-pinned Windows directory queries and bounded native record decoding.
use super::{FileKind, WIDE_UNIT_BYTES, kind};
use std::{
    ffi::OsString,
    fs::File,
    io, mem,
    os::windows::{ffi::OsStringExt, io::AsRawHandle},
};
use windows_sys::Win32::{
    Foundation::ERROR_NO_MORE_FILES,
    Storage::FileSystem::{
        FILE_ID_BOTH_DIR_INFO, FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo,
        GetFileInformationByHandleEx,
    },
};

/// Batch directory records in 64 KiB without unbounded allocation.
const DIRECTORY_PAGE_BYTES: usize = 64 * 1024;
/// FileIdBothDirectoryInfo records start on an eight-byte boundary.
const DIRECTORY_RECORD_ALIGNMENT: usize = 8;

/// The native query requires aligned storage; decoding only uses checked byte slices.
#[repr(align(8))]
struct Page([u8; DIRECTORY_PAGE_BYTES]);

/// The entries of a pinned directory, read from its handle rather than a path that could
/// have been replaced by a link.
pub fn read_dir(directory: &File) -> io::Result<impl Iterator<Item = io::Result<DirEntry>> + '_> {
    Ok(ReadDir::new(move |query, buffer: &mut Page| {
        query_directory(directory, query, buffer)
    }))
}

/// The native-query seam: choose restart/continuation and fill one aligned record page.
fn query_directory(directory: &File, query: Query, buffer: &mut Page) -> io::Result<()> {
    let class = match query {
        Query::Restart => FileIdBothDirectoryRestartInfo,
        Query::Continue => FileIdBothDirectoryInfo,
    };
    // SAFETY: the buffer is writable for the length passed and u64-aligned.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            directory.as_raw_handle(),
            class,
            buffer.0.as_mut_ptr().cast(),
            DIRECTORY_PAGE_BYTES as u32,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Query {
    Restart,
    Continue,
}

#[derive(Clone, Copy)]
enum Scan {
    Query(Query),
    /// Byte offset of the next record in the current page.
    Record(usize),
    Done,
}

struct ReadDir<Q> {
    query: Q,
    buffer: Box<Page>,
    scan: Scan,
}

impl<Q> Iterator for ReadDir<Q>
where
    Q: FnMut(Query, &mut Page) -> io::Result<()>,
{
    type Item = io::Result<DirEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let query = match self.scan {
                Scan::Record(offset) => match self.record(offset) {
                    Ok(Some(entry)) => return Some(Ok(entry)),
                    Ok(None) => continue,
                    Err(error) => return Some(Err(error)),
                },
                Scan::Query(query) => query,
                Scan::Done => return None,
            };
            if let Err(error) = (self.query)(query, &mut self.buffer) {
                self.scan = Scan::Done;
                if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                    return None;
                }
                return Some(Err(error));
            }
            self.scan = Scan::Record(0);
        }
    }
}

impl<Q> ReadDir<Q> {
    fn new(query: Q) -> Self {
        Self {
            query,
            buffer: Box::new(Page([0; DIRECTORY_PAGE_BYTES])),
            scan: Scan::Query(Query::Restart),
        }
    }

    /// Decode the record at `offset` and advance past it; `.` and `..` decode to `None`.
    fn record(&mut self, offset: usize) -> io::Result<Option<DirEntry>> {
        let Some((next, entry)) = decode_record(&self.buffer.0, offset) else {
            self.scan = Scan::Done;
            return Err(io::Error::other("malformed directory listing"));
        };
        self.scan = next.map_or(Scan::Query(Query::Continue), Scan::Record);
        Ok(entry)
    }
}

fn decode_record(page: &[u8], offset: usize) -> Option<(Option<usize>, Option<DirEntry>)> {
    if !offset.is_multiple_of(DIRECTORY_RECORD_ALIGNMENT) {
        return None;
    }
    let record = page.get(offset..)?;
    let field = |start: usize| {
        let bytes = record.get(start..start + mem::size_of::<u32>())?;
        Some(u32::from_le_bytes(bytes.try_into().ok()?))
    };
    let next = usize::try_from(field(mem::offset_of!(
        FILE_ID_BOTH_DIR_INFO,
        NextEntryOffset
    ))?)
    .ok()?;
    let attributes = field(mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileAttributes))?;
    // When FILE_ATTRIBUTE_REPARSE_POINT is set, EaSize carries the reparse tag.
    let tag = field(mem::offset_of!(FILE_ID_BOTH_DIR_INFO, EaSize))?;
    let length = usize::try_from(field(mem::offset_of!(
        FILE_ID_BOTH_DIR_INFO,
        FileNameLength
    ))?)
    .ok()?;
    if length == 0 || !length.is_multiple_of(WIDE_UNIT_BYTES) {
        return None;
    }
    let name_start = mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
    let name_end = name_start.checked_add(length)?;
    let next = match next {
        0 => None,
        next if next >= name_end
            && next < record.len()
            && next.is_multiple_of(DIRECTORY_RECORD_ALIGNMENT) =>
        {
            Some(offset.checked_add(next)?)
        }
        _ => return None,
    };
    let name: Vec<_> = record
        .get(name_start..name_end)?
        .as_chunks::<WIDE_UNIT_BYTES>()
        .0
        .iter()
        .map(|bytes| u16::from_le_bytes(*bytes))
        .collect();
    let dot = u16::from(b'.');
    let entry = (name != [dot] && name != [dot, dot]).then(|| DirEntry {
        name: OsString::from_wide(&name),
        kind: kind(attributes, tag),
    });
    Some((next, entry))
}

// These tests exercise Windows-native directory query buffers, unavailable on other OSes.
#[cfg(all(test, windows))]
mod tests {
    use super::super::open_directory;
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
    fn page(names: &[&str]) -> Page {
        let mut buffer = Page([0; DIRECTORY_PAGE_BYTES]);
        let header = mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
        let mut offset = 0;
        for (index, name) in names.iter().enumerate() {
            let name: Vec<_> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
            let record_bytes = (header + name.len()).next_multiple_of(DIRECTORY_RECORD_ALIGNMENT);
            let record = &mut buffer.0[offset..offset + record_bytes];
            let next = if index + 1 == names.len() {
                0
            } else {
                record_bytes
            };
            write_field(
                record,
                mem::offset_of!(FILE_ID_BOTH_DIR_INFO, NextEntryOffset),
                next,
            );
            write_field(
                record,
                mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileNameLength),
                name.len(),
            );
            record[header..header + name.len()].copy_from_slice(&name);
            offset += record_bytes;
        }
        buffer
    }

    fn write_field(record: &mut [u8], offset: usize, value: usize) {
        let value = u32::try_from(value).unwrap().to_le_bytes();
        record[offset..offset + value.len()].copy_from_slice(&value);
    }

    #[test]
    fn injected_native_pages_drain_then_continue_without_restarting_or_requerying_eof() {
        let mut replies = std::collections::VecDeque::from([
            (Query::Restart, Some(page(&[".", "..", "a", "b"]))),
            (Query::Continue, Some(page(&["c"]))),
            (Query::Continue, None),
        ]);
        let mut entries = ReadDir::new(move |query, buffer: &mut Page| {
            let (expected, response) = replies.pop_front().expect("query after end-of-directory");
            assert_eq!(query, expected, "native query restart/continuation request");
            match response {
                Some(page) => {
                    buffer.0.copy_from_slice(&page.0);
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
        let mut query_error = ReadDir::new(|_, _: &mut Page| {
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
        for length in [3, DIRECTORY_PAGE_BYTES + 2] {
            let mut response = page(&["a"]);
            write_field(
                &mut response.0,
                mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileNameLength),
                length,
            );
            let mut response = Some(response);
            let mut entries = ReadDir::new(move |_, buffer: &mut Page| {
                buffer
                    .0
                    .copy_from_slice(&response.take().expect("query after malformed record").0);
                Ok(())
            });
            assert_eq!(
                entries.next().unwrap().err().unwrap().to_string(),
                "malformed directory listing"
            );
            assert!(entries.next().is_none());
            assert!(entries.next().is_none());
        }

        // An overlapping, unaligned native offset must end iteration without another query.
        let mut response = page(&["a"]);
        write_field(
            &mut response.0,
            mem::offset_of!(FILE_ID_BOTH_DIR_INFO, NextEntryOffset),
            1,
        );
        let mut response = Some(response);
        let mut entries = ReadDir::new(move |_, buffer: &mut Page| {
            buffer
                .0
                .copy_from_slice(&response.take().expect("query after malformed offset").0);
            Ok(())
        });
        assert_eq!(
            entries.next().unwrap().err().unwrap().to_string(),
            "malformed directory listing"
        );
        assert!(entries.next().is_none());
        assert!(entries.next().is_none());
    }
}
