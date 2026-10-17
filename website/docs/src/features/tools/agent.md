# Agent tools

[Tools](index.md#build-your-own-tool-artifact) shows how to declare and attach a custom tool Artifact.

Command fields `description`, `protocol`, `command` and `args` are required.
`input_schema` defaults to `{"type":"object","additionalProperties":false}`
(empty arguments only). Supplied schemas must have root `type: "object"` and are
compiled by `jsonschema`, without its file/HTTP resolution features. Local `$ref`,
composition and other standard schema features work; there is no custom keyword
subset. Schema defaults never change arguments. Declarations/schemas are capped
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
Built-ins execute in-process without output directories (only `help` starts its
program); `view_image` applies the same image checks as `json` image results.

| Built-in | Arguments | Result |
|---|---|---|
| `graph` | Fixed declaration `args = []`; call `{}` | Complete name-based Artifact/eval tree, extracted criteria, graph fingerprint and input → consumer relations |
| `read` | `{path, offset?, limit?}`; offset is a 1-based line (default 1), limit defaults to 80, max 500 | `artifactId`, `resolvedArtifactId`, `path`, `lines: [{number, text}]`, `startLine`, nullable `endLine`, `lineCount`, `totalLines` only when EOF is known, `truncated`, nullable `nextOffset` |
| `list` | `{path?, offset?, limit?}`; offset is 0-based (default 0), limit defaults to/max 200 | `artifactId`, `path`, sorted `entries: [{name, path, kind, artifactId?}]`, `totalEntries`, `truncated`, nullable `nextOffset` |
| `glob` | `{pattern, path?}`; `path` must name a directory | Searched `path`, up to 200 sorted logical `files`, `truncated` |
| `grep` | `{pattern, path?, glob?, caseInsensitive?, maxResults?}`; case-sensitive by default, maxResults defaults to/max 200 | Searched `path`, `matches: [{path, line, text}]` (one per matching line, sorted by path then line), `truncated` |

`read.artifactId` identifies the Artifact that **declared the tool**;
`resolvedArtifactId` identifies the Artifact reached through a child or mount.
For example, if app declares `mounts = { rules = "spec" }`, its read tool called
with `{"path":"rules/requirements.md"}` returns ownership metadata
`{"artifactId":"app","resolvedArtifactId":"spec","path":"rules/requirements.md"}`
alongside the line data. `path` remains the logical request path, not a physical
machine path. List's top-level `artifactId` likewise names the declaring Artifact.
Entry kinds are `file`, `directory`, `symlink`, `other` or `mount`; only a mount
entry adds `artifactId`, naming its target Artifact. Glob/grep return the searched
logical `path`, and grep's matching line numbers start at 1.

Paths are relative logical paths from the tool's declaring Artifact, including
children and mount aliases; omitted paths mean its root. They never accept
absolute paths, traversal components or symlinks. Logical paths are at most
4096 UTF-16 code units and reject ASCII controls, backslashes, colons and empty
or `.`/`..` components. The lone `.` is accepted as
a file Artifact's virtual root for list/glob/grep; other dot components are
rejected. The registry scope remains the eval's
admitted Artifacts, not other evals' references. Paths are opened read-only through
pinned directory descriptors with no-follow component checks; special files cannot
be read. `list` can report `symlink`/`other` entries, but searches skip them.
Mounts have kind `mount`. A file Artifact exposes only its target file, mounts and
referenced Artifacts, not unrelated siblings in its working directory.
`list` with no path or `path: "."` lists that virtual root's target file and
declared mounts. Read the target by its filename; file paths remain relative to
the containing folder. A mount alias may shadow an unrelated physical sibling,
but not the target filename.

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
without inspecting contents, including binary filenames. `grep` accepts a file
or directory and skips binary (NUL) or invalid-UTF-8 contents in their entirety.
Its optional `glob` argument is a filename filter, not a call to the `glob` tool.

Glob patterns use `*` within a component and `**` across directories, relative to `path`.
Glob and grep patterns (including grep's optional `glob`) must be nonempty and at
most 4096 characters. Globs must be relative, with no leading slash,
backslashes, colons, NUL, or empty/`.`/`..` path components. Grep uses Rust `regex`
syntax, with a 2 MiB compiled-regex size ceiling; an empty, invalid or oversized
regex is an error. Grep's optional `glob` filter matches relative file paths
(or the basename when `path` names a file). Hidden files are included; git ignore
rules do not filter results.
Search follows logical mounts without repeating a mount cycle. Bounds are 10,000
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
      "example": {"path": "/workspace/example", "kind": "folder", "children": {}, "mounts": {}}
    },
    "executionPaths": {"shared/rules.json": "/workspace/shared/rules.json"}
  },
  "args": {"section": "summary"}
}
```

Scope entries contain canonical physical paths, `kind` (`file`, `folder` or `graph`) and
logical child/mount maps. For a file Artifact, `context.artifactPath` and its
scope entry's `path` name the file, not the process cwd; the cwd is its containing
folder. File Artifact references reject `/path` suffixes.
These protocol keys stay camelCase; declarations use `execution_paths` and
`input_schema`. `executionPaths` is
omitted when empty. Declared execution paths are up to 64 unique workspace-relative
files/directories, resolved without symlinks or copying. This remains
workspace-relative for file Artifacts; it is not relative to their cwd or target.

When an Agent review starts, artifactize pins each execution path of the eval's
tools: a file to the SHA-256 of its bytes, a directory to the SHA-256 over each
file's or symlink's relative path and digest (a symlink inside contributes its target
and is not followed). Directories are only traversed: they do not contribute a
digest or increment the entry counter, so empty directories do not count. The pins
are recorded in the result's provenance as
`executionPaths: {"TOOL": {"PATH": "sha256"}}` (by registered tool name, such as
`coverage_spec`, and declared path), travel with remote records, and stay with the
result when it is reused, so a reused verdict shows which binary produced it. A
path that cannot be pinned (missing, a special file, or more than 1 GiB of file
bytes) fails the review with `PREPARATION_FAILED` before the Agent is called.
Pins retain at most 10,000 file/symlink entries; after that many have been retained,
encountering any further filesystem entry fails preparation. This is not a total
filesystem traversal bound: directory visits alone, including empty directories,
do not increment the counter. Pins are provenance, not part of the
[reuse key](../fingerprint.md#the-reuse-key): to review again when a binary
changes, cover it in a fingerprint.
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
stdin. Cleaned, lossily decoded stdout becomes one text block, capped at 64 KiB
with an explicit truncation marker; nonzero exit marks that bounded stdout as a
tool error. Stderr is not included. Commands such as `rg` that exit nonzero for
no matches need an owner wrapper if that should count as successful empty output.

Command executables follow the shared [Program lookup](../others/platforms.md#program-lookup)
rules, including the file-Artifact exception and rejection of scoped traversal.
JSON-protocol argv may use existing scoped Artifact references, but
cannot add Artifacts outside the eval's admitted scope. Plain argv uses only its
schema-property placeholders. Commands themselves are never interpolated.
Command calls use the owner's working directory (the containing folder for a file
Artifact) as cwd, the author's own environment (the
[runtime environment](../declarations.md#runtime-execution-library): the real home, no
`ARTIFACTIZE_*` variables of the caller) with private external TMP/output, a default 120000 ms deadline (1–2147483647), and
process-group cancellation/cleanup. Per-call directories are removed on success,
failure and cancellation, including dropped call futures; caller-owned output
roots remain. Commands are trusted read-only programs, not sandboxed.

Internal callers use `tools::Registry::new(&config, "artifact/eval")`, `list()`
and `call(name, args, output_root, cancellation).await`. Only Agent evals are
accepted. Each Artifact in its admitted scope contributes its Agent declarations
under `<name>_<artifactId>`; tool-name and session-provenance-name concatenation
collisions are rejected. Human tools
are never listed. Listing creates no directories and runs no owner code. Calls
return normalized `ToolResult {content, is_error}` for successful, authored and
system-error results. Registry calls do not mutate payloads or declarations. The
Agent loop calls this registry.

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

After execution-path pinning, artifactize starts only the sessions referenced by
the eval's admitted tools, while holding the review's backend slot. Every session
must be ready before the first model request. Spawn failure, exit before readiness,
or readiness timeout produces `PREPARATION_FAILED` with no model request; already
started sessions undergo stop and cleanup rollback. Any cleanup failure is also
reported, with unconfirmed state retained for recovery. Readiness has an internal
30-second bound. After readiness, stdout and stderr are drained and discarded.

If the start process exits during the review, artifactize immediately kills its
remaining process group/Job and cancels in-flight bound calls. The cleaned group
is released once termination is confirmed, so later review cleanup does not signal
its old numeric process-group ID. A failure to retire its saved identity is reported
without skipping cancellation/join of bound calls or stop. Later bound calls return
``The tool session `app` ended.``; there is no restart. This is an ordinary tool
error in the saved conversation, not a new event kind.

Stop is attempted after the Agent loop and before input revalidation, fingerprint recheck
and result commit, on every ending: verdict, error, timeout, cancellation or dropped
review future, including a review panic. Cleanup cancels and awaits in-flight bound
calls before running stop. Stop uses its own cancellation token and an internal
30-second bound, then artifactize kills and reaps the session group/Job. Its
directory and admission records are removed only after all owned-process cleanup
is confirmed, even when stop itself fails or hangs. Windows cleanup waits for all
Job descendants, with a separate internal 30-second termination bound.

A lazy cleanup executor owns session lifetimes independently of the caller's
runtime: dropping that runtime still runs stop and cleanup. Process termination
cannot run stop. Before the next tool session starts, artifactize sweeps directories
whose saved owner identity has exited (including an unreaped zombie), or whose
supervisor lease has been released after a cleanup failure even while its owner
process remains alive. Active leases protect concurrent reviews. On Unix, start,
stop and bound invocation groups are recorded before admission and each saved PID/start-time identity is checked
before signalling, including when the group leader is already gone. Windows Jobs
terminate when their owner dies; recovery waits up to 30 seconds for pending file
sharing dispositions to clear. Cleanup or record-retirement failures are reported
instead of publishing a successful result. Unconfirmed process cleanup retains its
journals for recovery; failed directory removal leaves the remaining state for the
next start to sweep after the lease is released. Sweep errors preserve the journals
and refuse the new session instead of admitting it. A reused or waited-on result
starts nothing.

Start/stop execution-path pins use `<session>_<artifact>.start` and `.stop` keys
in the result's existing `executionPaths` provenance, including remote records.
`tools check` statically checks the referenced session's executables and operands
without running it. `tools check --execute` on a bound tool and `session send`
each own a fresh tool session and clean it up afterward.

Internal callers start sessions with
`registry.start_sessions(state, selected_tool_or_none, cancellation).await`,
attach them with `registry.with_sessions(&sessions)`, then release and await
`sessions.close().await` after their calls. Dropping the guard also starts cleanup.
Session commands remain **trusted, not sandboxed**: commands can modify the
workspace or escape cleanup by deliberately detaching descendants.

## Tool diagnostics

```sh
artifactize tools check                         # every Agent/Human eval's scope
artifactize tools check app/review              # same as --eval app/review
artifactize tools check --artifact app --audience agent
artifactize tools check --execute --artifact app --audience agent --tool read --args '{"path":"README.md"}'
artifactize tools check --execute --artifact app --audience human --tool inspect
```

`tools check` discovers and validates declarations, resolves executable availability,
scoped argv operands and declared execution paths, and prints JSON scopes, schemas
and per-tool readiness checks. It is static by default: no owner process, fingerprint
script, database or output directory is created. A positional selector or `--eval`
selects one Agent/Human eval and cannot be combined with `--artifact`, `--audience`,
`--tool` or `--execute`. Explicit execution requires all three Artifact, audience
and tool flags; the short operation name or its published name is accepted.
`--args` is valid only for Agent execution; Human schemas admit only an empty
object and Human commands take no free arguments. A check never creates a Run,
verdict or cache entry. Agent calls run in the author's environment with private temporary output (removed after
execution); Human calls use the reviewer's real environment. Exit codes are
0 for ready/success, 1 for declaration/preflight/tool/cleanup failure, and 2 for
invalid invocation. `--repo`, `--state-dir` and `--json` are accepted; reports are
JSON even without `--json`.
