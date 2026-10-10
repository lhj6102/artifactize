# artifactize platform checklist

What code in `crates/artifactize` and `crates/artifactize-tools` must follow so that it behaves
the same on Linux, macOS and Windows. Development happens on Linux; macOS and Windows run the
full test suite on every push to `main`, so a change that only works on Linux shows up there.

## Where operating-system code lives

- **P-PLATFORM-MODULE**: operating-system calls (`std::os::unix`, `std::os::windows`, `libc`,
  `windows-sys`, `/proc`) and `cfg(unix)`, `cfg(windows)` or `cfg(target_os = …)` branches live
  only in `crates/artifactize/src/platform/` and `crates/artifactize-tools/src/files/`. Other code
  calls their portable interface. A branch elsewhere is a missing platform function.
- **P-ONE-LOOKUP**: a declared program is found only through `artifactize_tools::program`
  (`PATH`, and `PATHEXT` on Windows) and started only through the process module. No code
  searches `PATH` itself or appends `.exe`.
- **P-ONE-OPENER**: a file or URL is opened in a desktop application only through
  `artifactize_tools::opener`. No other code names `xdg-open`, `open`, `explorer`, `start` or
  `wslview`.
- **P-NO-SHELL**: commands run as an argument vector, never as a `sh -c` or `cmd /c` string. The
  one exception is the `$EDITOR` launch in the platform module, because `EDITOR` may hold
  arguments.

## Paths

- **P-LOGICAL-PATHS**: a path shown to a person or a model, stored in a record, or compared as
  text is a logical path: relative to the Artifact or repository, with `/` separators. Native
  paths stay inside file and process calls.
- **P-NO-FIXED-ROOTS**: no hard-coded `/tmp`, `/proc`, `/dev/null`, `/bin/…`, `HOME` or drive
  letters. Use `std::env::temp_dir`, the state home and platform functions.
- **P-SCOPE-THROUGH-HANDLES**: scope decisions never compare native paths as strings. They go
  through the pinned, no-follow opens in `files` and `scope`, which handle case-insensitive
  volumes, Windows 8.3 names and the macOS `/tmp`, `/var` and `/etc` aliases.
- **P-TRUSTED-ROOTS**: a root the operator gives (state home, repository, run output) is
  resolved once, where it enters. Everything below it is opened without following links.

## Processes

- **P-PROCESS-TREE**: a declared command is spawned through the process module, so that it is
  admitted before it runs, kept in one process group or Job Object, and cleaned up with its
  children. No direct `Command::spawn` for declared commands.
- **P-EXPLICIT-ENV**: a child's environment is built from an explicit list after clearing the
  inherited one. Windows variable names are compared without regard to case.
- **P-EXIT-STATUS**: exit handling covers an exit code and, on Unix, a terminating signal.
  Windows reports exit codes only.

## Tests

- **P-TEST-HELPERS**: tests use `tests/support/os.rs` (integration) and `crate::test_os` (unit)
  for program paths, temporary roots, links, permissions and process control. They do not
  hard-code `/bin/sh`, `/bin/true`, `/proc`, shell-script fixtures or uncanonicalized temporary
  paths.
- **P-TEST-EVERY-OS**: a test runs on every operating system unless the behavior exists on only
  one. A `cfg` gate on a test carries a comment saying why, and a gated security test (scope
  escape, private files, process cleanup) has a counterpart on the other systems.
- **P-TEST-NO-TIMING**: tests do not depend on clock resolution, sleep lengths or names made
  from the time. They use `TempDir` for unique directories and explicit synchronization for
  ordering.
