//! Native process checkpoints used by the platform contract tests.

/// Hold a child immediately before its identity is published. Closing this owner on
/// assertion unwind releases pre_exec with cancellation, so spawning still reaps it.
#[cfg(unix)]
pub struct BeforeIdentity(tokio::net::UnixStream);

#[cfg(unix)]
impl BeforeIdentity {
    pub fn hold(command: &mut tokio::process::Command) -> Self {
        use std::{io, os::fd::AsRawFd, os::unix::net::UnixStream};

        let (parent, child) = UnixStream::pair().unwrap();
        parent.set_nonblocking(true).unwrap();
        let parent_fd = parent.as_raw_fd();
        // SAFETY: the forked child uses only async-signal-safe syscalls and stack
        // data. Both endpoints stay alive until spawn; closing the inherited parent
        // endpoint ensures dropping the fixture parent releases the child with EOF.
        unsafe {
            command.pre_exec(move || {
                libc::close(parent_fd);
                let fd = child.as_raw_fd();
                let pid = (libc::getpid() as u32).to_ne_bytes();
                let mut offset = 0;
                while offset < pid.len() {
                    let written =
                        libc::write(fd, pid[offset..].as_ptr().cast(), pid.len() - offset);
                    if written < 0 {
                        let error = io::Error::last_os_error();
                        if error.raw_os_error() == Some(libc::EINTR) {
                            continue;
                        }
                        return Err(error);
                    }
                    if written == 0 {
                        return Err(io::Error::from_raw_os_error(libc::EPIPE));
                    }
                    offset += written as usize;
                }
                let mut release = 0_u8;
                loop {
                    match libc::read(fd, (&mut release as *mut u8).cast(), 1) {
                        1 => return Ok(()),
                        -1 => {
                            let error = io::Error::last_os_error();
                            if error.raw_os_error() != Some(libc::EINTR) {
                                return Err(error);
                            }
                        }
                        _ => return Err(io::Error::from_raw_os_error(libc::ECANCELED)),
                    }
                }
            });
        }
        Self(tokio::net::UnixStream::from_std(parent).unwrap())
    }

    pub async fn pid(&mut self) -> u32 {
        use tokio::io::AsyncReadExt;
        let mut pid = [0; size_of::<u32>()];
        self.0.read_exact(&mut pid).await.unwrap();
        u32::from_ne_bytes(pid)
    }

    pub async fn release(&mut self) {
        use tokio::io::AsyncWriteExt;
        self.0.write_all(&[1]).await.unwrap();
    }
}

/// Read-only membership observation for the exact live child and Job handles.
#[cfg(windows)]
pub fn process_in_job(
    child: &tokio::process::Child,
    job: &std::os::windows::io::OwnedHandle,
) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::JobObjects::IsProcessInJob;

    let mut in_job = 0;
    // SAFETY: both borrowed handles stay open for the query and the out pointer is valid.
    let queried = unsafe {
        IsProcessInJob(
            child.raw_handle().unwrap(),
            job.as_raw_handle(),
            &mut in_job,
        )
    };
    assert_ne!(queried, 0, "query fixture child Job membership");
    in_job != 0
}
