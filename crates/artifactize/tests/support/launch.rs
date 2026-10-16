//! Own the detached Human launcher fixture and its inputs through PID handoff and exit.
use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use crate::support::os;

/// Keep joined inputs for a post-cleanup observation, without exposing the temporary owner.
pub struct RetainedInputs {
    _directory: Rc<RefCell<Option<tempfile::TempDir>>>,
}

struct Handoff {
    pid: u32,
    identities: Vec<(u32, u64)>,
}

/// Install before launching: a pending handoff still belongs to this fixture on unwind.
pub struct Launched {
    root: PathBuf,
    inputs: Rc<RefCell<Option<tempfile::TempDir>>>,
    processes: Option<super::Processes>,
    handoff: Option<Handoff>,
}
impl Launched {
    pub fn new(root: tempfile::TempDir) -> Self {
        Self {
            root: root.path().to_owned(),
            inputs: Rc::new(RefCell::new(Some(root))),
            processes: Some(super::Processes::default()),
            handoff: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    pub fn retain_inputs(&self) -> RetainedInputs {
        RetainedInputs {
            _directory: self.inputs.clone(),
        }
    }

    pub fn own(&mut self) -> Option<u32> {
        if let Some(handoff) = &self.handoff {
            return Some(handoff.pid);
        }
        let pid = fs::read_to_string(self.path().join("pid"))
            .ok()?
            .parse()
            .ok()?;
        // Native launchers may remain alive alongside Python. Their complete marker is
        // published before `pid`; an absent optional marker denotes a single-process probe.
        let pids = match fs::read(self.path().join("pids")) {
            Ok(bytes) => serde_json::from_slice::<Vec<u32>>(&bytes).ok()?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(_) => return None,
        };
        let pids = pids
            .into_iter()
            .chain([pid])
            .collect::<std::collections::BTreeSet<_>>();
        // Own every surviving sibling even if another wrapper has already exited before
        // its birth identity can be read. A failed handoff must still clean up the rest.
        for &pid in &pids {
            self.processes.as_mut().unwrap().own(pid);
        }
        let identities = pids
            .into_iter()
            .map(|pid| os::observed_start_time(pid).map(|start| (pid, start)))
            .collect::<Option<Vec<_>>>()?;
        self.handoff = Some(Handoff { pid, identities });
        Some(pid)
    }

    /// Release explicitly so an enclosing fixture can retain its own inputs on failure too.
    pub fn close(mut self) -> Result<(), String> {
        self.release()
    }

    fn release(&mut self) -> Result<(), String> {
        if self.processes.is_none() {
            return Ok(());
        }
        // A successful launch can still be starting its interpreter when an assertion
        // unwinds. Keep the root and release in place until that pending handoff arrives.
        // The first signal also opens the deliberately held handoff in the cleanup probe.
        let _ = fs::write(self.path().join("release-handoff"), "");
        const HANDOFF_BOUND: Duration = Duration::from_secs(30);
        const HANDOFF_POLL: Duration = Duration::from_millis(10);
        let deadline = std::time::Instant::now() + os::patience(HANDOFF_BOUND);
        let adopted = loop {
            if self.own().is_some() {
                break true;
            }
            if std::time::Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(HANDOFF_POLL);
        };
        let released = fs::write(self.path().join("release"), "");
        // Join before disposing inputs, even when the original test assertion is unwinding.
        let joined = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            drop(self.processes.take());
        }));
        let exited = self.handoff.as_ref().is_some_and(|handoff| {
            handoff
                .identities
                .iter()
                .all(|&(pid, start)| !os::same_process_running(pid, start))
        });
        if !adopted || released.is_err() || joined.is_err() || !exited {
            let retained = self.inputs.borrow_mut().take().unwrap().keep();
            return Err(format!(
                "Detached launch cleanup failed; inputs retained at {}",
                os::path_text(&retained)
            ));
        }
        Ok(())
    }
}
impl Drop for Launched {
    fn drop(&mut self) {
        if let Err(message) = self.release() {
            if std::thread::panicking() {
                eprintln!("{message}");
            } else {
                panic!("{message}");
            }
        }
    }
}

/// Publish complete markers atomically, then wait for the fixture's explicit release.
/// The first argument may name a separate owned input directory; otherwise use the tool cwd.
/// A native launcher probe can declare OWNED_PIDS before this source to include its wrapper.
pub const LAUNCH: &str = r"import json, os, pathlib, sys, time
root = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else pathlib.Path.cwd()
if (root / 'hold-handoff').exists():
    (root / 'handoff-ready').write_text('')
    while not (root / 'release-handoff').exists(): time.sleep(0.01)
for name, contents in [('pids', json.dumps(globals().get('OWNED_PIDS', [os.getpid()]))), ('pid', str(os.getpid())), ('marker', os.environ.get('ARTIFACTIZE_HUMAN_MARKER', 'launched'))]:
    pending = root / (name + '.pending')
    pending.write_text(contents)
    pending.replace(root / name)
while not (root / 'release').exists(): time.sleep(0.01)
";
