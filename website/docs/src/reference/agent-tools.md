# Agent tools

[Agent evals and backends](../guides/agent-evals.md#agent-tools) shows an Agent tool declaration.

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
`read`, `list`, `section` and `help` work on a fixed target, as described in
[Built-in tools](builtin-tools.md). Only explicitly declared tools are listed.
Built-ins execute in-process without output directories (only `help` starts its
program); `view_image` applies the same image checks as `json` image results.

| Built-in | Arguments | Result |
|---|---|---|
| `read` | `{path, offset?, limit?}`; offset is a 1-based line (default 1), limit defaults to 80, max 500 | `lines: [{number, text}]`, `startLine`, nullable `endLine`, `lineCount`, `totalLines` only when EOF is known, `truncated`, nullable `nextOffset` |
| `list` | `{path?, offset?, limit?}`; offset is 0-based (default 0), limit defaults to/max 200 | Sorted `entries: [{name, path, kind, ...}]`, `totalEntries`, `truncated`, nullable `nextOffset` |
| `glob` | `{pattern, path?}` | Up to 200 sorted logical `files`, `truncated` |
| `grep` | `{pattern, path?, glob?, caseInsensitive?, maxResults?}`; case-sensitive by default, maxResults defaults to/max 200 | `matches: [{path, line, text}]` (one per matching line, sorted by path then line), `truncated` |

Paths are relative logical paths from the tool's declaring Artifact, including
children and mount aliases; omitted paths mean its root. They never accept
absolute paths, traversal components or symlinks. The lone `.` is accepted as
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

Globs use `*` within a component and `**` across directories, relative to `path`.
Grep uses Rust `regex` syntax; `glob` filters relative file paths (or the basename
when `path` names a file). Hidden files are included; git ignore rules do not
filter results. Binary (NUL) and invalid UTF-8 files are skipped in their entirety.
Search follows logical mounts without repeating a mount cycle. Bounds are 10,000
traversed entries, 8 MiB per grep file, 64 MiB searched,
and 512 KiB per JSON result. Search limits or skipped oversized files set
`truncated: true`; narrow the path/pattern to continue. Listing a directory with
more than 10,000 entries returns an error. Listing can page early at the result
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

Scope entries contain canonical physical paths, `kind` (`file` or `folder`) and
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
entry's relative path and digest (a symlink inside contributes its target and is
not followed). The pins are recorded in the result's provenance as
`executionPaths: {"TOOL": {"PATH": "sha256"}}` (by registered tool name, such as
`coverage_spec`, and declared path), travel with remote records, and stay with the
result when it is reused, so a reused verdict shows which binary produced it. A
path that cannot be pinned (missing, a special file, or a folder over 10,000
entries or 1 GiB) fails the review with `PREPARATION_FAILED` before the Agent is
called. Pins are provenance, not part of the
[reuse key](../concepts/fingerprints-and-reuse.md#the-reuse-key): to review again when a binary
changes, cover it in a fingerprint.
Successful stdout is one JSON object with 1–32 content blocks:
`{"content":[{"type":"text","text":"..."},{"type":"json","data":{}}]}`.
Text is at most 64 KiB per block; compact JSON data at most 512 KiB per block;
the normalized result at most 8 MiB. Both process streams are bounded at 16 MiB.
An optional `isError` boolean is accepted. Exit-0 authored errors must have
exactly one nonblank text block, such as
`{"isError":true,"content":[{"type":"text","text":"Choose a smaller range."}]}`.
These reach the reviewer unchanged. Nonzero exit, crash, malformed/truncated
JSON, and process failures yield generic errors; stderr is never forwarded.
There are no observation receipts.

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

Bare executables use PATH only (with PATHEXT on Windows; see
[Program lookup](platforms.md#program-lookup)), never implicit owner or
`node_modules/.bin` lookup. Commands containing `/` resolve from the owner through the scope resolver
(`./tool` is accepted; traversal and symlinks are rejected). Absolute commands run
as given. JSON-protocol argv may use existing scoped Artifact references, but
cannot add Artifacts outside the eval's admitted scope. Plain argv uses only its
schema-property placeholders. Commands themselves are never interpolated.
Command calls use the owner's working directory (the containing folder for a file
Artifact) as cwd, runtime's PATH/LANG-only inheritance and
private external HOME/TMP/output, a default 120000 ms deadline (1–2147483647), and
process-group cancellation/cleanup. Per-call directories are removed on success,
failure and cancellation, including dropped call futures; caller-owned output
roots remain. Commands are trusted read-only programs, not sandboxed.

Internal callers use `tools::Registry::new(&config, "artifact/eval")`, `list()`
and `call(name, args, output_root, cancellation).await`. Only Agent evals are
accepted. Each Artifact in its admitted scope contributes its Agent declarations
under `<name>_<artifactId>`; concatenation collisions are rejected. Human tools
are never listed. Listing creates no directories and runs no owner code. Calls
return normalized `ToolResult {content, is_error}` for successful, authored and
system-error results. Registry calls do not mutate payloads or declarations. The
Agent loop calls this registry.

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
verdict or cache entry. Agent calls use isolated temporary output (removed after
execution); Human calls use the reviewer's real environment. Exit codes are
0 for ready/success, 1 for declaration/preflight/tool/cleanup failure, and 2 for
invalid invocation. `--repo`, `--state-dir` and `--json` are accepted; reports are
JSON even without `--json`.
