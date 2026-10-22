# Operating systems

artifactize runs on Linux (x86_64 and aarch64), macOS (Apple silicon and Intel) and
Windows (x64). WSL 2 runs the Linux build. [Install](../../getting-started/install.md)
has the channels for each. A project, its declarations and its reviews are the same
on every OS; this page lists what differs.

## Project commands run as declared

Runtime evals, fingerprint scripts, command Agent tools and command Human tools run
their `command` with `args` as one argument vector on every OS. There is no shell and
no per-OS variant of a declaration. A project declares commands that exist where it is
reviewed: an eval that runs `python3` or `grep` needs them on every reviewer's `PATH`.
For reading and searching files, the [built-in tools](../tools/builtin.md) work
everywhere without an external program. `open` uses the system opener: `open` on macOS,
the default application on Windows, and `xdg-open` on Linux and WSL, which must be
installed.

Commands run in your own environment, as described in
[Runtime execution](../declarations.md#runtime-execution). What differs by OS: the private
temporary folder is `TMPDIR` on Linux and macOS and also `TMP` and `TEMP` on Windows, and
on Windows the system variables are passed on too.

On Linux, programs the desktop app starts from its AppImage get the environment the app
was started with, minus the settings the AppImage adds. A variable you had set that the
AppImage overwrote, such as `GTK_THEME`, is removed rather than restored; set it again in
the command if it needs it.

## Program lookup

A declared program is found the same way by `tools check`, runtime evals,
fingerprint scripts and command tools:

- A bare name, such as `python3`, is searched on `PATH`. Relative and empty `PATH`
  entries resolve against the command's working directory. On Unix, an unset or
  empty `PATH` therefore searches that directory. On Windows, a name without an extension tries each extension in `PATHEXT`, in order,
  so `npx` finds `npx.cmd`. Without `PATHEXT`, the order is `.COM;.EXE;.BAT;.CMD`.
- A relative name containing `/`, such as `./tools/check`, is resolved inside a
  folder Artifact's admitted scope. For a file Artifact, a relative executable
  resolves beneath its containing folder and may name an unrelated sibling (for
  example `./inspect`). `..` and symlinks are refused in both cases. This executable lookup does
  not make sibling files visible to built-in tools or scoped argv references.
- An absolute path runs as given.

Built-in **help** is different: its literal executable uses ordinary cwd/PATH
lookup, including absolute paths, parent-relative paths and executable symlinks.
Its subcommand arguments are forwarded unchanged, not resolved as scoped file
operands. It runs a trusted program, not an operation confined to Artifact files.

Windows batch files (`.cmd`, `.bat`) can be a command, but not every character can
reach them as an argument. artifactize refuses such an argument instead of passing it
changed. The error reads
`Process arguments could not be passed safely`, and nothing runs.

Tool sessions are stopped and cleaned up the same way on every OS; the
[tool-session contract](../tools/agent.md#tool-sessions) explains what that covers
and the limits of trusted commands.

## Paths

Logical paths use `/` on every OS, in declarations, tool arguments, results and
records. Artifactize matches them case-sensitively on every OS: Artifact
references, tool paths, `.artfignore` and artifactsum `files` patterns match the
exact spelling.

How the spelling on disk is checked differs by OS:

| OS | Spelling on disk |
|---|---|
| macOS, Windows | Artifactize enforces it. On a case-insensitive volume (the default on both), a path must use the exact spelling of the entry on disk, and a file or folder whose name differs from a child Artifact only by case is refused rather than read. |
| Linux | Artifactize relies on the filesystem. A case-sensitive filesystem (the usual one) only opens the exact spelling; on a case-insensitive one, such as a casefolded folder or a Windows drive mounted in WSL, a path that differs only by case may open the same entry. |

## Opening files and the browser

The built-in [`open`](../tools/builtin.md#tools) tool and the `login codex` sign-in open
their target with the desktop's default application:

| OS | Opener |
|---|---|
| Linux | `xdg-open` |
| WSL | `xdg-open`, inside the Linux distribution |
| macOS | `open` |
| Windows | the Windows shell (ShellExecute) |

WSL behaves as Linux: there is no fallback to `wslview` or Windows programs. If
`xdg-open` is missing or opens nothing, install your distribution's `xdg-utils` and
set a default application. `xdg-open` opens files on WSL through `explorer.exe`,
which reports a failure even after opening the file, so on WSL artifactize also
accepts `xdg-open`'s exit status 4. `login codex` also prints its sign-in URL for any browser.

## Desktop console launches

On Windows, launching `artifactize` without arguments from Start or Explorer shows
help plus a terminal hint and waits for Enter if this process is the console's
only attached process and both stdin and stdout are consoles. It never pauses
with arguments, from an existing terminal, or with redirected input/output. Linux
and macOS retain the ordinary no-argument help and immediate exit.

The Microsoft Store package has two visible entries, **Artifactize** for the desktop
app and **Artifactize CLI** for this help launch, and one command alias,
`artifactize`; its tools are available as `artifactize tools`. Non-Store channels
also ship the standalone `artifactize-tools` command.

## State

The default state directory differs by OS: `%LOCALAPPDATA%\artifactize` on Windows,
`~/.local/state/artifactize` on Linux and macOS. The Store version keeps its own
package-private state, which Windows removes on uninstall, and WSL keeps its own Linux
state. See [State](state.md#state) for the full precedence and the Windows overlay
caveat.
