# Platform checklist

Rules that keep code behaving the same on Linux, macOS and Windows.

## Operating-system code

- **P-PLATFORM-MODULE**: operating-system calls (`std::os::unix`, `std::os::windows`, `libc`,
  `windows-sys`, `/proc`) and `cfg(unix)`, `cfg(windows)` or `cfg(target_os = …)` branches live
  only in the crate's platform layer. Other code calls that layer's portable interface. A branch
  elsewhere is a missing platform function.
- **P-ONE-LOOKUP**: a declared program is found by one shared lookup (`PATH`, and `PATHEXT` on
  Windows) and started by one process layer. No other code searches `PATH` or appends `.exe`.
- **P-ONE-OPENER**: a file or URL is opened in a desktop application by one shared opener. No
  other code names `xdg-open`, `open`, `explorer`, `start` or `wslview`.
- **P-NO-SHELL**: commands run as an argument vector, never as a `sh -c` or `cmd /c` string. The
  one exception is launching the user's editor, because `EDITOR` may hold arguments.

## Paths

- **P-LOGICAL-PATHS**: a path shown to a person or a model, stored in a record, or compared as
  text is a logical path: relative to its root, with `/` separators. Native paths stay inside
  file and process calls.
- **P-NO-FIXED-ROOTS**: no hard-coded `/tmp`, `/proc`, `/dev/null`, `/bin/…`, `HOME` or drive
  letters. Use `std::env::temp_dir`, configured directories and platform functions.
- **P-SCOPE-THROUGH-HANDLES**: access decisions never compare native paths as strings. They go
  through pinned, no-follow opens, which account for case-insensitive volumes, Windows 8.3 names
  and system path aliases such as the macOS `/tmp`, `/var` and `/etc` links.
- **P-TRUSTED-ROOTS**: a root the operator gives is resolved once, where it enters. Everything
  below it is opened without following links.

## Processes

- **P-PROCESS-TREE**: a declared command is spawned through the process layer, so that it is
  admitted before it runs, kept in one process group or Job Object, and cleaned up with its
  children. No direct `Command::spawn` for declared commands.
- **P-EXPLICIT-ENV**: a child's environment is built from an explicit list after clearing the
  inherited one. Windows variable names are compared without regard to case.
- **P-EXIT-STATUS**: exit handling covers an exit code and, on Unix, a terminating signal.
  Windows reports exit codes only.

## Tests

- **P-TEST-HELPERS**: tests use shared operating-system helpers for program paths, temporary
  roots, links, permissions and process control. They do not hard-code `/bin/sh`, `/bin/true`,
  `/proc`, shell-script fixtures or uncanonicalized temporary paths.
- **P-TEST-EVERY-OS**: a test runs on every operating system unless the behavior exists on only
  one. A `cfg` gate on a test carries a comment saying why, and a gated security test (access
  escape, private files, process cleanup) has a counterpart on the other systems.
- **P-TEST-NO-TIMING**: tests do not depend on clock resolution, sleep lengths or names made
  from the time. They use unique temporary directories and explicit synchronization for
  ordering.
