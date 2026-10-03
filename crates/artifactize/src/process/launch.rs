use std::{
    io::{self, Read, Write},
    net::Shutdown,
    os::{fd::AsRawFd, unix::net::UnixStream},
    sync::Arc,
};

use process_wrap::tokio::CommandWrap;
use tokio::{io::unix::AsyncFd, task::JoinHandle};

use super::Child;

pub(super) struct Gate(Arc<AsyncFd<UnixStream>>);

impl Gate {
    pub async fn pid(&self) -> io::Result<u32> {
        let mut bytes = [0; 4];
        let mut offset = 0;
        while offset < bytes.len() {
            let mut ready = self.0.readable().await?;
            match ready.try_io(|socket| socket.get_ref().read(&mut bytes[offset..])) {
                Ok(Ok(0)) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(Ok(count)) => offset += count,
                Ok(Err(error)) => return Err(error),
                Err(_) => continue,
            }
        }
        Ok(u32::from_ne_bytes(bytes))
    }

    pub fn admit(&self) -> io::Result<()> {
        self.0.get_ref().write_all(&[1])
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        // shutdown also disconnects the copy held alive by the spawning thread.
        let _ = self.0.get_ref().shutdown(Shutdown::Both);
    }
}

pub(super) fn spawn(mut command: CommandWrap) -> io::Result<(Gate, JoinHandle<io::Result<Child>>)> {
    let (parent, child) = UnixStream::pair()?;
    parent.set_nonblocking(true)?;
    let gate = Gate(Arc::new(AsyncFd::new(parent)?));
    let parent = gate.0.clone();
    // SAFETY: the forked child uses only async-signal-safe syscalls and stack data.
    // Both descriptors stay alive until spawn completes; CLOEXEC closes the child
    // endpoint on exec. Closing the inherited parent endpoint makes EOF meaningful.
    unsafe {
        command.command_mut().pre_exec(move || {
            libc::close(parent.get_ref().as_raw_fd());
            let pid = (libc::getpid() as u32).to_ne_bytes();
            let fd = child.as_raw_fd();
            let mut offset = 0;
            while offset < pid.len() {
                let written = libc::write(fd, pid[offset..].as_ptr().cast(), pid.len() - offset);
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
            let mut admission = 0_u8;
            loop {
                match libc::read(fd, (&mut admission as *mut u8).cast(), 1) {
                    1 if admission == 1 => return Ok(()),
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
    // std spawn waits for exec's error pipe, so it cannot run on the task that
    // opens the gate. Returning a guarded child also covers a dropped join handle.
    let spawning = tokio::task::spawn_blocking(move || command.spawn().map(Child));
    Ok((gate, spawning))
}

#[cfg(test)]
mod tests {
    use std::{process::Stdio, time::Duration};

    use process_wrap::tokio::ProcessGroup;

    use super::*;

    #[tokio::test]
    async fn parent_disconnect_before_admission_prevents_exec_and_reaps() {
        let mut command = CommandWrap::with_new("/bin/true", |command| {
            command.env_clear().stdin(Stdio::null());
        });
        command.wrap(ProcessGroup::leader());
        let (gate, spawning) = spawn(command).unwrap();
        let pid = gate.pid().await.unwrap();
        assert!(std::fs::exists(format!("/proc/{pid}")).unwrap());
        drop(gate);
        let error = tokio::time::timeout(Duration::from_secs(2), spawning)
            .await
            .unwrap()
            .unwrap()
            .err()
            .expect("the child must not exec");
        assert_eq!(error.raw_os_error(), Some(libc::ECANCELED));
        assert!(!std::fs::exists(format!("/proc/{pid}")).unwrap());
    }
}
