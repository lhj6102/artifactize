# Artifactize

A Rust rebrand of [CCDD](https://github.com/lhj6102/ccdd), ported from CCDD 7.0.0 (`cbf28b4`).

Work in progress. See the [plan](docs/PLAN.md) and the [capability inventory](docs/ccdd-7-inventory.md).

## Runtime CLI

```sh
cargo run -q -p artifactize -- --repo crates/artifactize/tests/fixtures/runtime verify --all
cargo run -q -p artifactize -- run show RUN_ID
```

The fixture intentionally includes GREEN, RED, a timeout ERROR, RED-blocked and
ERROR-waiting dependents, and a two-Artifact cycle. Its overall exit code is 2.
`verify` requires exactly one selector: positional `ARTIFACT`, `--eval ID`,
`--evals CSV`, `--artifacts CSV`, `--evals-file PATH`, `--artifacts-file PATH`,
or `--all`. Eval IDs are qualified (`green/check`). Selection order is preserved,
with duplicates removed at their first occurrence; Artifacts expand to their
evals in declaration order. READY runtime evals execute concurrently up to
`--jobs N` (default 4, minimum 1), with dispatch in that same ordered selection
then recursive configuration order. The graph is re-evaluated after each result;
newly READY evals do not wait for an entire batch to finish. Identity
claim waiters occupy job slots while polling, but cache hits occupy none.
An Artifact selector runs only that Artifact's evals;
an individual eval selector runs only that eval. Both retain the full dependency
closure as a final obligation. `--recursive` includes every eval in that closure,
including other evals on the selected Artifact and cycle peers, in configuration
order. `--all` already includes every eval and all no-eval Artifact obligations.
A family name selects every instance, also in
`--artifacts` and `--artifacts-file`; overlapping family/instance entries are
deduplicated without selecting the family template itself.

RED blocks downstream execution; missing/operational evidence waits. Cycle peers
have no internal gates. `--ignore-gates` bypasses execution gates only: final
validation still requires actual GREEN evidence or explicit `basis: true` throughout
the required scope. A basis never waives its dependencies. Selected GREEN results
with missing obligations remain recorded in an INCOMPLETE Run; both text and JSON
output identify unmet obligations. Agent/Human evals require an existing identity
hit until their execution support arrives.

Root `reviewPolicy.dependencyGates` defaults to `green`; `ignore` enables bypass.
The library's `project::VerifyOptions.ignore_gates` can explicitly override either
policy, including `Some(false)` to enforce gates. `--force` marks only explicitly
selected evals for a fresh review, not recursive dependencies; it neither expands
the execution scope nor bypasses gates. Runs record the resolved policy and each
request's force flag. Forced evals never read, join or replace cached results;
dependencies may still reuse their own identity entries.

`verify --max-executions N` sets a nonnegative, shared per-Run executor-start
budget (unlimited when omitted); it is not an Artifact declaration field.
The Run records `jobs`, `maxExecutions` and `executionsStarted`. A prepared
executor invocation consumes one start, even when it fails to spawn or later
returns ERROR. Identity preparation/rechecks, cache hits, and joined waiters
consume none. Preparation failures before invocation consume none. Zero permits
reuse and joining only; a waiter needing to replace a failed/dead owner must use
its own Run's remaining budget. Exhaustion never interrupts running evals, but
leaves remaining READY requests `BUDGET_EXHAUSTED` and ends the Run INCOMPLETE
with a reason (exit 4). Hitting the cap exactly without unmet starts is not an
error. There are no reservations, refunds, admission pools or restart ledger.

Selection files must be regular files no larger than 4 MiB, containing a JSON
string array or one trimmed ID per line (UTF-8 BOM and CRLF are accepted). They
must contain 1–100000 IDs before deduplication. Empty lines are ignored; IDs cannot
contain ASCII whitespace, control characters or commas. Malformed JSON arrays
never fall back to line parsing. Relative file paths resolve from the CLI's cwd,
not `--repo`.

`verify ... --profile NAME` selects a complete declared `profileVariants` entry
for every included eval. Each eval can declare up to 64 safely named variants,
all retaining its default reviewer kind. Unknown variants fail before creating a
Run, and source declarations are never rewritten. The library accepts
`project::selection::ProfileSelection::Named` or `ProfileSelection::Evals` (a
qualified-eval-to-name map); mappings outside the included scope fail. With
`--recursive`, variants also apply to dependency evals. Runtime
variant arguments rebuild scoped references and dependency gates. Stored request
profiles describe the actual execution; `requestedProfile` retains the requested
variant separately when an identity hit returns another profile.

Verification runs in the foreground, with or without `--wait`: GREEN exits 0,
RED 1, ERROR 2, and INCOMPLETE 4. Ctrl-C/SIGTERM cancels every owned process
group, waits for cleanup, and records ERROR/CANCELLED for running and queued
requests, never RED. Previously committed results remain unchanged. There is no
detached worker. `--json` prints
full saved results, including payloads, argv, stdout/stderr and runtime details;
there is no compact projection or `--full` flag. `run show RUN_ID` always prints
full saved JSON and exits 0 on a successful read, regardless of the saved verdict.
It never discovers declarations or runs code, and does not need `--repo`.

One `state.sqlite` holds Runs from every repository (bundled SQLite, WAL, schema 1),
with the canonical repository path recorded on each Run. The state home is
`$ARTIFACTIZE_STATE_HOME`, falling back to `$XDG_STATE_HOME/artifactize` or
`~/.local/state/artifactize`. `--state-dir PATH` moves the whole state, including
private output directories under `PATH/runs`. The database is a fresh format;
there is no migration from earlier receipt layouts. Saved Runs stay readable after
the original repository is removed. State/output inside the reviewed repository
is rejected, including through symlink ancestors; database files and their WAL
sidecars must be regular files. No writer transaction spans a subprocess or async
suspension. Completed identity results are shared within this database; cross-process
claims and polling waiters prevent duplicate identity execution.

Declarations use `evals`, with qualified eval IDs such as `green/check`.
`stale` accepts only `{"kind":"identity","script":{"command":"/bin/sh","args":["identity.sh"]}}`,
optionally with `inputs` and `timeoutMs`; `weight` is rejected. Discovery validates
these fields without opening or executing the script or its inputs. There is no
implicit file-hash or always-stale mode: no identity means no reuse. The old
`critics`, `stale.paths`, `resultCheck`, `envRequirements`,
`reviewPolicy.maxConcurrentExecutors` and tool metadata `observation` fields are
rejected. Agent tools use the flat declarations below; Human tools still use their
existing declaration shape until P4.4.

## Owner identity commands

Before executing any eval, `verify` computes each declared identity in the selected
required dependency closure, including dependencies whose evals are not selected.
Every eval on an Artifact receives the same literal value, also saved on its
request and in the Run's Artifact validation. An identity does not contain any
implicit repository, eval, profile, dependency or content salt.

The command runs from its owner's folder with JSON on stdin:
`{"version":1,"artifactId":"example"}`. Family instances additionally receive
`"family":{"name":"family","material":["input.txt"]}`. Bare commands resolve
through PATH; absolute executables run as given, while relative executable paths
containing `/` (such as `./identity.sh`) must remain inside the owner without
symlinks. Arguments stay literal except explicit scoped Artifact references,
resolved with the same rules as runtime argv. There are no interpreter-specific
flags, wrappers or entry-file rules.

`inputs` accepts up to 64 unique owner-relative literal file/directory paths.
Inputs and family material must exist without symlink traversal on every call;
contents are never hashed. Commands share runtime isolation, cancellation, bounded
raw output and a 30,000 ms default timeout (1–2,147,483,647 ms allowed). Their private
external output, HOME and temporary directories are removed after each invocation.
Stdout must be exactly 1–128 ASCII characters from `[A-Za-z0-9._:-]`, optionally
followed by one LF. It is not trimmed or cleaned. Nonzero exit, malformed output,
timeout, cancellation, missing inputs or cleanup failure abort preparation with
an operational error, without starting any eval or falling back to an uncached
review. Stderr is not forwarded as an identity diagnostic.

After each runtime review exits, its identity is recomputed before accepting a
GREEN or RED verdict. A changed value records ERROR/INPUT_CHANGED with no semantic
result; a failed recheck also records ERROR. Force does not skip preparation or
this recheck. Artifacts without an identity run without either step. There is no
workspace monitoring, hashing or fingerprinting.

## Completed identity reuse

A successful identity recheck publishes either GREEN or RED to `cache_entries`,
pointing to a self-contained `executions` row. Errors and cancellation are never
published. The owner identity alone is the key: repositories, evals, profiles,
schemas and criteria do not partition it. Owners must include any distinction
that makes results noninterchangeable in their identity output.

A hit returns the original result without running the eval or re-validating it
against the requested profile/schema. The request saves the original execution ID,
actual `profile`, `provenance` (repository, Run, request, eval and completion time)
and `usage` when reported, alongside `requestedProfile`. Runtime usage is null,
not an invented zero. Cached RED remains RED for gates and final obligations.
Dependencies outside execution selection can supply cached evidence without
running. Results remain readable after the source repository is deleted; external
paths embedded in result text are not made portable.

No identity means no cache lookup or publication. `--force` bypasses lookup and
publication for explicitly selected evals, leaving any existing entry unchanged.
Forced and uncached results still satisfy their own Run and retain execution audit.

## Status and static graph

```sh
artifactize --repo PROJECT status
artifactize --repo PROJECT status ARTIFACT --recursive --json
artifactize --repo PROJECT graph FAMILY --json
artifactize --repo PROJECT config check --json
```

`status` accepts the same exclusive selectors as `verify`, defaulting to `--all`,
and the same `--profile`, `--recursive`, `--force` and `--ignore-gates` policy
options. It reports current Artifact/eval states, unmet final obligations, and
`execute` / `reuse` / `wait` / `blocked` actions. All evals in the required closure
are shown; `selected` and `included` distinguish explicit selection from recursive
execution. Action counts cover included evals only. Exit 0 means current validation
is satisfied; 1 means obligations remain; invalid input or state errors exit 2.

Status prepares current owner identities for the selected required closure, using
the same isolation and validation as verify. Identity failures exit 2; an old
saved identity is never substituted. It never runs tools or eval commands, creates
Runs, reserves work, creates a missing database or updates cache access times.
Identity commands use disposable output under the state directory, which may be
created even when no database exists. `graph` and `config check` remain fully
static and never run owner code.

A current completed identity entry yields PASS or RED and a `reuse` action when
gates allow. Force still applies only to selected evals. Without a hit, Agent/Human
execution actions remain blocked until execution support arrives. Saved attempts
are read for this canonical repository only: each eval's optional
`last: {runId, verdict, identity?}` is historical, not current evidence. Use
`run show RUN_ID` for full attribution. Noncached GREEN/RED satisfies only its own
Run, so its later status is STALE rather than reuse (ENG-24). Basis-only scopes can
be satisfied; a basis with unmet dependencies is INCOMPLETE.

`graph [ARTIFACT|FAMILY]` defaults to the whole project, or shows the selected
required closure including cycle peers. Text lists Artifacts, evals, families,
components and input-to-consumer relations. Full JSON includes expanded static
Artifact/eval definitions (including profiles, payloads, schemas and tool views),
child/mount/instruction/argv relation metadata, cycle markers, dependency-first
SCCs and family membership. Component IDs refer to the full graph and may be
noncontiguous in a selected projection. `config check` keeps its static validity
confirmation and JSON Artifact/eval counts. There is one text or full JSON output
level, with no `plan`, `--compact` or `--full`.

## Scoped input library

`config::read_workspace_config` (also used by `config check`) resolves nearest
child ownership, validates mounts and explicit references, and returns typed
input-to-consumer `relations` for graph scheduling. Instruction references resolve
an owner's mount alias or a global Artifact name. Backslash-escaped braces, doubled
braces, `${variables}`, nested/JSON groups and unmatched braces stay literal.
Payloads are never changed and references never expand file content.

`scope::eval_scope` admits the target and explicit references plus their child
and mount closure, not the referenced Artifacts' eval instructions.
`Scope::resolve_path` follows logical child/mount paths to canonical Artifact
identities; `Scope::resolve_input` additionally requires existing files/directories
without symlink traversal. Logical paths reject absolute paths, traversal, empty
components, backslashes, colons, controls and lengths above 4096 characters.

Before preparing a runtime command, call `scope::resolve_argv` with the admitted
scope. Explicit `{name}`, `{name}/path` and `--flag={name}/path` operands resolve to
absolute scoped input paths and add dependencies even without instruction
references. Use `{owner}/mount/path` for logical paths starting at the owner.
Other arguments (including escaped references) remain literal, and the command
is never interpolated. `config check` validates reference names and syntax but
does not open runtime operands or execute programs. Input existence and symlink
checks happen during argument preparation. Graph closure and runtime CLI execution
use these same resolvers. The Agent tool registry uses the same admitted scope.

## Agent tools

`views.agentTools` is an explicit safe-name map. Each entry is either a flat
command declaration or a built-in reference; unknown and mixed fields are rejected.
There is no `metadata`, `script`, `resultKinds`, `artifactKind` or `observation`
wrapper on Agent tools.

```json
{
  "views": {
    "agentTools": {
      "inspect": {
        "description": "Inspect a section of {artifactName}.",
        "inputSchema": {
          "type": "object",
          "properties": {"section": {"type": "string"}},
          "required": ["section"],
          "additionalProperties": false
        },
        "protocol": "json",
        "command": "python3",
        "args": ["inspect.py"],
        "timeoutMs": 120000,
        "executionPaths": ["shared/rules.json"]
      },
      "search": {
        "description": "Search {artifactName}.",
        "inputSchema": {
          "type": "object",
          "properties": {"query": {"type": "string"}},
          "required": ["query"],
          "additionalProperties": false
        },
        "protocol": "plain",
        "command": "rg",
        "args": ["--", "{query}", "."]
      },
      "read": {"builtin": "read", "description": "Read {artifactName}."}
    }
  }
}
```

Command fields `description`, `protocol`, `command` and `args` are required.
`inputSchema` defaults to `{"type":"object","additionalProperties":false}`
(empty arguments only). Supplied schemas must have root `type: "object"` and are
compiled by `jsonschema`, without its file/HTTP resolution features. Local `$ref`,
composition and other standard schema features work; there is no custom keyword
subset. Schema defaults never change arguments. Declarations/schemas are capped
at 8 MiB. Calls require a JSON object of at most 64 KiB before any process or output
directory is created. Validation errors show at most five bounded, escaped
instance/schema paths (under 4 KiB), never argument values.

Descriptions must be nonblank, at most 4000 UTF-16 code units, and support only
`{artifactName}` interpolation (the canonical Artifact ID). Built-in references
accept `builtin: "read" | "list" | "glob" | "grep" | "view_image"` and optional
`description`. Only explicitly declared tools are listed. `read`, `list`, `glob`
and `grep` execute in-process without subprocesses or output directories;
`view_image` and image results arrive together in P4.2.

| Built-in | Arguments | Result |
|---|---|---|
| `read` | `{path, offset?, limit?}`; offset is a 1-based line (default 1), limit defaults to 80, max 500 | `lines: [{number, text}]`, `startLine`, nullable `endLine`, `lineCount`, `totalLines` only when EOF is known, `truncated`, nullable `nextOffset` |
| `list` | `{path?, offset?, limit?}`; offset is 0-based (default 0), limit defaults to/max 200 | Sorted `entries: [{name, path, kind, ...}]`, `totalEntries`, `truncated`, nullable `nextOffset` |
| `glob` | `{pattern, path?}` | Up to 200 sorted logical `files`, `truncated` |
| `grep` | `{pattern, path?, glob?, caseInsensitive?, maxResults?}`; case-sensitive by default, maxResults defaults to/max 200 | `matches: [{path, line, text}]` (one per matching line, sorted by path then line), `truncated` |

Paths are relative logical paths from the tool's declaring Artifact, including
children and mount aliases; omitted paths mean its root. They never accept
absolute paths, dot components or symlinks. The registry scope remains the eval's
admitted Artifacts, not other evals' references. Paths are opened read-only through
pinned directory descriptors with no-follow component checks; special files cannot
be read. `list` can report `symlink`/`other` entries, but searches skip them.
Mounts have kind `mount`; family folders have kind `family` and an `instances`
catalog. Listing a family folder directly pages its logical `instance` entries;
reading its physical files requires `<family>/<instance>/<path>`.

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
Search follows logical mounts and family instances without repeating a mount
cycle. Bounds are 10,000 traversed entries, 8 MiB per grep file, 64 MiB searched,
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
      "example": {"path": "/workspace/example", "children": {}, "mounts": {}}
    },
    "executionPaths": {"shared/rules.json": "/workspace/shared/rules.json"}
  },
  "args": {"section": "summary"}
}
```

Scope entries contain canonical physical paths and logical child/mount maps;
family instances also include `family: {name, material}`. `executionPaths` is
omitted when empty. Declared execution paths are up to 64 unique workspace-relative
files/directories, resolved without symlinks, copying, hashing or pinning.
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

For `plain`, only top-level declared `inputSchema.properties` can appear as argv
placeholders. Whole-token `{query}` and embedded `--query={query}` both work.
Strings substitute literally; other JSON values use compact JSON. Missing values
and NUL bytes fail before spawn. `{{` and `}}` escape literal braces. Substitution
is single-pass: values containing braces, quotes, spaces, `$()` or semicolons
remain one literal argv element, never shell code. Plain tools receive empty
stdin. Cleaned, lossily decoded stdout becomes one text block, capped at 64 KiB
with an explicit truncation marker; nonzero exit marks that bounded stdout as a
tool error. Stderr is not included. Commands such as `rg` that exit nonzero for
no matches need an owner wrapper if that should count as successful empty output.

Bare executables use PATH only, never implicit owner or `node_modules/.bin`
lookup. Commands containing `/` resolve from the owner through the scope resolver
(`./tool` is accepted; traversal and symlinks are rejected). Absolute commands run
as given. JSON-protocol argv may use existing scoped Artifact references, but
cannot add Artifacts outside the eval's admitted scope. Plain argv uses only its
schema-property placeholders. Commands themselves are never interpolated.
Command calls use the owner folder as cwd, runtime's PATH/LANG-only inheritance and
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
Agent loop (P5) and MCP/audit integration (P4.5) will call this API; neither is
implemented here.

## Artifact families

A subfolder's `artifactize.json` can declare a static family with
`"family": {"instances": "instances.json"}` or an inline instance-name map.
The family name is reserved, not an Artifact; each of its 1–10000 instances gets
ordinary Artifact and `instance/eval` identities. Instance names must be globally
unique and cannot shadow entries in the shared folder. Families cannot be the
workspace root, contain nested markers, or declare `reviewPolicy`.

An instance accepts `variant`, object `params`, and up to 64 unique existing
owner-relative `material` paths, resolved without symlink traversal. Parameters
merge shallowly: family defaults, then the named family variant, then the instance.
Exact `{"$param":"/pointer"}` objects inside views and evals copy JSON values
using RFC 6901 pointers, including arrays and the empty root pointer. No string
interpolation or parameter substitution occurs in names, mounts, identity hooks,
or basis. Expanded declarations receive normal validation.

All instance scripts use the shared folder as cwd. A parent addresses material as
`<family-folder>/<instance>/<path>`; bypassing the instance is rejected. Instance
material is an ownership declaration, not a sandbox hiding sibling files.
Discovery keeps each instance's family membership and sorted material, without
computing any digest or content fingerprint. Only an explicit identity can become
a reuse key. Identity commands receive each selected instance's family name and
material paths; each review rechecks its own instance identity. No workspace
monitoring or automatic reuse is added.

The runtime-only fixture demonstrates parameterized views, shared evals,
independent inputs/results, and a shared identity hook (inert during discovery):

```sh
cargo run -q -p artifactize -- --repo crates/artifactize/tests/fixtures/families config check
cargo run -q -p artifactize -- --repo crates/artifactize/tests/fixtures/families verify --all
```

## Runtime execution library

`runtime::Command::prepare(program, args, workspace, run_dir, timeout_ms)` prepares
one invocation for `runtime::execute`. Arguments are literal; the runner never adds
a shell. The default cwd is the canonical workspace; a scoped caller can set
`command.cwd` to the resolved Artifact directory. `process::run` remains the
lower-level API for already-resolved commands with a complete explicit environment.
`verify` uses this policy with the resolved owner Artifact as cwd.

Only `PATH` and `LANG` are inherited (`LANG` defaults to `en_US.UTF-8`). Each prepared
runtime command gets a fresh 0700 directory below the caller's external `run_dir`,
with 0700 `output`, `tmp`, `home`, and `cache` subdirectories. Children receive
`ARTIFACTIZE_WORKSPACE_DIR`, `ARTIFACTIZE_OUTPUT_DIR`, `ARTIFACTIZE_TMP_DIR`, private
`HOME` and `XDG_CACHE_HOME`, and `TMPDIR`/`TMP`/`TEMP` pointing at private temporary
storage. Existing output ancestors are canonicalized before creation; output
inside the canonical workspace, including through symlinks, is rejected. The
caller owns the run directory; prepared output persists after execution for
receipts and explicit pruning. Failed preparation removes its newly owned
invocation directory, not the caller's root.

The runtime deadline defaults to 30,000 ms and accepts 1 through 2,147,483,647 ms.
It covers inert-child registration and execution without a reset. Each raw stdout
and stderr stream is capped at 128 KiB while excess bytes are drained. Runtime
results decode UTF-8 lossily, remove ANSI CSI sequences and C0 controls except tab,
LF and CR (DEL is preserved, matching CCDD), and retain the truncation flag and
actual exit status/duration. Low-level process results remain unmodified bytes.

Cancellation and timeout signal the entire owned process group with SIGTERM,
then SIGKILL after at most one second; ordinary leader exit also kills its group.
Capture stops waiting at most one second after cleanup if a pipe remains open.
Dropping the caller cancels the supervisor, which still cleans up and reaps the
leader. Configured programs are trusted local code, **not an OS sandbox**: they
can use ordinary OS access, must keep reviewed input unchanged, and descendants
that deliberately detach into another process group can escape cleanup. These
controls do not promise confinement or detection of every adversarial transient
write.
