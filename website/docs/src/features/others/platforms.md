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
For reading, searching and opening files, the [built-in tools](../tools/builtin.md) work
everywhere without an external program.

Runtime evals, fingerprint scripts and command Agent tools also run in the author's own
environment: `artifactize` passes on its whole environment (the home and cache variables of
each OS, and on Windows the system variables too), so a toolchain behind a version manager
resolves as in a terminal. Only `ARTIFACTIZE_*` variables are withheld, and each run gets a
private `TMPDIR` (`TMP` and `TEMP` as well) of its own. See the
[runtime environment](../declarations.md#runtime-execution-library). Command Human tools
inherit the reviewer's complete environment, `ARTIFACTIZE_*` variables included, and get no
private temporary or output directories ([Human tools](../tools/human.md)).

## Program lookup

A declared program is found the same way by `tools check`, runtime evals,
fingerprint scripts, command tools and the built-in `help`:

- A bare name, such as `python3`, is searched on `PATH`, never in the current folder.
  On Windows, a name without an extension tries each extension in `PATHEXT`, in order,
  so `npx` finds `npx.cmd`. Without `PATHEXT`, the order is `.COM;.EXE;.BAT;.CMD`.
- A relative name containing `/`, such as `./tools/check`, is resolved inside a
  folder Artifact's admitted scope. For a file Artifact, a non-mount relative
  executable resolves beneath its containing folder and may name an unrelated
  sibling (for example `./inspect`); a mount-prefixed path still uses the admitted
  scope. `..` and symlinks are refused in both cases. This executable lookup does
  not make sibling files visible to built-in tools or scoped argv references.
- An absolute path runs as given.

Windows batch files (`.cmd`, `.bat`) run directly, without a shell string: the Rust
standard library quotes each argument for `cmd.exe` and refuses an argument it cannot
pass safely. The error reads `Process arguments could not be passed safely`, and
nothing runs.

Tool sessions use process groups on Unix and kill-on-close Jobs on Windows.
Artifactize owns their cleanup and recovery; the cross-platform
[tool-session contract](../tools/agent.md#tool-sessions) explains the guarantees
and the limits of trusted commands.

## Paths

Logical paths use `/` on every OS, in declarations, tool arguments, results and
records. They are case-sensitive everywhere: Artifact references, tool paths,
`.artfignore` and artifactsum `files` patterns match the exact spelling. On a
case-insensitive volume (the macOS default, and Windows), a path must also use the
exact spelling of the entry on disk, and a file or folder whose name differs from a
child Artifact or mount alias only by case is refused rather than read.

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
set a default application. `login codex` also prints its sign-in URL for any browser.

## Editor

Standalone `artifactize review` edits the owner fields in `$VISUAL`, else `$EDITOR`,
else `vi` (Notepad on Windows). The variable may hold arguments, such as
`code --wait`.

## Desktop console launches

On Windows, launching `artifactize` without arguments from Start or Explorer shows
help plus a terminal hint and waits for Enter if this process is the console's
only attached process and both stdin and stdout are consoles. It never pauses
with arguments, from an existing terminal, or with redirected input/output. Linux
and macOS retain the ordinary no-argument help and immediate exit.

The Microsoft Store package has one visible **Artifactize** entry and one command
alias, `artifactize`; its tools are available as `artifactize tools`. Non-Store
channels also ship the standalone `artifactize-tools` command.

## State

The state directory is `$ARTIFACTIZE_STATE_HOME`, else `$XDG_STATE_HOME/artifactize`,
else `%LOCALAPPDATA%\artifactize` on Windows, else `~/.local/state/artifactize` (also
on macOS). On Windows, every install for one user shares it, the Microsoft Store app
included; WSL keeps its own. See [State](state.md#state).
