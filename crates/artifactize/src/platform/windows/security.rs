//! Owner-only files and directories: a protected DACL whose single ACE grants the current
//! user full control, the Windows counterpart of modes 0700 and 0600.

use std::{
    ffi::c_void,
    fs::{File, OpenOptions},
    io, mem,
    os::windows::{
        ffi::OsStrExt,
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr, slice,
};

use tempfile::TempDir;
use windows_sys::Win32::{
    Foundation::{ERROR_SUCCESS, HANDLE, INVALID_HANDLE_VALUE, LocalFree},
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT, SetSecurityInfo,
        },
        DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetAclInformation, GetSecurityDescriptorDacl,
        GetTokenInformation, INHERIT_ONLY_ACE, IsWellKnownSid, OWNER_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
        TokenUser, WinBuiltinAdministratorsSid, WinLocalSystemSid,
    },
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateDirectoryW, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TYPE_DISK,
        GetFileInformationByHandle, GetFileType, READ_CONTROL, ReOpenFile, WRITE_DAC,
    },
    System::{
        SystemServices::{
            ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE, ACCESS_DENIED_CALLBACK_ACE_TYPE,
            ACCESS_DENIED_CALLBACK_OBJECT_ACE_TYPE, ACCESS_DENIED_OBJECT_ACE_TYPE,
        },
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

/// Create a directory and its missing parents, each owner-only from the start; existing
/// ones keep their DACL.
pub(crate) fn create_private_dir_all(path: &Path) -> io::Result<()> {
    match create_private_dir(path) {
        Ok(()) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) if path.is_dir() => return Ok(()),
        Err(error) => return Err(error),
    }
    match path.parent() {
        Some(parent) => create_private_dir_all(parent)?,
        None => return Err(io::Error::other("failed to create whole tree")),
    }
    match create_private_dir(path) {
        Err(_) if path.is_dir() => Ok(()),
        result => result,
    }
}

/// Create one directory with an owner-only DACL that new files and subdirectories inherit.
pub(crate) fn create_private_dir(path: &Path) -> io::Result<()> {
    let descriptor = owner_only(true)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let path = wide(path)?;
    // SAFETY: a NUL-terminated path and a valid security descriptor, both alive for the call.
    if unsafe { CreateDirectoryW(path.as_ptr(), &attributes) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// A new owner-only temporary directory below `parent`. It inherits `parent`'s DACL until
/// the protected one replaces it, which is owner-only too when artifactize made `parent`.
pub(crate) fn private_tempdir_in(prefix: &str, parent: &Path) -> io::Result<TempDir> {
    let directory = tempfile::Builder::new().prefix(prefix).tempdir_in(parent)?;
    let opened = OpenOptions::new()
        .access_mode(READ_CONTROL | WRITE_DAC)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(directory.path())?;
    restrict(opened.as_raw_handle(), true)?;
    Ok(directory)
}

/// Whether only the current user can use a directory; see `is_owner_only`.
pub(crate) fn is_private_dir(path: &Path) -> io::Result<bool> {
    let directory = OpenOptions::new()
        .access_mode(READ_CONTROL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    is_owner_only(directory.as_raw_handle())
}

/// Whether an open file is a regular, single-link disk file that only the current user can
/// use: the counterparts of Unix's file type, link count and 0600 checks.
pub(crate) fn is_private_file(file: &File) -> io::Result<bool> {
    let handle = file.as_raw_handle();
    // SAFETY: an open file handle.
    if unsafe { GetFileType(handle) } != FILE_TYPE_DISK {
        return Ok(false);
    }
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: an open file handle and a valid out pointer.
    if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0
        || info.nNumberOfLinks != 1
    {
        return Ok(false);
    }
    is_owner_only(handle)
}

/// Windows options cannot carry a security descriptor: a new file inherits the owner-only
/// DACL of its private directory, and `is_private_file` verifies the result.
pub(crate) fn private_options() -> OpenOptions {
    OpenOptions::new()
}

/// Replace an open file's DACL with a protected owner-only one.
pub(crate) fn restrict_file(file: &File) -> io::Result<()> {
    // The original handle may lack WRITE_DAC; reopening the same file object needs no path.
    // SAFETY: an open file handle; the result is checked and then owned.
    let handle = unsafe {
        ReOpenFile(
            file.as_raw_handle(),
            READ_CONTROL | WRITE_DAC,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            0,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: ReOpenFile returned a new handle that nothing else owns.
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    restrict(handle.as_raw_handle(), false)
}

fn restrict(handle: HANDLE, directory: bool) -> io::Result<()> {
    let descriptor = owner_only(directory)?;
    let (mut present, mut defaulted) = (0, 0);
    let mut dacl: *mut ACL = ptr::null_mut();
    // SAFETY: a valid descriptor and out pointers; `dacl` points into the descriptor.
    if unsafe { GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: an open handle with WRITE_DAC and a DACL that outlives the call.
    let error = unsafe {
        SetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null(),
        )
    };
    if error != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    Ok(())
}

/// The closest Windows reading of Unix's "no group or other bits": the owner is the current
/// user, or Administrators or SYSTEM, who can take ownership of anything anyway; and every
/// ACE that grants access to the object names the current user. Deny ACEs and inherit-only
/// ACEs grant nothing here; any other kind of ACE fails the check, as does a NULL DACL.
pub(crate) fn is_owner_only(handle: HANDLE) -> io::Result<bool> {
    let mut owner: PSID = ptr::null_mut();
    let mut dacl: *mut ACL = ptr::null_mut();
    let mut descriptor = ptr::null_mut();
    // SAFETY: an open handle with READ_CONTROL; the out pointers point into `descriptor`,
    // which is freed only after the last use below.
    let error = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if error != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    let _descriptor = Local(descriptor);
    let user = current_user()?;
    let user = user.sid();
    // SAFETY: valid SIDs from GetSecurityInfo and the token.
    let owned = unsafe {
        EqualSid(owner, user) != 0
            || IsWellKnownSid(owner, WinBuiltinAdministratorsSid) != 0
            || IsWellKnownSid(owner, WinLocalSystemSid) != 0
    };
    if !owned || dacl.is_null() {
        return Ok(false);
    }
    let mut size = ACL_SIZE_INFORMATION::default();
    // SAFETY: a valid ACL and an output buffer of the size passed.
    if unsafe {
        GetAclInformation(
            dacl,
            (&raw mut size).cast(),
            mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    for index in 0..size.AceCount {
        let mut ace: *mut c_void = ptr::null_mut();
        // SAFETY: the index is below the ACL's ACE count.
        if unsafe { GetAce(dacl, index, &mut ace) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: every ACE starts with an ACE_HEADER.
        let header = unsafe { ace.cast::<ACE_HEADER>().read_unaligned() };
        if u32::from(header.AceFlags) & INHERIT_ONLY_ACE != 0 {
            continue;
        }
        match u32::from(header.AceType) {
            ACCESS_ALLOWED_ACE_TYPE => {
                // SAFETY: an ACCESS_ALLOWED_ACE's SID starts at its SidStart field.
                let sid = unsafe { &raw mut (*ace.cast::<ACCESS_ALLOWED_ACE>()).SidStart };
                // SAFETY: both are valid SIDs.
                if unsafe { EqualSid(sid.cast(), user) } == 0 {
                    return Ok(false);
                }
            }
            ACCESS_DENIED_ACE_TYPE
            | ACCESS_DENIED_OBJECT_ACE_TYPE
            | ACCESS_DENIED_CALLBACK_ACE_TYPE
            | ACCESS_DENIED_CALLBACK_OBJECT_ACE_TYPE => {}
            _ => return Ok(false),
        }
    }
    Ok(true)
}

/// A protected DACL with one ACE granting the current user full control. A directory's ACE
/// is inherited by the files and subdirectories created in it.
fn owner_only(directory: bool) -> io::Result<Local> {
    let sid = user_identity()?;
    let inheritance = if directory { "OICI" } else { "" };
    let sddl: Vec<u16> = format!("O:{sid}D:P(A;{inheritance};FA;;;{sid})")
        .encode_utf16()
        .chain([0])
        .collect();
    let mut descriptor = ptr::null_mut();
    // SAFETY: a NUL-terminated SDDL string; the descriptor is freed by `Local`.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(Local(descriptor))
}

/// Stable current-token SID, not an environment-provided username.
pub(crate) fn user_identity() -> io::Result<String> {
    let user = current_user()?;
    let mut text = ptr::null_mut();
    // SAFETY: a valid SID; the string is allocated by LocalAlloc and freed by `Local`.
    if unsafe { ConvertSidToStringSidW(user.sid(), &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let text = Local(text.cast());
    // SAFETY: ConvertSidToStringSidW returned a NUL-terminated string.
    let sid = unsafe {
        let start = text.0.cast::<u16>();
        let length = (0..).take_while(|&index| *start.add(index) != 0).count();
        String::from_utf16_lossy(slice::from_raw_parts(start, length))
    };
    Ok(sid)
}

/// Create a named-pipe instance with the same owner-only security used for private files.
pub(crate) fn private_pipe(
    options: &tokio::net::windows::named_pipe::ServerOptions,
    address: &Path,
) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    let descriptor = owner_only(false)?;
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    // SAFETY: valid SECURITY_ATTRIBUTES and descriptor kept alive through the create call.
    unsafe { options.create_with_security_attributes_raw(address, (&raw mut attributes).cast()) }
}

/// Memory from LocalAlloc, freed on drop.
struct Local(*mut c_void);

impl Drop for Local {
    fn drop(&mut self) {
        // SAFETY: the pointer came from LocalAlloc and is freed once.
        unsafe { LocalFree(self.0) };
    }
}

/// The current process's TOKEN_USER, in a buffer aligned for it.
struct User(Vec<u64>);

impl User {
    fn sid(&self) -> PSID {
        // SAFETY: GetTokenInformation filled the buffer with a TOKEN_USER.
        unsafe { (*self.0.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }
}

fn current_user() -> io::Result<User> {
    let mut token = ptr::null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: OpenProcessToken returned a new handle that nothing else owns.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut length = 0;
    // SAFETY: an empty buffer only asks for the required length; this call fails by design.
    unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &mut length,
        )
    };
    // Round the OS-reported byte count up to u64 words so TOKEN_USER/SID
    // storage remains eight-byte aligned without changing the requested byte length.
    let mut buffer = vec![0_u64; (length as usize).div_ceil(std::mem::size_of::<u64>())];
    // SAFETY: the buffer holds at least `length` bytes, aligned for TOKEN_USER and its SID.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(User(buffer))
}

/// A NUL-terminated UTF-16 path; an interior NUL would silently name another path.
fn wide(path: &Path) -> io::Result<Vec<u16>> {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    wide.push(0);
    Ok(wide)
}
