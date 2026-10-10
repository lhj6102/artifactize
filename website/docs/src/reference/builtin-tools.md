# Built-in tools

artifactize ships a set of tools written in Rust that behave the same on Linux, macOS
and Windows. Declare one by name with `builtin` instead of writing a script or calling
an OS program such as `cat` or `xdg-open`. Built-in tools run inside artifactize, so
they do not need the [`artifactize-tools`](#artifactize-tools-cli) command on `PATH`;
that command offers the same tools to scripts and runtime evals.

```toml
[views.agent_tools]
read = { builtin = "read" }                                       # the model picks the path
grep = { builtin = "grep" }
style = { builtin = "read", args = ["STYLE.md"] }                 # one fixed file
spec = { builtin = "section", args = ["spec.md"] }                # the model passes only the heading
install = { builtin = "section", args = ["spec.md", "Install"] }  # one fixed section
reqs = { builtin = "read", args = ["reqs/requirements.md"] }      # through the mount alias
cargo_help = { builtin = "help", args = ["cargo", "build"] }      # cargo build --help

[views.human_tools]
open = { builtin = "open", args = ["{artifactPath}/notes.md"] }   # kind = "launch"
docs = { builtin = "open", args = ["https://artifactize.dev/docs/"] }
notes = { builtin = "read", args = ["notes.md"] }                 # kind = "output"
summary = { builtin = "section", args = ["{reqs}/requirements.md", "Summary"] }
```

A built-in declaration takes `builtin`, `args` (optional for the Agent tools that
take model input) and an optional `description`; each tool has a default description. A Human built-in may also name its `kind`, which must
match the tool (`open` launches, every other tool prints output) or `config check`
rejects it. Built-ins take no `timeout_ms`, `command`, `protocol` or `input_schema`. The
reviewer sees each tool as `<name>_<artifactId>`, as with
[command tools](agent-tools.md#agent-tools).

## Tools

| Tool | Agent, model input | Agent, fixed `args` | Human `args` | Prints |
|---|---|---|---|---|
| `read` | `{path, offset?, limit?}` | `[file]` | `[file]` | the file |
| `list` | `{path?, offset?, limit?}` | `[folder]` | `[folder]` | the sorted entries, as JSON |
| `glob` | `{pattern, path?}` | — | — | matching paths |
| `grep` | `{pattern, path?, glob?, caseInsensitive?, maxResults?}` | — | — | matching lines |
| `view_image` | `{path}` | — | — | the image |
| `section` | — | `[file]` (the model passes `heading`) or `[file, heading]` | `[file, heading]` | one Markdown section |
| `help` | — | `[program, subcommand...]` | `[program, subcommand...]` | `program subcommand... --help` |
| `open` | — | — | `[file, folder or URL]` (launch) | nothing; opens the default application |

Without `args`, an Agent tool takes the model's input; the input, result and limits of
`read`, `list`, `glob`, `grep` and `view_image` are in
[Agent tools](agent-tools.md#agent-tools). With `args`, the tool takes no model input
(except `section` with one argument) and returns text:

- **`read`** prints a UTF-8 file, at most 64 KiB; a longer file ends with
  `[output truncated]`. Binary and invalid UTF-8 files are refused.
- **`list`** prints one page of the folder's sorted entries as JSON, like the
  model's `list`.
- **`section`** prints one section of a Markdown file. It finds the first heading
  whose text equals `heading` exactly, at any level (`#` or underlined headings;
  headings inside code blocks do not count). The section runs from that heading to
  the next heading of the same or a higher level, so nested sections are included.
  When no heading matches, the error lists the headings the file has. The file may
  be up to 8 MiB; the printed section, up to 64 KiB.
- **`help`** runs `program subcommand... --help` and prints its output. The program is
  a literal name, found on `PATH` (with `PATHEXT` on Windows; see
  [Program lookup](platforms.md#program-lookup)), and runs in the declaring
  Artifact's folder with empty stdin. It must finish within 10 seconds, exit 0 and
  print at most 64 KiB, stdout and stderr together. It runs a real program, so name
  only programs you trust.
- **`open`** (Human only) opens one file, folder or `http://`/`https://` URL in the
  desktop's default application: `xdg-open` on Linux and WSL, `open` on macOS, the
  Windows shell (ShellExecute) on Windows. The target is passed as one argument,
  never through a shell. Other URL schemes, such as `file:` or `mailto:`, are
  refused. The result only records that the application was
  started; the application belongs to the reviewer and keeps running.

### Paths in `args`

`args` names a target inside the declaring Artifact's scope, never outside it:

- **Agent tools** use logical paths, as the model does: relative to the declaring
  Artifact, with a mount alias or child folder as the first component
  (`reqs/requirements.md`).
- **Human tools** accept the same logical paths, and also `{artifactPath}` or
  `{artifactPath}/path` and `{name}` or `{name}/path`, where `name` is an Artifact
  or a mount alias of the declaring Artifact, as in
  [Human tools](human-tools.md#human-tools). A `--flag=` prefix is not accepted.

Absolute paths, `..`, symlinks and special files are refused. `config check` reports a
missing or out-of-scope target, and every call checks it again. `help` arguments are
literal names, not paths, and accept no placeholders.

### Reuse

Tool declarations, including a built-in's name and `args`, are not part of the
[reuse key](../concepts/fingerprints-and-reuse.md#the-reuse-key): a tool is a way of
viewing an Artifact. Changing a tool reuses earlier verdicts, and a new artifactize
release that changes how a built-in behaves does too. To review again when the file a tool reads changes, cover that file in the
fingerprint; the default artifactsum covers the Artifact's own files.

## `artifactize-tools` CLI

Every installer puts `artifactize-tools` next to `artifactize`. It offers the same
tools as subcommands, for runtime evals, fingerprint scripts and your own scripts:

```sh
artifactize-tools read STYLE.md
artifactize-tools list [PATH]
artifactize-tools glob PATTERN [PATH]
artifactize-tools grep [--glob GLOB] [--case-insensitive] PATTERN [PATH]
artifactize-tools section spec.md "Install"
artifactize-tools help cargo build          # cargo build --help
artifactize-tools open notes.md             # or an http(s) URL
```

There is no Artifact scope outside a review, so the current directory is the scope.
Every path is relative to it and must stay below it; `.` names the directory itself
and is the default for `list`, `glob` and `grep`. Absolute paths, `..` and symlinks are
refused, even when the link points inside the directory. Only `open` accepts a URL,
and only `http://` or `https://`.

`read`, `section` and `help` print text with the limits above; `list`, `glob` and
`grep` print the JSON result of the Agent tool. A tool error goes to stderr with exit
1; invalid usage exits 2. `artifactize-tools --help` and `artifactize-tools
SUBCOMMAND --help` describe each subcommand.
