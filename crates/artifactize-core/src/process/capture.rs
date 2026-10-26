//! Draining a child's output pipes into its bounded capture, its evidence files and the lines a
//! Tool log shows. How each pipe ended and whether its evidence was saved are kept apart, so a
//! slow disk is never taken for a process that still holds a pipe.

use std::{io, path::PathBuf, time::Duration};

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    time::{Instant, timeout},
};
use tokio_util::sync::CancellationToken;

use crate::{
    platform,
    tools::human::log::{Lines, Stream},
};

/// Drain process pipes in small bounded chunks without one read per byte.
const CAPTURE_BUFFER_BYTES: usize = 8192;

/// How long a pipe may stay open once the process group is gone. Only a process outside the
/// group can hold it then, such as a descendant that left the group. The time counts only
/// while the pipe is waited for, never while its evidence is written, and it is generous
/// because waiting costs nothing unless such a process exists, while a late wakeup on a busy
/// runtime must not pass for one.
pub(crate) const OUTPUT_CLOSE_GRACE: Duration = Duration::from_secs(5);

/// How reading one pipe ended.
#[derive(Debug)]
pub(super) enum PipeEnd {
    /// Every writer closed it.
    Closed,
    /// Still open [`OUTPUT_CLOSE_GRACE`] after the process group was gone.
    Open,
    Failed(io::Error),
}

/// One stream's capture: what was read of it, how its pipe ended and whether its evidence was
/// saved.
#[derive(Debug)]
pub(super) struct Captured {
    pub output: Vec<u8>,
    pub truncated: bool,
    pub pipe: PipeEnd,
    pub evidence: io::Result<()>,
}

/// Read `pipe` to its end, keeping its first `limit` bytes and writing all of it to `path` and
/// `lines`. Once `gone` is cancelled, waiting for the pipe is limited to
/// [`OUTPUT_CLOSE_GRACE`] in all.
pub(super) async fn capture(
    mut pipe: impl AsyncRead + Unpin,
    limit: usize,
    path: Option<PathBuf>,
    mut lines: Option<Lines>,
    gone: &CancellationToken,
) -> Captured {
    let mut evidence = Evidence::open(path);
    let mut output = Vec::new();
    let mut buffer = [0; CAPTURE_BUFFER_BYTES];
    let mut truncated = false;
    let mut grace = OUTPUT_CLOSE_GRACE;
    let pipe_end = loop {
        let count = match read(&mut pipe, &mut buffer, gone, &mut grace).await {
            Some(Ok(0)) => break PipeEnd::Closed,
            Some(Ok(count)) => count,
            Some(Err(error)) => break PipeEnd::Failed(error),
            None => break PipeEnd::Open,
        };
        let chunk = &buffer[..count];
        evidence.write(chunk).await;
        if let Some(lines) = &mut lines {
            lines.push(chunk);
        }
        let retained = count.min(limit - output.len());
        output.extend_from_slice(&chunk[..retained]);
        truncated |= retained < count;
    };
    if let Some(lines) = &mut lines {
        lines.finish();
    }
    let evidence = evidence.finish(matches!(pipe_end, PipeEnd::Closed)).await;
    Captured {
        output,
        truncated,
        pipe: pipe_end,
        evidence,
    }
}

/// One read of `pipe`: unbounded while the process group runs, then within what is left of
/// `grace`, which the time spent waiting here uses up. `None` when it ran out.
async fn read(
    pipe: &mut (impl AsyncRead + Unpin),
    buffer: &mut [u8],
    gone: &CancellationToken,
    grace: &mut Duration,
) -> Option<io::Result<usize>> {
    if !gone.is_cancelled() {
        // A pipe read is cancel safe: a read dropped here has taken no bytes.
        tokio::select! {
            biased;
            read = pipe.read(buffer) => return Some(read),
            () = gone.cancelled() => {}
        }
    }
    let started = Instant::now();
    let read = timeout(*grace, pipe.read(buffer)).await;
    *grace = grace.saturating_sub(started.elapsed());
    read.ok()
}

/// A stream's evidence file. A failed write ends the evidence, not the draining, so the pipe is
/// still read to its end and the failure is reported once it is.
struct Evidence {
    file: Option<(PathBuf, tokio::fs::File)>,
    failure: Option<io::Error>,
}

impl Evidence {
    fn open(path: Option<PathBuf>) -> Self {
        let Some(path) = path else {
            return Self {
                file: None,
                failure: None,
            };
        };
        let mut options = platform::private_options();
        options.write(true).create_new(true);
        match options.open(&path) {
            Ok(file) => Self {
                file: Some((path, tokio::fs::File::from_std(file))),
                failure: None,
            },
            Err(error) => Self {
                file: None,
                failure: Some(error),
            },
        }
    }

    async fn write(&mut self, chunk: &[u8]) {
        let Some((_, file)) = &mut self.file else {
            return;
        };
        #[cfg(test)]
        slow_disk::wait().await;
        // Flushed at once, so captured evidence is available while the process still runs.
        let written = async {
            file.write_all(chunk).await?;
            file.flush().await
        };
        if let Err(error) = written.await {
            self.file = None;
            self.failure = Some(error);
        }
    }

    /// Flush the file and, when the pipe was `closed`, mark the stream complete beside it.
    async fn finish(self, closed: bool) -> io::Result<()> {
        if let Some(failure) = self.failure {
            return Err(failure);
        }
        let Some((path, mut file)) = self.file else {
            return Ok(());
        };
        #[cfg(test)]
        slow_disk::wait().await;
        file.flush().await?;
        if closed {
            let mut options = platform::private_options();
            options.write(true).create_new(true);
            let extension = path.extension().expect("stream extension");
            options
                .open(path.with_extension(format!("{}.complete", extension.to_string_lossy())))?;
        }
        Ok(())
    }
}

/// What failed in capturing one stream.
#[derive(Debug)]
pub enum StreamFailure {
    /// Its pipe was still open [`OUTPUT_CLOSE_GRACE`] after the process group was gone.
    Open,
    /// Its pipe could not be read.
    Read(io::Error),
    /// Its evidence file could not be written.
    Evidence(io::Error),
}

/// A capture failure with all the run had come to: how the process ended and the output read.
#[derive(Debug, thiserror::Error)]
#[error("process output could not be captured: {}; {}", failure_text(&self.failures), end_text(&self.output.status))]
pub struct CaptureError {
    pub output: super::Output,
    /// Each failure with its stream, never empty.
    pub failures: Vec<(Stream, StreamFailure)>,
}

fn failure_text(failures: &[(Stream, StreamFailure)]) -> String {
    failures
        .iter()
        .map(|(stream, failure)| {
            let stream = match stream {
                Stream::Stdout => "stdout",
                Stream::Stderr => "stderr",
            };
            match failure {
                StreamFailure::Open => format!(
                    "{stream} was still open {}s after the process group was gone, so a process \
                     outside the group holds it",
                    OUTPUT_CLOSE_GRACE.as_secs()
                ),
                StreamFailure::Read(error) => format!("{stream} could not be read: {error}"),
                StreamFailure::Evidence(error) => {
                    format!("{stream} could not be saved: {error}")
                }
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn end_text(status: &std::process::ExitStatus) -> String {
    format!(
        "the process had ended ({})",
        platform::exit_description(status)
    )
}

impl Captured {
    /// The output read, and what failed.
    fn split(self) -> ((Vec<u8>, bool), impl Iterator<Item = StreamFailure>) {
        let pipe = match self.pipe {
            PipeEnd::Closed => None,
            PipeEnd::Open => Some(StreamFailure::Open),
            PipeEnd::Failed(error) => Some(StreamFailure::Read(error)),
        };
        let evidence = self.evidence.err().map(StreamFailure::Evidence);
        (
            (self.output, self.truncated),
            pipe.into_iter().chain(evidence),
        )
    }
}

/// Combine how the process ended with its two streams' captures.
pub(super) fn finish(
    status: std::process::ExitStatus,
    stdout: Captured,
    stderr: Captured,
    duration: Duration,
) -> Result<super::Output, super::Error> {
    let ((stdout, stdout_truncated), stdout_failures) = stdout.split();
    let ((stderr, stderr_truncated), stderr_failures) = stderr.split();
    let failures: Vec<_> = stdout_failures
        .map(|failure| (Stream::Stdout, failure))
        .chain(stderr_failures.map(|failure| (Stream::Stderr, failure)))
        .collect();
    let output = super::Output {
        status,
        stdout,
        stderr,
        truncated: stdout_truncated || stderr_truncated,
        duration,
    };
    if failures.is_empty() {
        Ok(output)
    } else {
        Err(super::Error::Capture(Box::new(CaptureError {
            output,
            failures,
        })))
    }
}

/// Tests stand in for a slow disk by holding every evidence write on their thread until
/// released.
#[cfg(test)]
pub(crate) mod slow_disk {
    use std::{cell::RefCell, sync::Arc};

    use tokio::sync::watch;

    /// The release switch, and how many evidence writes wait for it.
    type Hold = (watch::Receiver<bool>, Arc<()>);

    thread_local! {
        static HOLD: RefCell<Option<Hold>> = const { RefCell::new(None) };
    }

    /// A disk that holds every evidence write on this thread until it is released.
    pub(crate) struct SlowDisk {
        release: watch::Sender<bool>,
        waiting: Arc<()>,
    }

    impl SlowDisk {
        /// How many evidence writes wait for the release now.
        pub(crate) fn waiting(&self) -> usize {
            // Each waiting write holds one clone; the disk and the thread's hold hold two.
            Arc::strong_count(&self.waiting) - 2
        }

        pub(crate) fn release(&self) {
            self.release.send_replace(true);
        }
    }

    pub(crate) fn hold() -> SlowDisk {
        let (release, held) = watch::channel(false);
        let waiting = Arc::new(());
        HOLD.with(|hold| *hold.borrow_mut() = Some((held, waiting.clone())));
        SlowDisk { release, waiting }
    }

    pub(super) async fn wait() {
        let held = HOLD.with(|hold| hold.borrow().clone());
        if let Some((mut held, waiting)) = held {
            let _ = held.wait_for(|released| *released).await;
            drop(waiting);
        }
    }
}
