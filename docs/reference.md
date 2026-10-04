# Reference

The detailed reference for artifactize: every command and flag, exact limits,
schemas and protocols, internal library APIs, backend wire details and the state
layout. The [README](../README.md) is the user guide, and [Install](INSTALL.md)
covers setup.

- [Command reference](#command-reference)
- [Runtime CLI](#runtime-cli)
- [State](#state)
- [Declaration validation](#declaration-validation)
- [Content staleKey](#content-stalekey)
- [staleKey scripts](#stalekey-scripts)
- [Completed result reuse](#completed-result-reuse)
- [Cache limits](#cache-limits)
- [Artifact families](#artifact-families)
- [Scoped input library](#scoped-input-library)
- [Runtime execution library](#runtime-execution-library)
- [Agent backends](#agent-backends)
- [Agent tools](#agent-tools)
- [Tool diagnostics and MCP](#tool-diagnostics-and-mcp)
- [Human tools](#human-tools)
- [Human reviews](#human-reviews)
- [Local diagnostics and maintenance](#local-diagnostics-and-maintenance)
- [Review store server](#review-store-server)
- [Remote review store client](#remote-review-store-client)

## Command reference

Every command accepts the common options `--repo PATH` (default: the current
directory), `--state-dir PATH` (default: the state home below) and `--json`, before
or after the subcommand, at most once each; commands that do not read a repository
or state ignore them, except `monitor` and `review` (which reject `--json`).
`SELECTOR` is exactly one of `ARTIFACT`, `--eval ID`, `--evals CSV`,
`--artifacts CSV`, `--evals-file PATH`, `--artifacts-file PATH` or `--all`.
`help [COMMAND]` and `--help` print help; `--version` prints the version.

| Command | Flags | Output | Exit |
|---|---|---|---|
| `verify SELECTOR` | `--profile NAME`, `--recursive`, `--force`, `--ignore-gates`, `--jobs N` (4), `--max-executions N`, `--wait`, `--timeout-ms MS` (600000, needs `--wait`) | text or JSON | outcome |
| `status [SELECTOR]` | `--profile NAME`, `--recursive`, `--force`, `--ignore-gates`; default `--all` | text or JSON | 0 satisfied, 1 not |
| `config check` | | text or JSON | 0 |
| `config graph [ARTIFACT\|FAMILY]` | | text or JSON | 0 |
| `run list` | `--repo-only` \| `--all`, `--limit N` (50), `--offset N` (0) | text or JSON | 0 |
| `run show RUN_ID` | `--wait`, `--timeout-ms MS` (600000, needs `--wait`) | JSON | 0; outcome with `--wait` |
| `request list` | `--run RUN_ID` | text or JSON | 0 |
| `request show ID` | | JSON | 0 |
| `request claim ID` | `--reviewer NAME` (`$USER`) | JSON | 0 |
| `request unclaim ID` | `--reviewer NAME` (`$USER`) | JSON | 0 |
| `request tool ID TOOL` | `--reviewer NAME` | text or JSON | 0; 2 tool error |
| `request submit ID` | `--verdict GREEN\|RED`, `--fields JSON` \| `--fields-file PATH`, `--reviewer NAME` | JSON | 0, also for RED |
| `cache list` | | text or JSON | 0 |
| `cache show STALE_KEY [EVAL_HASH]` | | JSON | 0; 4 missing |
| `cache rm STALE_KEY [EVAL_HASH]` | | JSON | 0 |
| `tools check [EVAL]` | `--eval ID`, `--artifact ID`, `--audience agent\|human`, `--tool NAME`, `--execute`, `--args JSON` | JSON | 0 ready, 1 not |
| `login chatgpt`, `logout chatgpt` | | text or JSON | 0 |
| `remote login URL` | `--share summary\|full` (summary); token on stdin | text or JSON | 0 |
| `remote logout` | | text or JSON | 0 |
| `remote status` | | text or JSON | 0 signed in, 1 not |
| `remote push` | `--dry-run` | text or JSON | 0 |
| `models openai\|anthropic\|chatgpt\|claude` | | text or JSON | 0 |
| `doctor` | | text or JSON | 0 ready, 1 hard error |
| `prune` | `--older-than DURATION`, `--dry-run` | text or JSON | 0 |
| `monitor` | `--all` (not with `--repo`) | terminal UI | 0 |
| `review [REQUEST_ID]` | `--all` (not with `--repo`), `--reviewer NAME` (`$USER`) | terminal UI | 0 |
| `server run` | `--listen ADDR` (`127.0.0.1:8417`) | listening address | 0 |
| `server token add NAME` | `--scopes read,publish,human` | the token, once | 0 |
| `server token list`, `server token revoke NAME` | `--purge` (revoke) | text or JSON | 0 |
| `server rm STALE_KEY [EVAL_HASH]` | | JSON | 0 |

Run outcome codes (`verify`, `run show --wait`): 0 GREEN, 1 RED, 2 ERROR or
cancelled, 3 Human wait timeout, 4 INCOMPLETE. `run show --wait` follows a RUNNING
Run until it finishes; when its own timeout expires first it prints the current
Run, exits 3 and leaves the Run running. Every command exits 2 for usage errors
(unknown, repeated, conflicting or missing options and values) and operational
errors: text on stderr, or `{"error":"..."}` on stdout with `--json`. There is no
`plan`, `history`, `run cancel` or `--full`.

## Runtime CLI

```sh
cargo run -q -p artifactize -- --repo crates/artifactize/tests/fixtures/runtime verify --all
cargo run -q -p artifactize -- run show RUN_ID
```

The fixture intentionally includes GREEN, RED, a timeout ERROR, RED-blocked and
ERROR-waiting dependents, and a two-Artifact cycle. Its overall exit code is 2.

The README covers [selectors, gates, outcomes and output](../README.md#verify-and-runs).

Root `reviewPolicy.dependencyGates` defaults to `green`; `ignore` enables bypass.
The library's `project::VerifyOptions.ignore_gates` can explicitly override either
policy, including `Some(false)` to enforce gates. `--force` marks only explicitly
selected evals for a fresh review, not recursive dependencies; it neither expands
the execution scope nor bypasses gates. Runs record the resolved policy and each
request's force flag. Forced evals never read, join or replace cached results;
dependencies may still reuse their own cache entries.

`verify --max-executions N` sets a nonnegative, shared per-Run executor-start
budget (unlimited when omitted); it is not an Artifact declaration field.
The Run records `jobs`, `maxExecutions` and `executionsStarted`. A prepared
Runtime or Agent invocation consumes one start, even when it fails to spawn or
later returns ERROR. Human waiting, claim and tool actions consume no starts. staleKey preparation/rechecks, cache hits, and joined waiters
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
variant separately when a staleKey hit returns another profile.

## State

One `state.sqlite` holds Runs from every repository (bundled SQLite, WAL, schema 2),
with the canonical repository path recorded on each Run. The state home is
`$ARTIFACTIZE_STATE_HOME`, falling back to `$XDG_STATE_HOME/artifactize` or
`~/.local/state/artifactize`. `--state-dir PATH` moves the whole state, including
private output directories under `PATH/runs`. A schema 1 database is upgraded in
place to schema 2 (the staleKey rename) before any read or write; there is no
migration from earlier receipt layouts. Saved Runs stay readable after
the original repository is removed. State/output inside the reviewed repository
is rejected, including through symlink ancestors; database files and their WAL
sidecars must be regular files. No writer transaction spans a subprocess or async
suspension. Completed results with a staleKey are shared within this database; cross-process
claims and polling waiters prevent duplicate execution for the same staleKey.

## Declaration validation

The README describes the [declaration](../README.md#folder-configuration) and the
two `staleKey` forms.

Discovery validates these fields without opening or executing scripts or inputs.
There is no implicit or always-stale mode: an Artifact without `staleKey` has no
reuse, so every `verify` reviews it again. The former `stale` field is rejected
with a message showing the `staleKey` shape. The old `critics`, `stale.paths`,
`resultCheck`, `envRequirements`, `reviewPolicy.maxConcurrentExecutors` and tool
metadata `observation` fields are rejected. Agent and Human tools use the separate flat
declarations below.

## Content staleKey

The README describes [`inputs`, `dependencies` and `ignore`](../README.md#content-stalekey).

Each dependency contributes its own entry, never its dependencies' entries:

- A staleKey script contributes its output.
- Any other Artifact contributes the SHA-256 of its own content inputs: its
  declared `inputs`/`ignore`, or `["."]` when it declares no `staleKey`.

`transitive` therefore lists every Artifact in the closure explicitly. Cycles
terminate, and SCC peers appear as ordinary dependencies; the owner never lists
itself. A family instance hashes the shared folder without any instance's
material, plus its own material. Editing one instance's material re-reviews only
that instance.

Walks follow the scoped path rules. Symlinks and special files fail closed unless
they are ignored, nothing is followed out of the owner, and a walk stops with an
error after 10,000 entries or 1 GiB. Hashing runs on preparation and on each
end-of-review recheck. Files a review writes into ignored paths, such as Python's
`__pycache__`, therefore never cause `INPUT_CHANGED`. A content staleKey records a
manifest with the execution: per-file digests (16 hex digits), the inputs digest
and each dependency's entry. Maps that would exceed 64 KiB are dropped from the
manifest but still covered by the staleKey. `status` diffs this manifest against
the current one.

## staleKey scripts

Before executing any eval, `verify` computes each declared staleKey in the selected
required dependency closure, including dependencies whose evals are not selected.
Every eval on an Artifact receives the same literal value, also saved on its
request and in the Run's Artifact validation. A script staleKey does not contain
any implicit repository, eval, profile, dependency or content salt. A content
staleKey failure (a missing input, a link, a limit) aborts preparation the same way.

The command runs from its owner's folder with JSON on stdin:
`{"version":1,"artifactId":"example"}`. Family instances additionally receive
`"family":{"name":"family","material":["input.txt"]}`. Bare commands resolve
through PATH; absolute executables run as given, while relative executable paths
containing `/` (such as `./stale_key.sh`) must remain inside the owner without
symlinks. Arguments stay literal except explicit scoped Artifact references,
resolved with the same rules as runtime argv: `{name}` may be a mount alias or a
global Artifact name, and each referenced Artifact (with its child and mount
closure) is admitted to the script's scope without becoming a graph relation.
`config check` rejects unknown, family and malformed references, and paths are
checked for existence and symlinks when the command is prepared. There are no
interpreter-specific flags, wrappers or entry-file rules.

`inputs` accepts up to 64 unique owner-relative literal file/directory paths.
Inputs and family material must exist without symlink traversal on every call;
contents are never hashed. Commands share runtime isolation, cancellation, bounded
raw output and a 30,000 ms default timeout (1–2,147,483,647 ms allowed). Their private
external output, HOME and temporary directories are removed after each invocation.
Stdout must be exactly 1–128 ASCII characters from `[A-Za-z0-9._:-]`, optionally
followed by one LF. It is not trimmed or cleaned. Nonzero exit, malformed output,
timeout, cancellation, missing inputs or cleanup failure abort preparation with
an operational error, without starting any eval or falling back to an uncached
review. Stderr is not forwarded as a staleKey diagnostic.

After each runtime or Agent review completes, its staleKey is recomputed before accepting a
GREEN or RED verdict. A changed value records ERROR/INPUT_CHANGED with no semantic
result; a failed recheck also records ERROR. Force does not skip preparation or
this recheck. Artifacts without a staleKey run without either step. There is no
workspace monitoring: content staleKeys hash files only at preparation and recheck.

## Completed result reuse

A successful staleKey recheck publishes either GREEN or RED to `cache_entries`,
pointing to a self-contained `executions` row. Errors and cancellation are never
published. The key is **(staleKey, Eval definition hash)**, intentionally
departing from CCDD, whose key was the staleKey alone. The definition hash is lowercase SHA-256
of canonical JSON with recursively sorted keys, containing the effective
`profile`, `payload` (including instruction), `passSchema` and `failSchema`.
The profile is the selected variant's full definition when `--profile` is used:
runtime command/args/timeout, Agent backend/model/reasoning/budgets/timeout, or
Human. Eval id/title, repository paths and unused profile variants are excluded.
Equal definitions still share across evals and repositories; changing criteria,
schema, args or effective profile requires a separate execution.

A script staleKey hashes no script or material file contents. Owners must still
encode input, script and material changes that invalidate results in its output.

A hit returns the original result without running the eval or re-validating it
against the requested profile/schema. The request saves the original execution ID,
actual `profile`, `evalDefHash`, `provenance` (repository, Run, request, eval,
definition hash and completion time)
and the original attempts as `reusedUsage` when reported, alongside
`requestedProfile`. Its own `usage` is null: a reused request spent nothing.
Runtime usage is null, not an invented zero. Cached RED remains RED for gates and final obligations.
Dependencies outside execution selection can supply cached evidence without
running. Results remain readable after the source repository is deleted; external
paths embedded in result text are not made portable.

No staleKey means no cache lookup or publication. `--force` bypasses lookup and
publication for explicitly selected evals, leaving any existing entry unchanged.
Forced and uncached results still satisfy their own Run and retain execution audit.

## Cache limits

Publishing a new reusable entry triggers LRU GC: at most 10,000 entries and 1 GiB
of retained execution JSON, with a 16 MiB per-entry limit. Oversized results still
reach their Run and existing waiters through the saved execution, but later calls
execute again. Reuse hits update last use; inspection does not. GC evicts oldest
eligible entries first, using staleKey then definition hash to break ties, and skips active executions
and in-flight waiters. Protected entries can temporarily exceed the caps, and
automatic maintenance failures are reported on stderr without replacing an already
completed verdict; the next publication retries collection.

GC and `rm` remove only reuse mappings, never execution or receipt rows or Run
output. These limits are not a bound on total database size or active scratch
space, and there is no semantic TTL, protected-reader registry or scratch cleanup.

## Artifact families

A subfolder's `artifactize.json` can declare a static family with
`"family": {"instances": "instances.json"}` or an inline instance-name map.
The family name is reserved, not an Artifact; each of its 1–10000 instances gets
ordinary Artifact and `instance/eval` ids. Instance names must be globally
unique and cannot shadow entries in the shared folder. Families cannot be the
workspace root, contain nested markers, or declare `reviewPolicy`.

An instance accepts `variant`, object `params`, and up to 64 unique existing
owner-relative `material` paths, resolved without symlink traversal. Parameters
merge shallowly: family defaults, then the named family variant, then the instance.
Exact `{"$param":"/pointer"}` objects inside views and evals copy JSON values
using RFC 6901 pointers, including arrays and the empty root pointer. No string
interpolation or parameter substitution occurs in names, mounts, staleKey scripts,
or basis. Expanded declarations receive normal validation.

All instance scripts use the shared folder as cwd. A parent addresses material as
`<family-folder>/<instance>/<path>`; bypassing the instance is rejected. Instance
material is an ownership declaration, not a sandbox hiding sibling files.
Discovery keeps each instance's family membership and sorted material, without
computing any digest or content fingerprint. Only a declared `staleKey` can
become a reuse key. staleKey scripts receive each selected instance's family name
and material paths. A content staleKey hashes the shared folder without any
instance's material, plus the instance's own material. Each review rechecks its
own instance's staleKey. No workspace monitoring or automatic reuse is added.

The runtime-only fixture demonstrates parameterized views, shared evals,
independent inputs/results, and a shared staleKey script (inert during discovery):

```sh
cargo run -q -p artifactize -- --repo crates/artifactize/tests/fixtures/families config check
cargo run -q -p artifactize -- --repo crates/artifactize/tests/fixtures/families verify --all
```

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
ids; `Scope::resolve_input` additionally requires existing files/directories
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

## Agent backends

The README shows the [Agent profile](../README.md#agent-reviews), the backends,
ChatGPT sign-in and `reasoning`. For the `chatgpt` backend:

Each inference attempt obtains a valid access token (refreshing under the auth lock
as needed). rig's ordinary Responses client sends it to `https://api.openai.com/v1`,
never the ChatGPT backend-api. Every request explicitly sets `store:false`, streams,
lifts all system messages into `instructions`, and replays the full history. There
is no `previous_response_id` or server-side conversation state, nor any unsupported
SIWC output cap, sampling, metadata or background parameter. Budgets stay client-side.
Subscription failures retain the provider code and message; authentication failures
explain how to sign in again. A usage-limit failure points to ChatGPT Settings > Usage.
`models chatgpt` uses a fresh GET `/v1/models`, filtering `visibility == "list"` and
printing `slug` and `display_name`. All backends use the same JSON envelope described
under [Local diagnostics and maintenance](#local-diagnostics-and-maintenance).

This contract follows the official SIWC [models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference),
[preview limitations](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)
and [errors and recovery](https://developers.openai.com/siwc/token-sharing-open-source/errors-and-recovery) documentation.

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
ERROR; on cache hits the tool audit is retained with original execution attribution
and the attempts move to `reusedUsage`, never counted as spent. Counters
are provider-reported (including reported zero), not inferred totals or costs.
Anthropic `inputTokens` is its native uncached input; cache read/write counters are
separate. OpenAI input already includes its cache reads. Never sum every counter.
Unreported fields stay absent. Assistant messages and reasoning are not persisted.

Owner validation before relying on a provider: run one real review per backend
(OpenAI key, Anthropic key, ChatGPT sign-in, Claude CLI) with an accessible exact
model ID and a declared tool. These real reviews are still pending for the owner.
Automated tests use fake HTTP transports or local HTTP servers and make no real
inference requests.

## Agent tools

The README shows an [Agent tool declaration](../README.md#agent-tools).

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
`view_image` applies the same image checks as `json` image results.

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
```

`tools check` discovers and validates declarations, resolves executable availability,
scoped argv operands and declared execution paths, and prints JSON scopes, schemas
and per-tool readiness checks. It is static by default: no owner process, staleKey
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

The internal async helper `mcp::write_config(config, effective_eval, execution_id,
state_dir, output_dir)` writes private `mcp-manifest.json` and `mcp-config.json`
files in an external execution directory and returns the config path. Pass the
config to the unmodified Claude CLI with `--strict-mcp-config --mcp-config CFG
--allowedTools 'mcp__artifactize__*'` (full backend controls are in `docs/PLAN.md`).
The generated stdio server is the internal `artifactize mcp --manifest PATH`
command at the current executable's absolute path. It is hidden from `--help` and
not meant to be run by hand: it speaks only MCP on stdio, rejects `--repo`,
`--state-dir` and `--json`, and exits 0, or 1 on a server failure. The Claude
backend launches the CLI with this config and runs the tools-disabled repair as a
second invocation with an empty MCP config.

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
the execution rows of reviews without a staleKey; the caller owns review completion. `run show` combines
this durable audit with in-process Agent audit through one projection; execution
completion also copies it into the self-contained execution/cache result.

## Human tools

The README shows a [Human tool declaration](../README.md#human-tools).

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
then `list()`, `command(name)` (the resolved program, argv and directory, without
running it) and `call(name, cancellation).await`. Names are
`<operation>_<artifactId>`, with collision rejection. Agent tools are never listed,
and Agent evals cannot construct a Human eval registry. Listing executes nothing.
Human results use a separate text/launch type; Agent results cannot include launch
blocks. These low-level registry operations do not authorize a claimant; use the
`human` lifecycle API, `request tool` or `review` below for recorded requests.
No desktop/project launcher factory, preparation phase, readiness hook
or observation receipt is added.

## Human reviews

The README covers the [Human review flow](../README.md#human-reviews) and the
[review](../README.md#review) terminal UI.

The internal library exposes asynchronous operations with an open `store::Receipts`:

- `human::claim(receipts, request_id, reviewer)` acquires one reviewer lock.
  Repeating the same reviewer is idempotent; another reviewer is refused.
  `human::default_reviewer()` reads `$USER`. There are no reservations, renewals,
  expiry timers, preparation phases, readiness hooks or alarms.
- `human::unclaim(receipts, request_id, reviewer)` releases that lock without a
  verdict, so another reviewer can claim the request. Only the claimant can release,
  and only while the request still waits; tool calls already recorded are kept.
- `human::run_human_tool(receipts, request_id, reviewer, tool, cancellation)`
  authorizes the claimant, reopens the recorded Artifact/eval scope and declarations,
  and checks the staleKey before invoking a registered Human tool. The tool takes
  no free arguments and uses the reviewer's real environment. Ordinary tool errors
  are correctable actions, not verdicts. Only tool name and error metadata are saved.
- `human::tool_command(receipts, request_id, tool)` resolves what that tool would
  run (repository, program, argv and directory) without claiming or running it.
- `human::submit(receipts, request_id, reviewer, result, cancellation)` accepts
  GREEN/RED with fields matching `passSchema`/`failSchema`, using the Agent result
  validator without repair. Invalid or oversized results (over 256000 JSON bytes)
  leave the request waiting for correction; schema errors list up to five failing
  instance paths. A valid submission recomputes the
  staleKey: a changed value settles ERROR/INPUT_CHANGED instead of the verdict.
  Settlement rechecks the claimant and waiting state transactionally, so a second
  submission fails. It releases the reviewer lock, completes saved followers, and
  publishes only GREEN/RED results with a staleKey to the cache.

The `artifactize review` terminal UI
calls `human::claim`, `run_human_tool`, `unclaim` and `submit` in-process, and
publishes a submission to the remote review store exactly like `request submit`.

`request list` includes all saved requests (waiting, claimed and settled), with
optional `--run` filtering. Text includes request/Run/eval IDs, status and reviewer;
`--json` and `request show` include the full saved audit, current claim (also for
shared-execution followers), source execution and summary. The `definition` field
joins the saved eval and its owning Artifact (including family membership) from
the Run; older Runs without saved definitions return null. These queries need no
repository and run no owner code. `run show` and JSON verify include a Run summary:
status counts, executed and reused requests by reviewer kind, wall time, actual
executor starts, attempts, tool counts and usage reporting completeness. Run totals
exclude reused source executions; the Run-level `usage.saved` sums their original
counters. Reused requests keep source attribution and the raw per-provider attempts
in `reusedUsage`; their own summaries report no attempts or tool calls (`usageState: "none"`).
Unreported usage is never represented as a known zero token count. There are no
separate summary commands or `--full` mode.

## Local diagnostics and maintenance

The README lists the [commands](../README.md#local-diagnostics-and-maintenance).

`doctor` makes no provider calls and creates no Run, verdict, cache entry or auth
lock. It reports the resolved state directory and tests writability with a temporary
directory, removed immediately (in the nearest existing ancestor when state does
not yet exist). `--repo` additionally runs the same static validation as `config
check`. API keys are reported only as present/absent, never validated or printed.
ChatGPT login presence and Unix-second access-token expiry come from protected local
storage without refreshing. `claude` is located on PATH and only `--version` runs,
with a five-second timeout and bounded output; Claude credentials are never read.
Missing keys/login/binary and expired tokens are warnings: optional backends need
not all be installed. The remote review store check is also offline: it reports
the resolved URL, share level and token source; a missing token is a warning, and
an invalid configuration (including plain HTTP to a non-loopback host) or an
unsafe token file is a hard error. Invalid config, unsafe/unwritable state, invalid auth storage
or a failing installed CLI are hard errors. Exit is 0 without hard errors, 1 with
hard errors, and 2 for invocation errors.

`models openai` and `models anthropic` call the provider's models endpoint with the
corresponding API key through rig; Anthropic pagination is followed. ChatGPT uses
its existing login/refresh flow. There is no account or backend fallback. Claude
has no listing API, so its command explains `--model` names/aliases without launching
inference. Text lists tab-separated slug/name pairs; JSON is consistently
`{"backend":"openai","models":[{"slug":"model-id","display_name":"model-id"}]}`,
with a `note` and empty `models` for Claude. Listings preserve provider order.

Only known scratch directories below `state/runs/<run-id>` are removed: runtime
`output`/`tmp`/`home`/`cache`, leftover tool output and Claude invocation directories.
Run roots, unknown files/directories, database rows, tool audit, results and cache
entries remain. Symlinks (including nested links), non-directory targets and
repository content are refused before deletion. Database reads have a five-second
busy timeout and finish before deletion; prune holds no writer lock. This is plain
prune, without quarantine, crash-recovery machinery or hostile filesystem-race
protection. Saved `run show` and `request show` remain readable after pruning.

## Review store server

```sh
artifactize server token add alice-laptop --scopes read,publish
artifactize server token add ci --scopes read
artifactize server run [--listen 127.0.0.1:8417]
artifactize server token list
artifactize server token revoke alice-laptop [--purge]
artifactize server rm STALE_KEY [EVAL_HASH]
```

`artifactize server` keeps a [shared remote review store](design/remote-store.md)
in its own `review-store.sqlite` under `--state-dir`, separate from `state.sqlite`.
It holds one immutable record per (Eval definition hash, staleKey); the first
writer wins. `server run` serves plain HTTP on loopback by default (it warns when
bound elsewhere) and stops on Ctrl-C/SIGTERM; put a TLS proxy or tunnel in front,
because clients require HTTPS except on loopback. Token commands work on the same
file while the server runs, and take effect immediately.

`token add` prints a random bearer token once; the store keeps only its SHA-256.
Scopes are `read` (look up), `publish` (publish runtime and Agent records) and
`human` (additionally publish Human sign-offs, together with `publish`). Give
untrusted CI `read` only. `token list` shows names, scopes and creation/revocation
times, never tokens. `token revoke NAME` rejects the token at once; `--purge`
also deletes every entry it published, and may be repeated later. Names of revoked
tokens are never reused. `rm` deletes a staleKey's entry, requiring the hash when
the staleKey has several definitions.

The API takes `Authorization: Bearer TOKEN` and answers JSON (`{"error":...}` on
failure; 401 for a missing, unknown or revoked token, 403 for a missing scope):

| Route | Scope | Result |
|---|---|---|
| `GET /v1/whoami` | any | `{"principal":NAME,"scopes":[...]}` |
| `POST /v1/lookup` with `{"keys":[{"staleKey","evalDefHash"}]}` (at most 1000) | `read` | `{"entries":[record,...]}` for the found keys |
| `PUT /v1/entries/{evalDefHash}/{staleKey}` with a record | `publish` (+ `human` for Human records) | 201 `{"created":true}`, or 200 `{"created":false}` when the key exists |

The server checks a record's envelope (schema 1, the path's staleKey and hash,
a GREEN/RED verdict and a profile kind) and size: 256 KiB for a summary, 16 MiB
for a full record carrying `execution`. It stamps `publisher` (the token name) and
`publishedAt` (server clock), replacing any client values. Lookups update last
use; inserts evict least-recently-used entries above 100,000 entries or 4 GiB.

## Remote review store client

```sh
printf '%s\n' "$TOKEN" | artifactize remote login https://reviews.example/ [--share full]
artifactize remote status [--json]
artifactize remote push [--dry-run] [--json]
artifactize remote logout
```

A client is configured only through the state directory and the environment,
never `artifactize.json`, so a cloned repository cannot redirect a token. `remote
login URL` reads one token line from stdin (never argv; a terminal does not echo
it), verifies it with `GET /v1/whoami`, then stores it in
`$STATE/auth/remote-token.json` (0700 directory,
0600 single-link file, like the ChatGPT credentials) and writes
`$STATE/remote.json`: `{"url":"https://reviews.example/","share":"summary"}`.
URLs must use HTTPS with the system trust store; plain `http://` is accepted only
for `localhost`, `127.0.0.0/8` and `[::1]`. Credentials, queries and fragments are
rejected. A stored token is bound to the origin it was issued for.

`ARTIFACTIZE_REMOTE` (a URL, or `off`), `ARTIFACTIZE_REMOTE_TOKEN` (for CI) and
`ARTIFACTIZE_REMOTE_SHARE` (`summary` or `full`) override `remote.json` and the
stored token. `remote status` prints the URL, share level, token source
(`env`, `file` or `none`), reachability, principal and scopes; it exits 0 when the
store accepts the token. Requests use a 2 s connect and 5 s total timeout and never
follow redirects. `remote logout` deletes the stored token and `remote.json`; the
server keeps the token valid until `server token revoke`. Tokens are never printed
or logged.

With a store configured, `verify` reads and writes through it. It checks the local
cache first and looks up the keys that missed in one batched `POST /v1/lookup`
before the Run claims anything. A hit is mirrored into the local cache (a
self-contained `remote-<executionId>` execution with an `origin`) and reused like a
local entry, so `run show`, `cache`, GC, the monitor and the Run summary count it as
reuse. Each key is looked up again just before a local claim and on each
`verify --wait` poll, at most once per second. A remote result for a key that waits
for a Human settles the waiting requests; their never-reviewed waiting execution
becomes ERROR (`SUPERSEDED`). Text output names the source:

```text
  app/check [run-Hq2b9X-1]: GREEN (reused from remote: alice@laptop, run-x05qFq)
  brand/signoff [run-Hq2b9X-2]: GREEN (reused from remote: Human sign-off by alice, published by alice-signoff, run-Ksl1Qr)
```

The producer (`user@host`) and the Human reviewer are what the publishing machine
recorded; the publisher is the server-authenticated token name. Once the local settle
publishes a GREEN/RED with a staleKey to the local cache (after the staleKey
recheck), verify sends its summary record, or the full record with share `full`,
outside any database transaction. `request submit` publishes Human sign-offs.
A token without `read` looks nothing up and one without `publish` publishes nothing,
so a read-only CI token only reuses. Human sign-offs also need `human`; with any
other token they stay local with a warning. Nothing is published for evals without a
staleKey, and `--force` makes no remote calls at all. `status` looks up read-only,
without mirroring, so its `reuse` prediction includes remote results.

`remote push` sends the local GREEN/RED results the store lacks: results produced
while it was unreachable or before `remote login`. It never sends mirrors. A
token with `read` first looks up which keys the store already has, and those are
not sent again. A key published in the meantime answers `created: false`. Both
count as `existing`, so pushing twice is harmless. Human sign-offs without the
`human` scope, and records over their size limit, are `skipped` with a reason on
stderr. `--dry-run` sends nothing and reports what would be pushed. The output is
`Pushed N, already in the store M, skipped K.`, or JSON
`{"dryRun":false,"pushed":N,"existing":M,"skipped":K}`. Unlike `verify`, `push`
fails on any remote failure, and it needs the `publish` scope.

Connection errors, timeouts and 5xx answers fail open: one warning, then the process
reviews locally without the remote. 401/403, TLS failures, other error answers and
invalid configuration fail closed (exit 2). A lookup that fails before the Run
starts leaves no Run; a later failure records the Run as ERROR. A record that does
not validate is ignored with a warning, and its eval is reviewed locally.
