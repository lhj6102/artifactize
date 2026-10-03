# Artifactize

A Rust rebrand of [CCDD](https://github.com/lhj6102/ccdd), ported from CCDD 7.0.0 (`cbf28b4`).

Work in progress. See the [plan](docs/PLAN.md) and the [capability inventory](docs/ccdd-7-inventory.md).

## Review monitor

```sh
artifactize monitor                 # Runs for the current repository
artifactize monitor --repo /path/to/repo
artifactize monitor --all           # All repositories in the shared state
artifactize --state-dir /path/to/state monitor --all
```

The ratatui TUI opens `state.sqlite` read-only and refreshes every second. It never
loads repository declarations, executes owner code, starts or cancels reviews,
reconciles dead owners, or changes saved state. Removed repositories remain readable.
Run pages are newest first; progress shows saved state counts, running evals and
elapsed times, waiting Human request IDs, completed durations, errors and waiting
reasons. Failed reads keep the last-known data with an error and last-refresh time.

- `j`/`k` or Up/Down: select a Run or scroll progress; Enter: open progress.
- `n`/`p` or PageDown/PageUp: older/newer Run pages (50 per page); PageDown/PageUp
  scroll within progress.
- `b`, Backspace or Left: return to Runs; Home: first row/top of progress.
- `r`: refresh now; `q`, Esc or Ctrl-C: quit. SIGTERM also restores the terminal.

The monitor requires an interactive terminal and does not support `--json`; use
`run list`/`run show` for noninteractive queries. Human review stays in the CLI.
The Artifact/eval tree and request details are planned next (P7.3/P7.4).

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
output identify unmet obligations. Human evals record WAITING_HUMAN requests;
without a submission, ordinary verify exits INCOMPLETE (4). Use `--wait` to keep
the same Run scheduling after Human submissions.

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
Runtime or Agent invocation consumes one start, even when it fails to spawn or
later returns ERROR. Human waiting, claim and tool actions consume no starts. Identity preparation/rechecks, cache hits, and joined waiters
consume none. Preparation failures before invocation consume none. Zero permits
reuse, joining and Human reviews; a budgeted waiter replacing a failed/dead owner uses
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
RED 1, ERROR 2, Human wait timeout 3, and INCOMPLETE 4. Ctrl-C/SIGTERM cancels every owned process
group, waits for cleanup, and records ERROR/CANCELLED for running and queued
requests, never RED. Previously committed results remain unchanged. There is no
detached worker. `--json` prints
full saved results, including payloads, argv, stdout/stderr and runtime details;
there is no compact projection or `--full` flag. `run show RUN_ID` always prints
full saved JSON and exits 0 on a successful read, regardless of the saved verdict.
It never discovers declarations or runs code, and does not need `--repo`.
Each Run saves its selection, effective policy, requested profile option, and
expanded Artifact/eval definitions for the selected dependency closure, including
family membership, schemas, tool views and graph relations. These are recorded
with the initial request rows, not reconstructed from current declarations.

`run list [--repo-only | --all]` lists saved Runs newest first as a text table, or
as a JSON array with `--json`. It defaults to the canonical `--repo` path (the
current directory when omitted); `--repo-only` makes that default explicit, and
`--all` includes every repository in the shared state. `--limit N` (default 50)
and `--offset N` (default 0) page the filtered results. Each row includes the Run
ID, repository, creation/completion timestamps, status, and request counts by
verdict/state (absent states have zero requests). Reads do not run owner code,
create missing state, or require a repository to still exist. There is no
separate `history` command.

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
rejected. Agent and Human tools use the separate flat declarations below.

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

After each runtime or Agent review completes, its identity is recomputed before accepting a
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

## Cache inspection and limits

```sh
artifactize cache list --json
artifactize cache show IDENTITY
artifactize cache rm IDENTITY
artifactize cache gc
```

These commands use the shared state home or `--state-dir PATH`, without loading a
repository. `list` shows identity, original verdict/repository/eval, retained JSON
bytes and last use (a text table, or a JSON array). `show` always prints the full
saved execution with result, actual profile, provenance and usage; a missing
entry prints `null` and exits 4. Reads neither create missing state nor update
access times. `rm` prints `{"removed":true}` (false if absent), preserving saved
Runs and execution audit. It refuses identities with active executions or waiters.

Publishing a new reusable entry triggers LRU GC: at most 10,000 entries and 1 GiB
of retained execution JSON, with a 16 MiB per-entry limit. Oversized results still
reach their Run and existing waiters through the saved execution, but later calls
execute again. Reuse hits update last use; inspection does not. GC evicts oldest
eligible entries first, using identity to break ties, and skips active executions
and in-flight waiters. Protected entries can temporarily exceed the caps; a later
publication or `cache gc` retries collection. Explicit GC prints removed and
remaining entry/byte counts as JSON. Automatic maintenance failures are reported
on stderr without replacing an already completed verdict.

GC and `rm` remove only reuse mappings, never execution or receipt rows or Run
output. These limits are not a bound on total database size or active scratch
space, and there is no semantic TTL, protected-reader registry or scratch cleanup.

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
gates allow. Force still applies only to selected evals. Human execution actions
record a waiting request; an active Human identity projects WAITING_HUMAN and a
`wait` action, even after the original verifier exits. Saved attempts
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

## Agent reviews

Agent evals use one explicit model and backend, with no bundled catalog, aliases,
credential search or fallback:

```json
{
  "kind": "agent",
  "backend": "openai",
  "model": "YOUR_EXACT_MODEL_ID",
  "reasoning": "high",
  "timeoutMs": 240000
}
```

`backend` accepts `openai`, `anthropic`, `chatgpt` or `claude`. The first two use
only `OPENAI_API_KEY` or `ANTHROPIC_API_KEY`, respectively. ChatGPT uses the stored
Sign in with ChatGPT credentials, never an API-key fallback. The official Claude
CLI backend (P5.6) currently returns an explicit not-yet-implemented ERROR.
`provider` and `effort` are not config aliases.

For an eligible ChatGPT subscription:

```sh
artifactize login chatgpt
artifactize models chatgpt           # account-visible slug and display name, in server order
artifactize models chatgpt --json
# Set backend: "chatgpt" and model to one returned slug, then:
artifactize verify --all
```

Use the same `--state-dir` for login, models and verify if overriding the default.
Each inference attempt obtains a valid access token (refreshing under the auth lock
as needed). rig's ordinary Responses client sends it to `https://api.openai.com/v1`,
never the ChatGPT backend-api. Every request explicitly sets `store:false`, streams,
lifts all system messages into `instructions`, and replays the full history. There
is no `previous_response_id` or server-side conversation state, nor any unsupported
SIWC output cap, sampling, metadata or background parameter. Budgets stay client-side.
Subscription failures retain the provider code and message; authentication failures
explain how to sign in again. A usage-limit failure points to ChatGPT Settings > Usage.
`models chatgpt` uses a fresh GET `/v1/models`, filtering `visibility == "list"` and
printing `slug` and `display_name`. Model listing for other backends remains P8.1.

This contract follows the official SIWC [models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference),
[preview limitations](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)
and [errors and recovery](https://developers.openai.com/siwc/token-sharing-open-source/errors-and-recovery) documentation.

`reasoning` is optional. When present, OpenAI and ChatGPT receive exactly `reasoning.effort`
(`none`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`). Anthropic receives
adaptive thinking and exactly `output_config.effort` (`low`, `medium`, `high`,
`max`). Other values are rejected, never remapped. A model that does not support
the requested setting fails at the provider; artifactize does not substitute a
model or lower effort. If the response reports a model ID, it must match exactly.

The rig-core 0.43.0 adapter uses streaming OpenAI Responses with `store:false`,
encrypted reasoning replay and parallel tool calls disabled, or Anthropic Messages.
The latter requires a per-turn output cap (16384 tokens), separate from the review
budget. One review deadline (default 240 seconds) covers all turns, retries and tools.
Truncated or incomplete responses are ERROR, even if they contain a JSON verdict.
At most two transient retries occur before any output, tool call or positive usage;
authentication and quota failures stop immediately with the provider message.
Nested HTTP retries and redirects are disabled.

Only tools from the eval's scope are exposed. They run sequentially with private
runtime environments. Tool results keep text, JSON and validated base64 image blocks rather than flattening
structured data. PNG/JPEG/WebP results reach both provider wires; unsupported-model
image requests fail with the provider error. Duplicate provider call IDs fail
closed. Tool audit and per-request-attempt `usage` are saved on both success and
ERROR and retained with original execution attribution on cache hits. Counters
are provider-reported (including reported zero), not inferred totals or costs.
Anthropic `inputTokens` is its native uncached input; cache read/write counters are
separate. OpenAI input already includes its cache reads. Never sum every counter.
Unreported fields stay absent. Assistant messages and reasoning are not persisted.

Agent evals share runtime evals' dependency gates, identity claims, reuse and final
identity recheck. Final output must be one strict JSON object containing
`"verdict":"GREEN"` or `"verdict":"RED"` and only the permitted owner-schema fields.
One tools-disabled repair is allowed for invalid final output, within the original
deadline. `maxTokens` and `maxToolCalls` are enforced client-side before further tools
execute; neither becomes a ChatGPT request parameter.

Owner validation before relying on a provider: run one real review with each API-key
backend and an accessible exact model ID. The owner's real ChatGPT review is pending:
sign in, list models, then run an Agent eval with a declared tool using a returned slug.
Automated tests use fake HTTP transports or local HTTP servers and make no real
inference requests.

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
Agent loop and stdio MCP server both call this registry.

## Tool diagnostics and MCP

```sh
artifactize tools check                         # every Agent/Human eval's scope
artifactize tools check app/review              # same as --eval app/review
artifactize tools check --artifact app --audience agent
artifactize tools check --execute --artifact app --audience agent --tool read --args '{"path":"README.md"}'
artifactize tools check --execute --artifact app --audience human --tool inspect
artifactize mcp --manifest /external/execution/mcp-manifest.json
```

`tools check` discovers and validates declarations, resolves executable availability,
scoped argv operands and declared execution paths, and prints JSON scopes, schemas
and per-tool readiness checks. It is static by default: no owner process, identity
hook, database or output directory is created. A positional selector or `--eval`
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

The internal async helper `mcp::write_config(config, effective_eval, execution_id,
state_dir, output_dir)` writes private `mcp-manifest.json` and `mcp-config.json`
files in an external execution directory and returns the config path. Pass the
config to the unmodified Claude CLI with `--strict-mcp-config --mcp-config CFG
--allowedTools 'mcp__artifactize__*'` (full backend controls are in `docs/PLAN.md`).
The generated stdio server uses the current executable's absolute path. P5.6 owns
launching Claude, assistant-turn admission and the tools-disabled repair call.

The manifest contains `executionId`, `evalId`, `repo`, `state`, `output` and the
effective Agent `profile`. The server reloads static declarations and binds that
execution to its eval, scoped definitions and budget in SQLite; reconnects reject
changed bindings instead of widening the scope or resetting counters. No snapshots,
pinned input manifests or provider credentials are involved. One server holds an
execution-directory lock; calls are serialized. Only Agent tools in the eval's
admitted scope are exposed, with dynamic JSON Schemas. rmcp 3.5.0 handles newline-
delimited JSON-RPC initialization (including 2024-11-05), ping, tool listing/calls,
notifications and protocol errors. Each inbound line is capped at 64 KiB before
unbounded buffering; oversized input closes the server with exit 1 and a stderr
diagnostic, including input without a newline. Outgoing image responses retain
the registry's image/result limits, not the inbound cap. Stdout is protocol-only.

Text and JSON blocks become MCP text (JSON is serialized); validated images remain
MCP images. Authored and operational tool failures return `isError: true`.
Cancellation and disconnect propagate to process-group cleanup. Every tool call,
including rejected names/arguments, cancellation and exhausted budgets, is audited
before a response. `mcp_sessions.started` is incremented transactionally before
admitted calls against the effective profile's `maxToolCalls`; denied calls are
audited without running or incrementing. SQLite `tool_calls` rows contain ordered
`name`, `arguments`, bounded `result` summary, `isError` and `error` fields.
Unfinished calls retain an error placeholder after a crash. Sessions may precede
identity-less execution rows; the caller owns review completion. `run show` combines
this durable audit with in-process Agent audit through one projection; execution
completion also copies it into the self-contained execution/cache result.

## Human tools

`views.humanTools` is a separate safe-name map of predefined commands. Each entry
requires exactly `description`, `kind: "launch" | "output"`, `command` and `args`,
with optional `timeoutMs` (1–2147483647). No `inputSchema`, free call arguments,
`protocol`, `executionPaths`, `metadata` or `script` wrappers are accepted.
Descriptions follow the Agent description rules, including `{artifactName}`.

```json
{
  "views": {
    "humanTools": {
      "open": {
        "description": "Open {artifactName} for review.",
        "kind": "launch",
        "command": "code",
        "args": ["{artifactPath}"]
      },
      "show": {
        "description": "Show the review notes for {artifactName}.",
        "kind": "output",
        "command": "cat",
        "args": ["{artifactPath}/notes.txt"],
        "timeoutMs": 120000
      }
    }
  }
}
```

Arguments are fixed literal argv, with scope placeholders only: `{artifactPath}`
is the declaring Artifact's canonical folder; `{name}` names an Artifact or its
owner's mount alias. Both accept `/path` and `--flag=` forms, resolved by the same
logical-path and no-symlink checks as runtime argv. Other brace forms are rejected;
there are no arbitrary string templates or escaped-brace interpolation. Unknown
names and invalid operand syntax are rejected when loading the workspace. Tool
operands never add dependencies or expand the review's admitted scope; an existing
but out-of-scope Artifact, missing input or symlink fails before execution. Input
metadata is checked at each call, not during inert config discovery.

Executable resolution is identical to Agent tools: bare names use PATH, relative
names containing `/` are owner-relative scoped paths (`./tool` is accepted), and
absolute executables run as given. No shell is added; the cwd is the declaring
Artifact's folder. Human commands inherit the reviewer's **complete real
environment**, including HOME, DISPLAY/WAYLAND_DISPLAY, XDG settings and config.
They do not use Agent isolation or create private HOME/TMP/output directories.
Only declare trusted commands: they have the reviewer's ordinary permissions and
environment, including any credentials already present there.

- `launch` starts a new session/process group with stdin/stdout/stderr disconnected
  and returns `{"content":[{"type":"launch","launched":true}],"isError":false}`
  immediately after spawn succeeds. There is no readiness wait or content capture.
  Spawn failure is an error; a later exit (even nonzero) does not undo the handoff.
  The program intentionally survives artifactize exit or later cancellation; the
  reviewer owns its lifetime. `timeoutMs` does not limit the handed-off program.
  A launch is neither a verdict nor proof of observation.
- `output` waits for completion, with a default 120000 ms timeout. Stdout and stderr
  are each captured up to 128 KiB and cleaned like runtime output. Each becomes a
  text block capped at 64 KiB with an explicit truncation marker; nonempty stderr
  has a `stderr:` label. Nonzero/signal exit sets `isError: true`, includes the exit
  status, and retains the bounded text. Timeout, cancellation and dropped calls
  use normal process-group cleanup, not intentional handoff.

Internal callers use `tools::human::Registry::new(&config, "artifact/eval")` for a
Human eval scope or `for_artifact(&config, "artifact")` for its child/mount scope,
then `list()` and `call(name, cancellation).await`. Names are
`<operation>_<artifactId>`, with collision rejection. Agent tools are never listed,
and Agent evals cannot construct a Human eval registry. Listing executes nothing.
Human results use a separate text/launch type; Agent results cannot include launch
blocks. These low-level registry operations do not authorize a claimant; use the
`human` lifecycle API or `request tool` below for recorded requests.
No desktop/project launcher factory, preparation phase, readiness hook
or observation receipt is added.

## Human reviews

READY Human evals persist WAITING_HUMAN and release their job slot. They consume
no `maxExecutions` budget, so even a zero budget admits a Human review. `verify`
without `--wait` exits INCOMPLETE and lists waiting requests; it does not fabricate
a verdict or keep a worker alive. Identity-bearing waiting executions retain their exclusive
identity claim after the verifier exits. Cross-repository followers refer to that
same execution and forward Human actions to its original request and repository.

The internal library exposes asynchronous operations with an open `store::Receipts`:

- `human::claim(receipts, request_id, reviewer)` acquires one reviewer lock.
  Repeating the same reviewer is idempotent; another reviewer is refused.
  `human::default_reviewer()` reads `$USER`. There are no reservations, renewals,
  expiry timers, preparation phases, readiness hooks or alarms.
- `human::run_human_tool(receipts, request_id, reviewer, tool, cancellation)`
  authorizes the claimant, reopens the recorded Artifact/eval scope and declarations,
  and checks the identity before invoking a registered Human tool. The tool takes
  no free arguments and uses the reviewer's real environment. Ordinary tool errors
  are correctable actions, not verdicts. Only tool name and error metadata are saved.
- `human::submit(receipts, request_id, reviewer, result, cancellation)` accepts
  GREEN/RED with fields matching `passSchema`/`failSchema`, using the Agent result
  validator without repair. Invalid or oversized results (over 256000 JSON bytes)
  leave the request waiting for correction. A valid submission re-runs the identity
  command: changed input settles ERROR/INPUT_CHANGED instead of the verdict.
  Settlement rechecks the claimant and waiting state transactionally, so a second
  submission fails. It releases the reviewer lock, completes saved followers, and
  publishes only identity-bearing GREEN/RED results to the cache.

For an identity-bearing Human eval, the next `verify` reuses the submitted result
and runs its dependents. **No identity means no reuse**: submission settles only
that Run, and a later `verify` asks for a new Human review. Continuing no-identity
Human dependents requires keeping the same Run alive with `verify --wait`.

```sh
artifactize verify --all --wait --timeout-ms 600000
# In another terminal, using the same state directory:
artifactize request list [--run RUN_ID] [--json]
artifactize request show REQUEST_ID
artifactize request claim REQUEST_ID [--reviewer NAME]
artifactize request tool REQUEST_ID inspect_child [--reviewer NAME]
artifactize request submit REQUEST_ID --verdict GREEN --fields '{"approved":true}'
# Alternatively: --fields-file /path/to/fields.json
```

Claim, tool and submit default the reviewer to `$USER`; `--reviewer NAME` can
select the same explicit reviewer for each action. Reviewer names are local
cooperative locks, not authenticated identities. Only the claimant can run tools
or submit. Tool names are `<operation>_<artifactId>` and take no free arguments.
Text output prints captured text or a launch notice; `--json` prints the tool
result. A tool failure exits 2 and does not invent a verdict.

Submission requires `--verdict GREEN|RED`. `--fields` and `--fields-file` are
mutually exclusive JSON objects of owner fields (default `{}`), not verdict
wrappers. Files must be regular files; fields and the complete result are bounded
at 256000 bytes. Invalid JSON, schemas, reviewer names or verdicts exit 2 and
leave the request correctable. A successful submission exits 0, even for RED;
it does not start a separate verifier.

`verify --wait` polls saved pending Human request states and resumes newly READY
dependents in the **same Run**, retaining its execution budget and earlier
results. While waiting the Run remains RUNNING. `--timeout-ms` requires `--wait`,
defaults to 600000, and accepts 1–2147483647. The deadline begins when scheduling
starts and is checked when foreground execution is idle with pending Human work;
it never interrupts running evals or cancels Human requests. On timeout the Run
ends INCOMPLETE with `waitTimedOut: true` and exit 3. Claims and submissions remain
available, but no background worker continues dependents. Without an identity,
use a new waiting verify and submit its new request to complete those dependents.
Ctrl-C/SIGTERM exits 2, cleans owned processes, and ends the Run as cancelled;
previously created Human requests remain available. Missing non-Human obligations
without any pending Human request return INCOMPLETE (4) immediately.

`request list` includes all saved requests (waiting, claimed and settled), with
optional `--run` filtering. Text includes request/Run/eval IDs, status and reviewer;
`--json` and `request show` include the full saved audit, current claim (also for
shared-execution followers), source execution and summary. The `definition` field
joins the saved eval and its owning Artifact (including family membership) from
the Run; older Runs without saved definitions return null. These queries need no
repository and run no owner code. `run show` and JSON verify include a Run summary:
status counts, wall time, actual executor starts, attempts, tool counts and usage
reporting completeness. Run totals exclude reused source executions; request
summaries retain source attribution and raw per-provider attempts in `usage`.
Unreported usage is never represented as a known zero token count. There are no
separate summary commands or `--full` mode.

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
