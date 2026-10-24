//! Job Objects: a child starts suspended in a new console process group, joins a
//! kill-on-close job, and its primary thread resumes only when the gate admits it.

use std::{
    io, mem,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    process::ExitStatus,
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};

use tokio::{
    process::{ChildStderr, ChildStdin, ChildStdout, Command},
    sync::oneshot,
    task::JoinHandle,
};
use windows_sys::Win32::{
    Foundation::{
        ERROR_ACCESS_DENIED, ERROR_CANCELLED, ERROR_INVALID_PARAMETER, FILETIME,
        HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation, WAIT_TIMEOUT,
    },
    System::{
        Console::{
            CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent, GetStdHandle, STD_ERROR_HANDLE,
            STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
        },
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
        },
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_BASIC_LIMIT_INFORMATION,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation,
            JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
            TerminateJobObject,
        },
        Threading::{
            CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW,
            CREATE_SUSPENDED, DETACHED_PROCESS, GetProcessTimes, OpenProcess, OpenThread,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, ResumeThread,
            THREAD_SUSPEND_RESUME, WaitForSingleObject,
        },
    },
};

/// Forced job termination uses a nonzero exit code so killed children cannot appear to
/// have completed successfully; keep the existing generic failure code of one.
const FORCED_TERMINATION_EXIT_CODE: u32 = 1;

/// The child and its job; every process in the job is killed when this drops.
pub(crate) struct Child {
    child: tokio::process::Child,
    job: Job,
}

impl Child {
    pub fn stdin(&mut self) -> &mut Option<ChildStdin> {
        &mut self.child.stdin
    }

    pub fn stdout(&mut self) -> &mut Option<ChildStdout> {
        &mut self.child.stdout
    }

    pub fn stderr(&mut self) -> &mut Option<ChildStderr> {
        &mut self.child.stderr
    }

    pub async fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.wait().await
    }

    pub async fn wait_leader(&mut self) -> io::Result<ExitStatus> {
        self.child.wait().await
    }

    pub fn try_wait_leader(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    /// Ask the child's console process group to stop with Ctrl-Break, the closest to SIGTERM.
    /// This fails when the child shares no console with artifactize; the job is killed after
    /// the grace period either way.
    pub fn interrupt(&self) -> io::Result<()> {
        // Group 0 would mean every process on this console, artifactize included.
        let Some(group) = self.child.id().filter(|&pid| pid != 0) else {
            return Ok(());
        };
        // SAFETY: a nonzero group id reaches only the child's own process group.
        if unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, group) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Terminate and await the whole Job, not just its leader. Descendants may
    /// still hold non-delete-shared files after TerminateJobObject returns.
    pub async fn cleanup(&mut self) -> io::Result<ExitStatus> {
        self.kill()?;
        let status = self.child.wait().await?;
        let job = Job(self.job.0.try_clone()?);
        crate::task::joined(tokio::task::spawn_blocking(move || job.wait_empty()).await)
            .map_err(io::Error::other)??;
        Ok(status)
    }

    /// Terminate every process in the job.
    pub fn kill(&mut self) -> io::Result<()> {
        self.job.terminate()
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.job.terminate();
    }
}

/// A job whose processes all die when its last handle closes, artifactize's included.
struct Job(OwnedHandle);

impl Job {
    fn new() -> io::Result<Self> {
        // SAFETY: no attributes and no name create a private, non-inheritable job.
        let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateJobObjectW returned a new handle that nothing else owns.
        let job = Self(unsafe { OwnedHandle::from_raw_handle(handle) });
        let limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
            BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                ..Default::default()
            },
            ..Default::default()
        };
        // SAFETY: the information class matches the structure and its size.
        let ok = unsafe {
            SetInformationJobObject(
                job.0.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                mem::size_of_val(&limits) as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    fn assign(&self, child: &tokio::process::Child) -> io::Result<()> {
        let process = child
            .raw_handle()
            .ok_or_else(|| io::Error::other("the child was reaped before joining its job"))?;
        // SAFETY: both handles are open.
        if unsafe { AssignProcessToJobObject(self.0.as_raw_handle(), process) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn wait_empty(&self) -> io::Result<()> {
        // The kernel's termination is asynchronous; cap even cleanup failures.
        const CLEANUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
        // Jobs expose active-process accounting, not a general empty-Job wait.
        const ACCOUNTING_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);
        let deadline = std::time::Instant::now() + CLEANUP_TIMEOUT;
        loop {
            let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            // SAFETY: the open Job handle and information-class-matching writable
            // buffer remain valid for the exact supplied buffer size.
            if unsafe {
                QueryInformationJobObject(
                    self.0.as_raw_handle(),
                    JobObjectBasicAccountingInformation,
                    (&raw mut info).cast(),
                    mem::size_of_val(&info) as u32,
                    ptr::null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if info.ActiveProcesses == 0 {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Job descendants did not terminate",
                ));
            }
            std::thread::sleep(ACCOUNTING_INTERVAL);
        }
    }

    fn terminate(&self) -> io::Result<()> {
        // SAFETY: the job handle is open.
        if unsafe { TerminateJobObject(self.0.as_raw_handle(), FORCED_TERMINATION_EXIT_CODE) } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

/// Holds a suspended child until admitted. Dropping it unadmitted kills the child before it
/// runs any code.
pub(crate) struct Gate {
    pid: Option<u32>,
    admitted: AtomicBool,
    outcome: Option<oneshot::Sender<bool>>,
}

impl Gate {
    pub async fn pid(&self) -> io::Result<u32> {
        self.pid.ok_or_else(|| io::ErrorKind::UnexpectedEof.into())
    }

    /// Resume the child's primary thread: only now does it start to run.
    pub fn admit(&self) -> io::Result<()> {
        resume(self.pid.ok_or(io::ErrorKind::UnexpectedEof)?)?;
        self.admitted.store(true, Ordering::Release);
        Ok(())
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        if let Some(outcome) = self.outcome.take() {
            let _ = outcome.send(*self.admitted.get_mut());
        }
    }
}

/// Spawn `command` suspended, in a new console process group and a kill-on-close job. The
/// task yields the child once the gate admits it, or kills it if the gate drops first.
pub(crate) fn spawn_gated(
    mut command: Command,
) -> io::Result<(Gate, JoinHandle<io::Result<Child>>)> {
    let job = Job::new()?;
    command.creation_flags(CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP);
    let child = command
        .spawn()
        .and_then(|mut child| match job.assign(&child) {
            Ok(()) => Ok(child),
            Err(error) => {
                // Still suspended, so nothing ran; kill it whatever its kill_on_drop says.
                let _ = child.start_kill();
                Err(error)
            }
        })?;
    let (outcome, admission) = oneshot::channel();
    let gate = Gate {
        pid: child.id(),
        admitted: AtomicBool::new(false),
        outcome: Some(outcome),
    };
    let spawning = tokio::spawn(async move {
        let mut child = child;
        if !matches!(admission.await, Ok(true)) {
            let _ = job.terminate();
            let _ = child.wait().await;
            return Err(io::Error::from_raw_os_error(ERROR_CANCELLED as i32));
        }
        Ok(Child { child, job })
    });
    Ok((gate, spawning))
}

/// Spawn `command` as [`spawn_gated`] does and admit it at once: it joins its job before it
/// runs.
pub(crate) async fn spawn_now(command: Command) -> io::Result<Child> {
    let (gate, spawning) = spawn_gated(command)?;
    gate.admit()?;
    drop(gate);
    spawning.await.map_err(io::Error::other)?
}

/// Resume every thread of a process created suspended; it has exactly one. Its handle is
/// held by the spawning task, so the PID cannot be reused meanwhile.
fn resume(pid: u32) -> io::Result<()> {
    // SAFETY: a snapshot of all threads; the result is checked and then owned.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateToolhelp32Snapshot returned a new handle that nothing else owns.
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
    let mut entry = THREADENTRY32 {
        dwSize: mem::size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut resumed = false;
    // SAFETY: an open snapshot and an entry whose dwSize is set.
    let mut more = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) } != 0;
    while more {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: the result is checked and then owned.
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if thread.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: OpenThread returned a new handle that nothing else owns.
            let thread = unsafe { OwnedHandle::from_raw_handle(thread) };
            // SAFETY: an open thread handle with THREAD_SUSPEND_RESUME.
            if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
                return Err(io::Error::last_os_error());
            }
            resumed = true;
        }
        // SAFETY: as for Thread32First.
        more = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) } != 0;
    }
    if !resumed {
        return Err(io::Error::other(
            "the suspended child has no thread to resume",
        ));
    }
    Ok(())
}

/// Spawn outside artifactize's console, process group and job, for a desktop handoff that
/// outlives it: a terminal, IDE or CI runner may hold artifactize in a kill-on-close job. A
/// job that forbids breakaway refuses that spawn with access denied; the handoff then starts
/// inside the job, as it did before.
pub(crate) fn spawn_detached(mut command: Command) -> io::Result<tokio::process::Child> {
    // A new process inherits every inheritable handle, not only the standard handles it is
    // given. artifactize's own stdout, when a caller reads it through a pipe, would then stay
    // open until the handoff exits and keep that caller waiting. std gives each child its own
    // duplicates, so artifactize's standard handles need not be inheritable.
    for id in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: GetStdHandle only reads the handle table; SetHandleInformation changes only
        // the inherit flag of a handle this process owns, and fails harmlessly for others.
        unsafe {
            let handle = GetStdHandle(id);
            if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
    let detached = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    command.creation_flags(detached | CREATE_BREAKAWAY_FROM_JOB);
    match command.spawn() {
        Err(error) if error.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => {
            command.creation_flags(detached);
            command.spawn()
        }
        spawned => spawned,
    }
}

/// Run workers also detach from their requester when its enclosing Job forbids breakaway;
/// that Job's lifetime remains an OS-imposed bound, just as for a desktop handoff.
pub(crate) fn spawn_run(command: Command) -> io::Result<tokio::process::Child> {
    spawn_detached(command)
}

/// A desktop handoff whose output pipes a Tool log drains. A write to a pipe nobody reads
/// fails with an error on Windows; no signal ends the program.
pub(crate) fn spawn_watched(command: Command) -> io::Result<tokio::process::Child> {
    spawn_detached(command)
}

/// A host probe such as Git runs without a console window. A console program started by a
/// process that has no console, such as the desktop app, would otherwise get a window of its
/// own until it exits. The probe's output goes through pipes, so the window shows nothing.
pub(crate) fn hide_console_window(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(CREATE_NO_WINDOW);
}

/// The creation time, in 100 ns intervals since 1601. A process that has exited counts as
/// not found, as does one this user may not query: artifactize records only its own
/// processes and children, so such a PID was reused by another user's or a system process.
pub(crate) fn process_start_time(pid: u32) -> io::Result<u64> {
    // SAFETY: the result is checked and then owned.
    let process = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    if process.is_null() {
        let error = io::Error::last_os_error();
        return Err(match error.raw_os_error().map(|code| code as u32) {
            Some(ERROR_INVALID_PARAMETER | ERROR_ACCESS_DENIED) => io::ErrorKind::NotFound.into(),
            _ => error,
        });
    }
    // SAFETY: OpenProcess returned a new handle that nothing else owns.
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    // A process handle is signaled on exit, including a real exit code of 259.
    // SAFETY: the owned process handle permits synchronization; zero never blocks.
    match unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } {
        WAIT_TIMEOUT => {}
        windows_sys::Win32::Foundation::WAIT_FAILED => return Err(io::Error::last_os_error()),
        _ => return Err(io::ErrorKind::NotFound.into()),
    }
    let mut creation = FILETIME::default();
    let (mut exit, mut kernel, mut user) = (creation, creation, creation);
    // SAFETY: an open process handle and valid out pointers.
    if unsafe {
        GetProcessTimes(
            process.as_raw_handle(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}

/// Windows identity lookup already excludes a signalled (terminated) process.
pub(crate) fn running_process_start_time(pid: u32) -> io::Result<u64> {
    process_start_time(pid)
}

/// Every owned preparation/execution child has a kill-on-close Job held by its Run worker.
pub(crate) fn kill_saved_session(_leader: crate::process::ChildIdentity) -> io::Result<()> {
    Ok(())
}

/// A dead owner has already closed its kill-on-close Job handle.
pub(crate) fn kill_saved_group(_leader: crate::process::ChildIdentity) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn an_unadmitted_gate_never_runs_the_child_and_kills_it() {
        let directory = crate::test_os::tempdir();
        let (gate, spawning) =
            spawn_gated(crate::test_os::windows_marker(directory.path())).unwrap();
        let pid = gate.pid().await.unwrap();
        assert!(process_start_time(pid).is_ok(), "the held child exists");
        // pid() acknowledges the suspended gate: the child cannot run until admitted.
        assert!(!directory.path().join("marker").exists());
        drop(gate);
        let error = tokio::time::timeout(Duration::from_secs(5), spawning)
            .await
            .unwrap()
            .unwrap()
            .err()
            .expect("the child must not run");
        assert_eq!(error.raw_os_error(), Some(ERROR_CANCELLED as i32));
        assert_eq!(
            process_start_time(pid).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert!(!directory.path().join("marker").exists());

        // Admitted, the same child runs: the marker would have shown an early start.
        let (gate, spawning) =
            spawn_gated(crate::test_os::windows_marker(directory.path())).unwrap();
        gate.admit().unwrap();
        drop(gate);
        let mut child = spawning.await.unwrap().unwrap();
        assert!(child.wait().await.unwrap().success());
        assert!(directory.path().join("marker").exists());
    }

    /// The marker fixture ([`crate::test_os::windows_marker`]): creates `marker` in its working
    /// directory once it runs.
    #[test]
    fn marker_probe() {
        if std::env::var_os("ARTIFACTIZE_MARKER_PROBE").is_none() {
            return;
        }
        std::fs::File::create("marker").unwrap();
    }

    /// A job that forbids breakaway refuses CREATE_BREAKAWAY_FROM_JOB; the handoff then starts
    /// inside it. A process cannot leave a job, so the probe runs in a test process of its own.
    #[test]
    fn a_detached_launch_inside_a_job_without_breakaway_still_starts() {
        let output = crate::test_os::run_test(
            "platform::windows::process::tests::detached_launch_probe",
            &[("ARTIFACTIZE_DETACHED_PROBE", "1".into())],
        );
        assert!(output.status.success(), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "{output:?}"
        );
    }

    #[tokio::test]
    async fn detached_launch_probe() {
        if std::env::var_os("ARTIFACTIZE_DETACHED_PROBE").is_none() {
            return;
        }
        let job = crate::test_os::windows_job_without_breakaway();
        let command = crate::test_os::stdin_waiter();
        let mut child = spawn_detached(command).unwrap();
        assert!(
            crate::test_os::process_in_job(&child, &job),
            "the fallback starts the handoff inside the job"
        );
        child.kill().await.unwrap();

        // A Run worker uses the same restricted Job, but with owner-only cwd and stderr
        // beneath a parent whose Everyone ACE would otherwise be inherited.
        let parent = crate::test_os::tempdir();
        crate::test_os::share_dir(parent.path());
        let directory = crate::platform::private_tempdir_in("run-", parent.path()).unwrap();
        let stderr_path = directory.path().join("worker.stderr");
        let stderr = std::fs::File::create(&stderr_path).unwrap();
        let mut command = crate::test_os::windows_marker(directory.path());
        command.stderr(std::process::Stdio::from(stderr));
        let mut worker = spawn_run(command).unwrap();
        assert!(worker.wait().await.unwrap().success());
        assert!(directory.path().join("marker").exists());
        assert!(crate::test_os::private_dir(directory.path()));
        assert!(!crate::test_os::has_inherited_aces(directory.path()));
        assert!(crate::test_os::private_file(&stderr_path));
        assert!(crate::test_os::private_file(
            &directory.path().join("marker")
        ));

        // Closing the job's last handle would end this process too; it closes at exit.
        std::mem::forget(job);
    }
}

#[cfg(test)]
mod console_tests {
    use windows_sys::Win32::{
        System::Console::GetConsoleWindow, UI::WindowsAndMessaging::IsWindowVisible,
    };

    const WINDOW_PROBE: &str = "platform::windows::process::console_tests::console_window_probe";
    const HIDDEN: &str = "console window: hidden";

    /// The desktop app has no console, so the probe runs from a detached process: the case in
    /// which Windows gives a console program a window of its own.
    #[test]
    fn a_probe_from_a_process_without_a_console_shows_no_window() {
        let mut parent = crate::test_os::test_command(
            "platform::windows::process::console_tests::detached_parent_probe",
            &[],
        );
        let output = crate::test_os::detached_launch(parent.arg("--ignored"))
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(HIDDEN),
            "{output:?}"
        );
    }

    #[test]
    #[ignore = "launched detached by the console window test"]
    fn detached_parent_probe() {
        // SAFETY: GetConsoleWindow only reads this process's console attachment.
        assert!(
            unsafe { GetConsoleWindow() }.is_null(),
            "the parent has no console"
        );
        let exe = std::env::current_exe().unwrap();
        let stdout = crate::platform::program::tool_output(
            exe.to_str().unwrap(),
            ["--exact", WINDOW_PROBE, "--ignored", "--nocapture"].map(std::ffi::OsStr::new),
        )
        .expect("the window probe runs");
        print!("{}", String::from_utf8_lossy(&stdout));
    }

    #[test]
    #[ignore = "launched as a host probe by the detached parent"]
    fn console_window_probe() {
        // SAFETY: both calls only read; IsWindowVisible accepts a null window.
        let visible = unsafe { IsWindowVisible(GetConsoleWindow()) } != 0;
        println!(
            "console window: {}",
            if visible { "visible" } else { "hidden" }
        );
    }
}

#[cfg(test)]
mod exit_tests {
    use super::*;

    /// The exit fixture ([`crate::test_os::windows_exit`]): exits with the code it is given.
    #[test]
    fn exit_probe() {
        if let Some(code) = std::env::var_os("ARTIFACTIZE_EXIT_PROBE") {
            std::process::exit(code.to_str().unwrap().parse().unwrap());
        }
    }

    #[tokio::test]
    async fn exit_code_259_does_not_mean_the_process_is_alive() {
        let mut child = crate::test_os::windows_exit(259).spawn().unwrap();
        let pid = child.id().unwrap();
        assert_eq!(child.wait().await.unwrap().code(), Some(259));
        assert!(
            !crate::test_os::running(pid),
            "the fixture sees a terminated process"
        );
        assert_eq!(
            process_start_time(pid).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}
