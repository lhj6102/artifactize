# Platform checklist

Rules that keep code behaving the same on Linux, macOS and Windows. Their purpose: code that
passes its checks on one operating system very likely behaves the same on the others; to that
end, everything that differs between operating systems goes through one platform layer, and
tests cover every OS.

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
  name aliases such as Windows 8.3 names and the macOS `/tmp`, `/var` and `/etc` links,
  filenames reserved on one system (Windows device names such as `NUL`), and replacing or
  deleting a file another handle still has open.
- **P-PATHS**: separators, drive letters, roots and verbatim prefixes. Path text stored or
  shown to a person or a model uses `/` and reads back as the same native path; a path handed
  to the operating system or a program keeps its native spelling.
- **P-TEXT**: turning operating-system strings and programs' output into text: bytes that are
  not UTF-8 on Unix, UTF-16 on Windows, and the CRLF line endings Windows programs write.
- **P-PROGRAMS**: finding a program (`PATH`, and `PATHEXT` on Windows) and opening a file or
  URL in a desktop application.
- **P-PROCESSES**: starting a program from an argument vector (Windows passes one command
  line, quoted for `cmd.exe` when the program is a batch file), stopping a process and its
  children, and exit status (Unix adds signals; Windows has exit codes only).
- **P-ENVIRONMENT**: environment variables (Windows names ignore case), the home, temporary,
  state and data directories, and the user and host names.
- **P-TERMINAL**: interrupt and close events this process receives, and hidden input.
- **P-IPC**: local byte-stream endpoints between processes (Unix sockets, Windows named
  pipes).

Out of scope, because the code meets none of them: shell command strings (declarations are
argument vectors and the product builds no shell text), time zones, the clipboard, shared
memory, configuration and cache directories, and console color modes.

## Tests

- **P-TEST-EVERY-OS**: a test runs on every operating system that has the behavior it tests;
  a `cfg` gate on a test carries a comment saying why. Operating-system setup in tests
  (programs, temporary paths, links, permissions, processes) goes through shared test
  helpers, never hard-coded values.
- **P-TEST-NO-TIMING**: clock resolution and scheduling differ between systems, so tests do
  not depend on the host clock's reading, sleep lengths or names made from the time; fixed
  timestamps and controlled clocks are fine.
