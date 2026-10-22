# Built-in tools

artifactize comes with tools that behave the same on Linux, macOS and Windows,
except for how a path's spelling on disk is checked (see
[Paths](../others/platforms.md#paths)). Declare one by name with `builtin` instead of
writing a script or calling an OS program such as `cat` or `xdg-open`. They need nothing
else installed, not even the [`artifactize-tools`](../../cli/index.md#current-directory-tools) command, except
`open`, which uses the system's opener (`xdg-open` on Linux), and `help`, which runs the
program it asks;
that command and `artifactize tools` offer the filesystem/program tools to scripts
and runtime evals. The project-aware `graph` tool is available only as a declared
review tool in artifactize, not in the standalone tools CLI.

```toml
[views.agent_tools]
read = { builtin = "read" }                                       # the model picks the path
grep = { builtin = "grep" }
style = { builtin = "read", args = ["DEV-CODING.md"] }                 # one fixed file
spec = { builtin = "section", args = ["spec.md"] }                # the model passes only the heading
install = { builtin = "section", args = ["spec.md", "Install"] }  # one fixed section
reqs = { builtin = "read", args = ["reqs/requirements.md"] }      # in the child folder reqs
structure = { builtin = "graph", args = [] }                    # current project entity tree
cargo_help = { builtin = "help", args = ["cargo", "build"] }      # cargo build --help

[views.human_tools]
open = { builtin = "open", args = ["{artifactPath}/notes.md"] }   # kind = "launch"
viewer = { builtin = "graph", kind = "launch", args = [] }       # existing desktop graph view
docs = { builtin = "open", args = ["https://artifactize.dev/docs/"] }
notes = { builtin = "read", args = ["notes.md"] }                 # kind = "output"
summary = { builtin = "section", args = ["{reqs}/requirements.md", "Summary"] }
```

A built-in declaration takes `builtin`, `args` (optional for the Agent tools that
take model input) and an optional `description`; each tool has a default description. A Human built-in may also name its `kind`, which must
match the tool (`open` and `graph` launch, every other tool prints output) or `config check`
rejects it. Built-ins take no `timeout_ms`, `command`, `protocol` or `input_schema`. The
reviewer sees each tool as `<name>_<artifactId>`, as with
[command tools](agent.md#agent-tools).

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
| `graph` | — | `[]` | `[]` (launch) | Agent: entity tree with evals, criteria and directed relations; Human: desktop viewer |
| `open` | — | — | `[file, folder or URL]` (launch) | nothing; opens the default application |

Without `args`, an Agent tool takes the model's input; the input, result and limits of
`read`, `list`, `glob`, `grep` and `view_image` are in
[Agent tools](agent.md#agent-tools). Fixed `args` remove the model input, except
`section` with one argument. Fixed Agent `list` returns a structured JSON content
block; fixed `read`, `section` and `help` return text blocks:

- **`read`** prints a UTF-8 file, at most 64 KiB; a longer file ends with
  `[output truncated]`. It validates only the bounded prefix it inspects: NUL or
  invalid UTF-8 there is refused, but such bytes later in a truncated file may go
  undetected. The CLI fixed read has the same limit.
- **`list`** prints one page of the folder's sorted entries as JSON, like the
  model's `list`.
- **`section`** prints one section of a Markdown file. It finds the first heading
  whose text equals `heading` exactly, at any level (`#` or underlined headings;
  headings inside code blocks do not count). The section runs from that heading to
  the next heading of the same or a higher level, so nested sections are included.
  When no heading matches, the error lists the headings the file has. The heading
  the model passes must be 1–4096 characters, and a declared heading may not be blank.
  The file may be up to 8 MiB; the printed section, up to 64 KiB.
- **`help`** runs `program subcommand... --help` and prints its stdout followed by its
  stderr, read as UTF-8 with terminal color codes and ASCII control characters other than
  tab, LF and CR removed (DEL is kept). The program is
  a literal program or path. Bare names use `PATH` (with `PATHEXT` on Windows).
  Unlike scoped command-tool executables, help uses ordinary program lookup:
  absolute paths, parent-relative paths and executable symlinks are accepted.
  Relative paths start at the declaring Artifact's folder; subcommand arguments
  are forwarded unchanged, without scope resolution. The program runs in that folder with empty stdin
  and gets no private output or temporary directories. For an Agent, it runs in the
  author's environment minus every `ARTIFACTIZE_*` variable (matched ignoring case). For a
  Human, it runs in the reviewer's whole environment, `ARTIFACTIZE_*` variables included,
  such as an `ARTIFACTIZE_REMOTE_TOKEN` credential. It must finish within 10 seconds, exit 0 and
  print at most 64 KiB, stdout and stderr together. It runs a real program, so name
  only programs you trust.
- **`open`** (Human only) opens one file, folder or `http://`/`https://` URL in the
  desktop's default application: `xdg-open` on Linux and WSL, `open` on macOS, the
  Windows shell (ShellExecute) on Windows. The target is passed as one argument,
  never through a shell. Other URL schemes, such as `file:` or `mailto:`, are
  refused. The result only records that the application was
  started; the application belongs to the reviewer and keeps running.
  On Linux, WSL and macOS, the opener itself must exit successfully within three
  seconds: an unavailable opener, timeout or nonzero opener exit is a tool error.
  On WSL, `xdg-open`'s exit status 4 also counts as success, because it opens files
  through `explorer.exe`, which reports a failure even after opening them.
  This acknowledgement deadline is not a lifetime limit on the opened application.

### view_image

`view_image` is an Agent-only tool with required model input `{"path":"board.png"}`.
The path is a logical path inside its admitted Artifact scope, with the usual
no-traversal and no-symlink checks. It reads a nonempty regular file and returns
an embedded image block (`type`, base64 `data`, `mimeType`), not a printed filename.

The file's contents, not its extension, decide the format: PNG, JPEG and WebP are
supported; GIF, BMP, animated PNG and anything else are refused. The file may be at most
**4 MiB**. The
[image-result contract](agent.md#image-results) explains normalization and the
same MIME/size checks for command tools; only those command tools can name files
in their private output directory.

### Current project graph

Declare `builtin = "graph", args = []` on any Artifact; no graph Artifact is
required. It always reads the entire current project's declarations, not the
selected review's historical Run or only its scope. It does not read source
contents, start an eval or add dependencies. The two audience-specific tools use
exactly the extraction that produces the graph fingerprint:

- **Agent:** no model input (`{}` only). Prints a name-based, cycle-safe tree of
  Artifacts and their evals, the complete extracted definitions/evaluation criteria,
  the graph fingerprint, and labeled arrows from input to consumer.
- **Human:** `kind = "launch"`. Starts `artifactize-desktop graph --repo PATH`
  using the normal detached application handoff. `artifactize-desktop` must be
  installed and discoverable through the program search path. The app opens on that
  branch's [Current](../../desktop/index.md#current): the same graph renderer and
  keyboard/zoom interactions as a Run's, but current declarations, each Artifact
  marked as reused or would run, rather than Run verdicts. Expand **Structure and
  evaluation criteria** for the same complete text tree as the Agent sees;
  **Refresh** or Ctrl+R/F5 scans again. This works before any Run exists. The tool
  returns a launch acknowledgement, not an approval; the app keeps running when the
  calling tool finishes.

### Paths in `args`

Filesystem targets must stay inside the scope admitted to the actual review:

- **Fixed Agent tools** use logical paths relative to the declaring Artifact,
  which may lead into a child folder (`reqs/requirements.md`). Discovery and
  `config check` validate these targets against that Artifact's child scope,
  rejecting missing or out-of-scope targets. Calls check them again.
- **Human tools** accept the same logical paths, and also `{artifactPath}` or
  `{artifactPath}/path` and `{name}` or `{name}/path`, where `name` is an Artifact,
  as in
  [Human tools](human.md#human-tools). A `--flag=` prefix is not accepted.
  Discovery and `config check` validate reference names and target existence
  against the workspace, not the narrower review scope. An existing `{other}/file`
  can therefore pass configuration validation without being a child. Preflight
  and calls require that `other` is admitted to the actual Human eval scope, so
  the eval references `{other}` itself; an Artifact-only tool check uses just that
  Artifact's child scope. Tool operands never add dependencies or expand that scope.

Filesystem targets refuse absolute paths, `..`, symlinks and special files at
configuration validation and on every call. `open` additionally accepts HTTP(S)
URLs. `help` takes a literal program or path plus subcommand arguments, without
placeholders; those are program arguments, not filesystem targets for these
scope checks.

### Reuse

Tool declarations, including a built-in's name and `args`, are not part of the
[reuse key](../fingerprint.md#the-reuse-key): a tool is a way of
viewing an Artifact. Changing a tool reuses earlier verdicts, and a new artifactize
release that changes how a built-in behaves does too. To review again when the file a tool reads changes, cover that file in the
fingerprint; the default artifactsum covers the Artifact's own files.

## `artifactize-tools` CLI

The same tools run from scripts and runtime evals as `artifactize tools` or
`artifactize-tools`; see [Current-directory tools](../../cli/index.md#current-directory-tools).
