# Agent tools

Declare a command tool under `[views.agent_tools.NAME]` in a tool Artifact and reference
that Artifact from an eval: the eval's Agent then sees it as `NAME_ARTIFACT`. Check it with
[`tools check`](../../cli/index.md#tool-diagnostics) before a review.
[Tools](index.md#build-your-own-tool-artifact) walks through an example.

Command fields `description`, `protocol`, `command` and `args` are required.
`input_schema` defaults to `{"type":"object","additionalProperties":false}`
(empty arguments only). Supplied schemas must have root `type: "object"`. Standard JSON Schema works,
including local `$ref` and composition; references to files or URLs do not. Schema defaults never change arguments. Declarations/schemas are capped
at 8 MiB. Calls require a JSON object of at most 64 KiB before any process or output
directory is created. Validation errors show at most five bounded, escaped
instance/schema paths (under 4 KiB), never argument values.

Descriptions must be nonblank, at most 4000 UTF-16 code units, and support only
`{artifactName}` interpolation (the canonical Artifact ID). Built-in references
take `builtin`, optional `args` and optional `description`. Without `args`, `read`,
`list`, `glob`, `grep` and `view_image` take the model's input below; with `args`,
`read`, `list`, `section` and `help` work on a fixed target, and `graph` with
`args = []` prints the entire current project structure, as described in
[Built-in tools](builtin.md). Only explicitly declared tools are listed.
Built-ins need no program of their own (only `help` starts the program it asks);
`view_image` applies the same image checks as `json` image results.

| Built-in | Arguments | Result |
|---|---|---|
| `graph` | Fixed declaration `args = []`; call `{}` | Complete name-based Artifact/eval tree, extracted criteria, graph fingerprint and input → consumer relations |
| `read` | `{path, offset?, limit?}`; offset is a 1-based line (default 1, at most 9007199254740991), limit defaults to 80, max 500 | `artifactId`, `resolvedArtifactId`, `path`, `lines: [{number, text}]`, `startLine`, nullable `endLine`, `lineCount`, `totalLines` only when EOF is known, `truncated`, nullable `nextOffset` |
| `list` | `{path?, offset?, limit?}`; offset is 0-based (default 0, at most 9007199254740991), limit defaults to/max 200 | `artifactId`, `path`, sorted `entries: [{name, path, kind}]`, `totalEntries`, `truncated`, nullable `nextOffset` |
| `glob` | `{pattern, path?}`; `path` must name a directory | Searched `path`, up to 200 sorted logical `files`, `truncated` |
| `grep` | `{pattern, path?, glob?, caseInsensitive?, maxResults?}`; case-sensitive by default, maxResults defaults to/max 200 | Searched `path`, `matches: [{path, line, text}]` (one per matching line, sorted by path then line), `truncated` |

`read.artifactId` identifies the Artifact that **declared the tool**;
`resolvedArtifactId` identifies the Artifact reached through a child folder.
For example, if app owns the child folder `rules` declared as `spec`, its read tool
called with `{"path":"rules/requirements.md"}` returns ownership metadata
`{"artifactId":"app","resolvedArtifactId":"spec","path":"rules/requirements.md"}`
alongside the line data. `path` remains the logical request path, not a physical
machine path. List's top-level `artifactId` likewise names the declaring Artifact.
Entry kinds are `file`, `directory`, `symlink` or `other`. Glob/grep return the
searched logical `path`, and grep's matching line numbers start at 1.

Paths are relative logical paths from the tool's declaring Artifact, including
its children; omitted paths mean its root. Another Artifact's files are read with
that Artifact's own tools, which an eval admits when it references the Artifact. They never accept
absolute paths, traversal components or symlinks. Logical paths are at most
4096 UTF-16 code units and reject ASCII controls, backslashes, colons and empty
or `.`/`..` components. The lone `.` is accepted as
a file Artifact's virtual root for list/glob/grep; other dot components are
rejected. A tool reaches only the Artifacts in its own eval's scope, never those of
other evals. Files are opened read-only, symbolic links are never followed, and special
files cannot be read. `list` can report `symlink`/`other` entries, but searches skip them.
A file Artifact exposes only its target file, not unrelated siblings in its
working directory. `list` with no path or `path: "."` lists that virtual root's
target file. Read the target by its filename; file paths remain relative to the
containing folder.

Every call validates admitted file targets before invoking a built-in or command
tool, even when reuse is disabled. A missing, non-regular or symlink-replaced
target returns an error without running the command.

Read returns at most 64 KiB of **complete original line bytes**, preserving LF,
CRLF and a UTF-8 BOM in each line's `text`. Only requested lines are decoded;
invalid UTF-8/NUL is an error, as is an oversized first requested line. An empty
file returns zero lines and `totalLines: 0`; a page past EOF returns zero lines
with the actual nonzero total. Concatenating returned `text` values reproduces
the source range without added line-number prefixes.

The `glob` built-in requires a directory `path`; it selects matching filenames
without inspecting contents, including the names of binary files. A file or folder name
that is not valid UTF-8 fails the whole `list`, `glob` or `grep` call, even when it would
not match: narrow the path or rename the entry. `grep` accepts a file
or directory and skips binary (NUL) or invalid-UTF-8 contents in their entirety.
Its optional `glob` argument is a filename filter, not a call to the `glob` tool.

Glob patterns use `*` within a component and `**` across directories, relative to `path`.
Glob and grep patterns (including grep's optional `glob`) must be nonempty and at
most 4096 characters. Globs must be relative, with no leading slash,
backslashes, colons, NUL, or empty/`.`/`..` path components. Grep's pattern is a regular expression, a text-search pattern such as `fn \w+_test` (a
line with `fn ` and a name ending in `_test`), in the syntax of Rust's `regex`; an empty,
invalid or very large pattern is an error. Grep's optional `glob` filter matches relative file paths
(or the basename when `path` names a file). Hidden files are included; git ignore
rules do not filter results.
Bounds are 10,000
traversed entries, 8 MiB per grep file, 64 MiB searched,
and 512 KiB per JSON result. Search limits or skipped oversized files set
`truncated: true`; narrow the path/pattern to continue. Separately, encountering
any one directory with more than 10,000 entries fails `list`, `glob` or `grep`
with an error, not a truncated search result. Listing can page early at the result
byte cap. Built-in arguments use the same bounded JSON Schema admission as commands.

The `json` protocol receives exactly one request on stdin:

```json
{
  "version": 1,
  "context": {
    "artifactId": "example",
    "artifactPath": "/workspace/example",
    "outputDir": "/external/private/output",
    "tmpDir": "/external/private/tmp",
    "scope": {
      "example": {"path": "/workspace/example", "kind": "folder", "children": {}}
    },
    "executionPaths": {"shared/rules.json": "/workspace/shared/rules.json"}
  },
  "args": {"section": "summary"}
}
```

Scope entries contain canonical physical paths, `kind` (`file`, `folder` or `graph`) and
logical child maps. For a file Artifact, `context.artifactPath` and its
scope entry's `path` name the file, not the process cwd; the cwd is its containing
folder. File Artifact references reject `/path` suffixes.
These protocol keys stay camelCase; declarations use `execution_paths` and
`input_schema`. `executionPaths` is
omitted when empty. Declared execution paths are up to 64 unique workspace-relative
files/directories of up to 1024 UTF-16 code units each, resolved without symlinks or copying. This remains
workspace-relative for file Artifacts; it is not relative to their cwd or target.

When an Agent review starts, artifactize records the SHA-256 of each execution path
of the eval's tools (for a directory, of its files and symbolic links) in the result as
`executionPaths: {"TOOL": {"PATH": "sha256"}}`, by published tool name such as
`coverage_spec`. The record travels with remote results and stays with a reused result,
so a reused verdict shows which binary produced it. A path that is missing, is a special
file, holds more than 1 GiB or more than 10,000 files fails the review with
`PREPARATION_FAILED` before the Agent is called. These hashes are a record, not part of
the [reuse key](../fingerprint.md#the-reuse-key): to review again when a binary changes,
cover it in a fingerprint.
Successful stdout is one JSON object with 1–32 content blocks:
`{"content":[{"type":"text","text":"..."},{"type":"json","data":{}}]}`.
Image blocks are described [below](#image-results).
Text is at most 64 KiB per block; compact JSON data at most 512 KiB per block;
the normalized result at most 8 MiB. Both process streams are bounded at 16 MiB.
The result object accepts only `content` and optional boolean `isError`.
Every block is also closed: text permits only `type` and `text`; JSON permits
only `type` and `data`; image permits only `type`, `mimeType` and exactly one of
`data` or `path`. Any extra field on a result or block is an invalid-output error,
not metadata passed through to the reviewer. Exit-0 authored errors must have
exactly one nonblank text block, such as
`{"isError":true,"content":[{"type":"text","text":"Choose a smaller range."}]}`.
These reach the reviewer unchanged. Nonzero exit, crash, malformed/truncated
JSON, and process failures yield generic errors; stderr is never forwarded.
There are no observation receipts.

### Image results

A JSON command can return an image block with `mimeType` and **exactly one** of
`data` (standard base64 bytes) or `path` (an output file):

```json
{"content":[{"type":"image","mimeType":"image/png","data":"<base64-encoded PNG bytes>"}]}
```

```json
{"content":[{"type":"image","mimeType":"image/png","path":"screenshots/board.png"}]}
```

Write the second form's file below the request's `context.outputDir`. A relative
path starts there; an absolute path is accepted only when it is still below that
directory. Traversal, symlinks in any component, special files and files outside
that output directory are refused. Artifactize reads and embeds the bytes before
removing the invocation's output, so the returned result is self-contained base64,
not a dangling local file path.

Images must be nonempty PNG, JPEG or WebP, at most **4 MiB inclusive** of
image-file bytes after base64 decoding, not decoded pixels. Detection checks
signatures/container framing rather than full image validity. Those bytes, not
the filename extension, identify the format and must match
`image/png`, `image/jpeg` or `image/webp` respectively. Invalid base64, a MIME
mismatch, unsupported formats such as GIF/BMP, and animated PNG are rejected.
The overall normalized 8 MiB tool-result limit still applies after base64 encoding.
[`view_image`](builtin.md#view_image) returns the same embedded image form and
uses the same byte/format limits, but reads its file from admitted Artifact scope.

For `plain`, only top-level declared `input_schema.properties` can appear as argv
placeholders. Whole-token `{query}` and embedded `--query={query}` both work.
Strings substitute literally; other JSON values use compact JSON. Missing values
and NUL bytes fail before spawn. `{{` and `}}` escape literal braces. Substitution
is single-pass: values containing braces, quotes, spaces, `$()` or semicolons
remain one literal argv element, never shell code. Plain tools receive empty
stdin. Their stdout becomes one text block, read as UTF-8 (invalid bytes replaced) with
terminal color codes and ASCII control characters other than tab, LF and CR removed (DEL is kept), capped at 64 KiB
with an explicit truncation marker; nonzero exit marks that bounded stdout as a
tool error. Stderr is not included. Commands such as `rg` that exit nonzero for
no matches need an owner wrapper if that should count as successful empty output.

Command executables follow the shared [Program lookup](../others/platforms.md#program-lookup)
rules, including the file-Artifact exception and rejection of scoped traversal.
JSON-protocol argv may use existing scoped Artifact references, but
cannot add Artifacts outside the eval's admitted scope. Plain argv uses only its
schema-property placeholders. Commands themselves are never interpolated.
A command runs in its Artifact's folder (the containing folder for a file Artifact),
in the author's own environment (the
[runtime environment](../declarations.md#runtime-execution): the real home, no
`ARTIFACTIZE_*` variables of the caller) with a private temporary and output folder per
call, removed when the call ends. The default deadline is 120000 ms (1–2147483647);
cancellation and a timeout stop the command and what it started. Commands are trusted
read-only programs, not sandboxed.

An Agent eval sees the Agent tools of every Artifact in its scope, each named
`<name>_<artifactId>`, such as `coverage_spec`; two tools that would get the same name
are a declaration error. Human tools are never offered to an Agent.

## Tool sessions

A **tool session** keeps a harness running across calls during one review. It is
not a saved Agent conversation. Declare its start and stop commands in the same
Artifact as its bound command tools:

```toml
[views.agent_sessions.app]
start = { command = "harness", args = ["start"], execution_paths = ["tools/harness"] }
stop = { command = "harness", args = ["stop"], execution_paths = ["tools/harness"] }

[views.agent_tools.click]
session = "app"
description = "Click an element of the running app."
command = "harness"
args = ["act"]
protocol = "json"
```

`start` and `stop` require `command` and `args`; their only optional field is
`execution_paths`. They use the usual executable lookup, scoped argv and JSON
stdin envelope, with `args: {}`. They do not take `description`, `protocol`,
`timeout_ms` or concurrency options. A command tool's `session` must name a
session in its own Artifact; built-in and Human tools do not accept it.

The harness implements this start contract:

- Read the usual JSON envelope on stdin. `ARTIFACTIZE_SESSION_DIR` names a private
  directory supplied by artifactize; use it for state shared by bound tools and stop.
- When ready, print one newline-terminated line on stdout. Its content is ignored.
- Keep the start process running for the whole session. Its exit ends the session.

Bound tools, whether `json` or `plain`, and stop receive the same
`ARTIFACTIZE_SESSION_DIR` and run in the author's environment, real home included, like
every command.
Each invocation still has its own private output and temporary directories.
Concurrent reviews have independent session directories and environments, even
on the same Artifact. Choose ports/displays through standard OS mechanisms
(for example port 0 or an automatically selected display); a desktop harness
starts its own headless display. The Agent sees no instance ID and cannot call
start or stop.

Execution-path pinning still happens at review start. A tool session starts only
when the Agent first calls a tool bound to it; later bound calls reuse that session.
An admitted session whose tools are never called starts no process and creates no
session directory. Startup and readiness count toward the review's timeout.
If start cannot run or exits before it is ready, the Agent gets that as the tool
call's error, not `PREPARATION_FAILED`, and later calls to the session return the same
error without starting it again. There is no separate readiness timeout: start may
prepare until the review times out or is cancelled. Output after the ready line is
discarded.

If the start process exits during the review, artifactize stops what it started and
cancels running bound calls. Later bound calls return
``The tool session `app` ended.``; there is no restart. This is an ordinary tool
error in the saved conversation.

However the review ends (verdict, error, timeout or cancellation), every session that
started is stopped after the Agent finishes and before the result is checked and saved.
Running bound calls are cancelled first, then stop runs, with up to 30 seconds before
artifactize ends the session's programs itself. The session folder is removed once they
have ended, even when stop fails or hangs. If artifactize itself is killed, stop cannot run. A cleanup failure is reported
instead of a successful result, and while an earlier session could not be cleaned up, a
new one is refused. A reused or joined result starts nothing.

Start/stop execution-path pins use `<session>_<artifact>.start` and `.stop` keys
in the result's existing `executionPaths` provenance, including remote records.
`tools check` statically checks the referenced session's executables and operands
without running it. `tools check --execute` on a bound tool and `session send`
each start a new session on the first bound call and stop it afterward. A follow-up
that calls no bound tools starts nothing.

Session commands are **trusted, not sandboxed**: they can change the workspace, and a
program that deliberately detaches from them may keep running.

Check tool declarations and run a tool by hand with
[`tools check`](../../cli/index.md#tool-diagnostics).
