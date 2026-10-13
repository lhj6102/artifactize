# Platform checklist

Rules that keep code behaving the same on Linux, macOS and Windows.

## One place for what differs

- **P-ONE-LAYER**: everything whose implementation or behavior differs between operating
  systems lives in the platform layer. Code outside it calls the layer's portable interface
  and has no operating-system calls (`std::os::unix`, `std::os::windows`, `libc`,
  `windows-sys`) and no `cfg(unix)`, `cfg(windows)` or `cfg(target_os = …)` branches.
- **P-NO-OS-VALUES**: code outside the platform layer names no operating-system specific
  value: no `/tmp`, `/proc`, `/dev/null`, `/bin/…`, drive letters, `.exe`, `sh`, `cmd`,
  `xdg-open`, `open`, `explorer` or `HOME`.

## What differs

These go through the platform layer:

- **P-FILES**: links and reparse points, permissions and owner-only access, case sensitivity,
  and name aliases such as Windows 8.3 names and the macOS `/tmp`, `/var` and `/etc` links.
- **P-PATHS**: separators, drive letters and roots. A path shown to a person or a model, or
  stored as text, uses `/`.
- **P-PROGRAMS**: finding a program (`PATH`, and `PATHEXT` on Windows) and opening a file or
  URL in a desktop application.
- **P-COMMANDS**: commands run as an argument vector, never as a shell string, because the
  shells differ. Launching the user's editor is the one exception.
- **P-PROCESSES**: starting, stopping and checking processes and their children, and exit
  status (Unix adds signals; Windows has exit codes only).
- **P-ENVIRONMENT**: environment variables (Windows names ignore case), the home, temporary
  and state directories, and the user and host names.
- **P-TERMINAL**: interrupt and stop signals, hidden input, and the user's editor.
- **P-IPC**: local endpoints between processes (Unix sockets, Windows named pipes).

## Tests

- **P-TEST-EVERY-OS**: a test runs on every operating system unless the behavior exists on
  only one; a `cfg` gate on a test carries a comment saying why. Operating-system setup in
  tests (programs, temporary paths, links, permissions, processes) goes through shared test
  helpers, never hard-coded values.
- **P-TEST-NO-TIMING**: clock resolution and scheduling differ between systems, so tests do
  not depend on timestamps, sleep lengths or names made from the time.
