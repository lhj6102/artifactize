//! Own the detached Human launcher fixture and its inputs through PID handoff and exit.
use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::support::os;

/// Keep joined inputs for a post-cleanup observation, without exposing the temporary owner.
pub struct RetainedInputs {
    _directory: Rc<RefCell<Option<tempfile::TempDir>>>,
}

struct Handoff {
    pid: u32,
}

/// Install before launching: a pending handoff still belongs to this fixture on unwind.
pub struct Launched {
    root: PathBuf,
    inputs: Rc<RefCell<Option<tempfile::TempDir>>>,
    processes: Option<super::Processes>,
    handoff: Option<Handoff>,
    launch: Vec<(u32, u64)>,
    native: Arc<Mutex<Option<artifactize_core::process::ChildIdentity>>>,
}
impl Launched {
    pub fn new(root: tempfile::TempDir) -> Self {
        Self {
            root: root.path().to_owned(),
            inputs: Rc::new(RefCell::new(Some(root))),
            processes: Some(super::Processes::default()),
            handoff: None,
            launch: Vec::new(),
            native: Arc::default(),
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

    pub fn observe(
        &self,
        run: artifactize_core::review::log::Run,
    ) -> artifactize_core::review::log::Run {
        let address = self.native.clone();
        run.observe_launch(move |identity| *address.lock().unwrap() = Some(identity))
    }

    pub fn native_identity(&self) -> Option<artifactize_core::process::ChildIdentity> {
        *self.native.lock().unwrap()
    }

    /// Adopt the launcher and its target before the application-level PID handoff.
    fn adopt_launch(&mut self) -> bool {
        if let Some(identity) = self.native_identity() {
            let birth = (identity.pid.get(), identity.start_time);
            if !self.launch.contains(&birth) {
                self.processes
                    .as_mut()
                    .unwrap()
                    .own_identity(birth.0, birth.1);
                self.launch.push(birth);
            }
        }
        let Ok(bytes) = fs::read(self.path().join("launch-identities")) else {
            return !self.launch.is_empty();
        };
        let Ok(pids) = serde_json::from_slice::<Vec<u32>>(&bytes) else {
            return !self.launch.is_empty();
        };
        for pid in pids {
            if let Some(start) = os::observed_start_time(pid)
                && !self.launch.contains(&(pid, start))
            {
                self.processes.as_mut().unwrap().own(pid);
                self.launch.push((pid, start));
            }
        }
        !self.launch.is_empty()
    }

    pub fn own(&mut self) -> Option<u32> {
        if let Some(handoff) = &self.handoff {
            return Some(handoff.pid);
        }
        self.adopt_launch();
        let pid = fs::read_to_string(self.path().join("pid"))
            .ok()?
            .parse()
            .ok()?;
        self.handoff = Some(Handoff { pid });
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
        loop {
            // Normal cleanup also lets the probe publish its observations. If the
            // handoff never arrives, the earlier launch identities still own the children.
            if self.own().is_some()
                || self.adopt_launch() && self.path().join("withhold-handoff").exists()
            {
                break;
            }
            if std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(HANDOFF_POLL);
        }
        let released = fs::write(self.path().join("release"), "");
        // Join before disposing inputs, even when the original test assertion is unwinding.
        let joined = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.processes
                .take()
                .unwrap()
                .release()
                .expect("join detached launch");
        }));
        let exited = !self.launch.is_empty()
            && self
                .launch
                .iter()
                .all(|&(pid, start)| !os::same_process_running(pid, start));
        if released.is_err() || joined.is_err() || !exited {
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
def write_marker(name, contents):
    pending = root / (name + '.pending')
    pending.write_text(contents)
    pending.replace(root / name)
if (root / 'withhold-handoff').exists():
    write_marker('handoff-ready', '')
    while not (root / 'release').exists(): time.sleep(0.01)
    raise SystemExit(0)
write_marker('launch-identities', json.dumps(globals().get('OWNED_PIDS', [os.getpid()])))
if (root / 'hold-handoff').exists():
    (root / 'handoff-ready').write_text('')
    while not (root / 'release-handoff').exists(): time.sleep(0.01)
for name, contents in [('pid', str(os.getpid())), ('marker', globals().get('MARKER') or os.environ.get('ARTIFACTIZE_HUMAN_MARKER', 'launched'))]:
    write_marker(name, contents)
while not (root / 'release').exists(): time.sleep(0.01)
";
