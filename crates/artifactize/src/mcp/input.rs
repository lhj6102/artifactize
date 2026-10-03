use std::{
    fs::File,
    io::{self, Read},
    os::fd::{AsRawFd, FromRawFd},
    pin::Pin,
    task::{Context, Poll, ready},
};

use tokio::io::{AsyncRead, ReadBuf, unix::AsyncFd};

/// Nonblocking stdin avoids a blocking reader keeping the process alive after cancellation.
pub(super) struct Input(AsyncFd<File>);

impl Input {
    pub fn new() -> io::Result<Self> {
        let fd = unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_DUPFD_CLOEXEC, 3) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let file = unsafe { File::from_raw_fd(fd) };
        let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(AsyncFd::new(file)?))
    }
}

impl AsyncRead for Input {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            let mut ready = ready!(self.0.poll_read_ready(cx))?;
            match ready.try_io(|inner| inner.get_ref().read(buffer.initialize_unfilled())) {
                Ok(Ok(length)) => {
                    buffer.advance(length);
                    return Poll::Ready(Ok(()));
                }
                Ok(Err(error)) => return Poll::Ready(Err(error)),
                Err(_) => continue,
            }
        }
    }
}
