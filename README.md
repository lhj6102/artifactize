# Artifactize

A lean Rust port and rebrand of [CCDD](https://github.com/lhj6102/ccdd), from CCDD 7.0.0 (`cbf28b4`).

artifactize 0.1.0 is complete. Folders declare Artifacts and their evals in static
`artifactize.json` files. artifactize builds the dependency graph and runs runtime,
Agent (OpenAI or Anthropic API key, ChatGPT sign-in, or the Claude CLI) and Human
evals, reusing GREEN/RED results by explicit identity. The CLI drives reviews and
`artifactize monitor` shows their progress. It runs on Linux and WSL.

Every non-DROP item in the [CCDD 7.0 capability inventory](docs/ccdd-7-inventory.md)
is checked off; the [plan](docs/PLAN.md) records the lean scope and what was dropped.
Start with [Install](docs/INSTALL.md): prerequisites, backend setup, a 5-minute
quick start, the monitor and cleanup.

## Examples

Install the binary as described in [Install](docs/INSTALL.md), then try the
example projects. Each README lists the exact commands:

- [Runtime relations](examples/runtime-relations/README.md): runtime evals over
  parent/child folders, a mount alias, `{artifact}` references in instructions
  and argv, a basis Artifact, a RED-able check and identity reuse.
- [Agent tools](examples/agent-tools/README.md): an Agent eval using the built-in
  `read`, `grep` and `view_image` tools, a declared `plain` tool and a declared `json` tool,
  pass/fail schemas, backend and model selection, and a Human sign-off with
  `launch` and `output` tools.
- [Family](examples/family/README.md): one family declaration with an instance
  list, parameters and variants, shared and per-instance material, and family
  selectors.

## Command reference

Every command accepts the common options `--repo PATH` (default: the current
directory), `--state-dir PATH` (default: the state home below) and `--json`, before
or after the subcommand, at most once each; commands that do not read a repository
or state ignore them, except `mcp` (which rejects all three) and `monitor` (which
rejects `--json`). `SELECTOR` is exactly one of `ARTIFACT`, `--eval ID`,
`--evals CSV`, `--artifacts CSV`, `--evals-file PATH`, `--artifacts-file PATH` or
`--all`. `help [COMMAND]` and `--help` print help; `--version` prints the version.

| Command | Flags | Output | Exit |
|---|---|---|---|
| `verify SELECTOR` | `--profile NAME`, `--recursive`, `--force`, `--ignore-gates`, `--jobs N` (4), `--max-executions N`, `--wait`, `--timeout-ms MS` (600000, needs `--wait`) | text or JSON | outcome |
| `status [SELECTOR]` | `--profile NAME`, `--recursive`, `--force`, `--ignore-gates`; default `--all` | text or JSON | 0 satisfied, 1 not |
| `graph [ARTIFACT\|FAMILY]` | | text or JSON | 0 |
| `config check` | | text or JSON | 0 |
| `run list` | `--repo-only` \| `--all`, `--limit N` (50), `--offset N` (0) | text or JSON | 0 |
| `run show RUN_ID` | `--wait`, `--timeout-ms MS` (600000, needs `--wait`) | JSON | 0; outcome with `--wait` |
| `request list` | `--run RUN_ID` | text or JSON | 0 |
| `request show ID` | | JSON | 0 |
| `request claim ID` | `--reviewer NAME` (`$USER`) | JSON | 0 |
| `request tool ID TOOL` | `--reviewer NAME` | text or JSON | 0; 2 tool error |
| `request submit ID` | `--verdict GREEN\|RED`, `--fields JSON` \| `--fields-file PATH`, `--reviewer NAME` | JSON | 0, also for RED |
| `cache list` | | text or JSON | 0 |
| `cache show IDENTITY [EVAL_HASH]` | | JSON | 0; 4 missing |
| `cache rm IDENTITY [EVAL_HASH]` | | JSON | 0 |
| `cache gc` | | JSON | 0 |
| `tools check [EVAL]` | `--eval ID`, `--artifact ID`, `--audience agent\|human`, `--tool NAME`, `--execute`, `--args JSON` | JSON | 0 ready, 1 not |
| `mcp --manifest PATH` | | stdio MCP | 0; 1 server failure |
| `login chatgpt`, `logout chatgpt` | | text or JSON | 0 |
| `models openai\|anthropic\|chatgpt\|claude` | | text or JSON | 0 |
| `doctor` | | text or JSON | 0 ready, 1 hard error |
| `prune` | `--older-than DURATION`, `--dry-run` | text or JSON | 0 |
| `monitor` | `--all` (not with `--repo`) | terminal UI | 0 |
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
detached worker.

Text output prints one line per request and marks a result taken from another
request's execution (an identity hit, or a joined execution) with its source Run:

```text
  web/tests [run-x05qFq-5]: GREEN
  docs/review [run-x05qFq-3]: GREEN (reused from run-Ksl1Qr)
Summary: executed 1 (runtime 1, agent 0, human 0), reused 4 (runtime 2, agent 1, human 1)
Usage: spent none; saved inputTokens 10, outputTokens 5, totalTokens 20
```

`executed` counts the Run's own executions, including ERROR results and Human
requests it recorded; `reused` counts the rest that have a source. `Usage` (printed
only when some usage was reported) separates counters spent in this Run from
`saved`, the sum of the reused executions' original counters. JSON verify and
`run show` carry the same numbers as `summary.executed`, `summary.reused` and the
Run-level `usage: {spent, saved}`; `summary.usage` stays equal to `usage.spent`.

`--json` prints
full saved results, including payloads, argv, stdout/stderr and runtime details;
there is no compact projection or `--full` flag. `run show RUN_ID` always prints
full saved JSON and exits 0 on a successful read, regardless of the saved verdict;
`--wait` instead exits with the Run outcome code once it finishes.
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
resolved with the same rules as runtime argv: `{name}` may be a mount alias or a
global Artifact name, and each referenced Artifact (with its child and mount
closure) is admitted to the identity's scope without becoming a graph relation.
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
review. Stderr is not forwarded as an identity diagnostic.

After each runtime or Agent review completes, its identity is recomputed before accepting a
GREEN or RED verdict. A changed value records ERROR/INPUT_CHANGED with no semantic
result; a failed recheck also records ERROR. Force does not skip preparation or
this recheck. Artifacts without an identity run without either step. There is no
workspace monitoring or file-content fingerprinting.

## Completed identity reuse

A successful identity recheck publishes either GREEN or RED to `cache_entries`,
pointing to a self-contained `executions` row. Errors and cancellation are never
published. The key is **(owner identity, Eval definition hash)**, intentionally
departing from CCDD's identity-only key. The definition hash is lowercase SHA-256
of canonical JSON with recursively sorted keys, containing the effective
`profile`, `payload` (including instruction), `passSchema` and `failSchema`.
The profile is the selected variant's full definition when `--profile` is used:
runtime command/args/timeout, Agent backend/model/reasoning/budgets/timeout, or
Human. Eval id/title, repository paths and unused profile variants are excluded.
Equal definitions still share across evals and repositories; changing criteria,
schema, args or effective profile requires a separate execution.

No script or material file contents are hashed. Owners must still encode input,
script and material changes that invalidate results in their identity output.

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

No identity means no cache lookup or publication. `--force` bypasses lookup and
publication for explicitly selected evals, leaving any existing entry unchanged.
Forced and uncached results still satisfy their own Run and retain execution audit.

## Cache inspection and limits

```sh
artifactize cache list --json
artifactize cache show IDENTITY [EVAL_HASH]
artifactize cache rm IDENTITY [EVAL_HASH]
artifactize cache gc
```

These commands use the shared state home or `--state-dir PATH`, without loading a
repository. `list` shows identity, Eval definition hash, original verdict/repository/eval, retained JSON
bytes and last use (a text table, or a JSON array). `show` always prints the full
saved execution with result, actual profile, provenance, usage and `producer`
(`user@host` and artifactize version; submitted Human results also record their
`reviewer`); a missing entry prints `null` and exits 4. Entries mirrored from a
[shared remote review store](docs/design/remote-store.md) carry an `origin`
(store, publisher, publication time), which `list` shows in place of the repository. Reads neither create missing state nor update
access times. `rm` prints `{"removed":true}` (false if absent), preserving saved
Runs and execution audit. For `show` and `rm`, the hash may be omitted when the
identity has only one entry; multiple definitions require the full hash from
`cache list`. `rm` refuses a key with active executions or waiters (without a hash,
any active definition for that identity prevents removal).

Publishing a new reusable entry triggers LRU GC: at most 10,000 entries and 1 GiB
of retained execution JSON, with a 16 MiB per-entry limit. Oversized results still
reach their Run and existing waiters through the saved execution, but later calls
execute again. Reuse hits update last use; inspection does not. GC evicts oldest
eligible entries first, using identity then definition hash to break ties, and skips active executions
and in-flight waiters. Protected entries can temporarily exceed the caps; a later
publication or `cache gc` retries collection. Explicit GC prints removed and
remaining entry/byte counts as JSON. Automatic maintenance failures are reported
on stderr without replacing an already completed verdict.

GC and `rm` remove only reuse mappings, never execution or receipt rows or Run
output. These limits are not a bound on total database size or active scratch
space, and there is no semantic TTL, protected-reader registry or scratch cleanup.

## Review store server

```sh
artifactize server token add alice-laptop --scopes read,publish
artifactize server token add ci --scopes read
artifactize server run [--listen 127.0.0.1:8417]
artifactize server token list
artifactize server token revoke alice-laptop [--purge]
artifactize server rm STALE_KEY [EVAL_HASH]
```

`artifactize server` keeps a [shared remote review store](docs/design/remote-store.md)
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

A current completed identity/Eval-definition entry yields PASS or RED and a `reuse`
action. verify attaches cached results before their gates resolve, so the action
stays `reuse` while a dependency is pending or RED. The state then shows the gate
(WAIT_DEPENDENCY or BLOCKED) and the reason names it. An eval without a cached
result runs only once its gates are GREEN. It shows `blocked` behind a RED
dependency and `execute` when its dependencies are GREEN through cached results.
It shows `wait` when a dependency's result, directly or further upstream, exists
only after verify executes it (or after a live execution finishes). That is the
limit of the prediction: status cannot say whether a `wait` eval will execute. Text ends with, for example,
`Verify actions: will execute 1, will reuse 4, wait 0, blocked 0`, so status run on
a merged checkout answers what verify will re-review there without running it.
Force still applies only to selected evals. Human execution actions
record a waiting request; an active Human identity/Eval-definition pair projects WAITING_HUMAN and a
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
Sign in with ChatGPT credentials, never an API-key fallback. `claude` runs the
unmodified official `claude` CLI from `PATH`, already signed in, with the eval's
tools served over MCP; artifactize never reads Claude credentials (launch controls
are in `docs/PLAN.md`).
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
printing `slug` and `display_name`. All backends use the same JSON envelope described
under [Local diagnostics and maintenance](#local-diagnostics-and-maintenance).

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
ERROR; on cache hits the tool audit is retained with original execution attribution
and the attempts move to `reusedUsage`, never counted as spent. Counters
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

Owner validation before relying on a provider: run one real review per backend
(OpenAI key, Anthropic key, ChatGPT sign-in, Claude CLI) with an accessible exact
model ID and a declared tool. These real reviews are still pending for the owner.
Automated tests use fake HTTP transports or local HTTP servers and make no real
inference requests.

## Local diagnostics and maintenance

```sh
artifactize doctor [--repo PATH] [--state-dir PATH] [--json]
artifactize models openai|anthropic|chatgpt|claude [--json]
artifactize prune [--older-than 7d] [--dry-run] [--state-dir PATH] [--json]
```

`doctor` makes no provider calls and creates no Run, verdict, cache entry or auth
lock. It reports the resolved state directory and tests writability with a temporary
directory, removed immediately (in the nearest existing ancestor when state does
not yet exist). `--repo` additionally runs the same static validation as `config
check`. API keys are reported only as present/absent, never validated or printed.
ChatGPT login presence and Unix-second access-token expiry come from protected local
storage without refreshing. `claude` is located on PATH and only `--version` runs,
with a five-second timeout and bounded output; Claude credentials are never read.
Missing keys/login/binary and expired tokens are warnings: optional backends need
not all be installed. Invalid config, unsafe/unwritable state, invalid auth storage
or a failing installed CLI are hard errors. Exit is 0 without hard errors, 1 with
hard errors, and 2 for invocation errors.

`models openai` and `models anthropic` call the provider's models endpoint with the
corresponding API key through rig; Anthropic pagination is followed. ChatGPT uses
its existing login/refresh flow. There is no account or backend fallback. Claude
has no listing API, so its command explains `--model` names/aliases without launching
inference. Text lists tab-separated slug/name pairs; JSON is consistently
`{"backend":"openai","models":[{"slug":"model-id","display_name":"model-id"}]}`,
with a `note` and empty `models` for Claude. Listings preserve provider order.

`prune` operates on the selected state's Runs across repositories, not the current
repository. It skips unfinished Runs, nonterminal/waiting requests and live execution
owners (PID plus process start time). `--older-than` compares Run completion time;
use a whole-number `s`, `m`, `h`, `d` or `w` duration. With no age filter, every
eligible Run is considered. `--dry-run` returns `wouldRemove` without deleting;
normal JSON returns `removed`, and both include `skippedRuns`.

Only known scratch directories below `state/runs/<run-id>` are removed: runtime
`output`/`tmp`/`home`/`cache`, leftover tool output and Claude invocation directories.
Run roots, unknown files/directories, database rows, tool audit, results and cache
entries remain. Symlinks (including nested links), non-directory targets and
repository content are refused before deletion. Database reads have a five-second
busy timeout and finish before deletion; prune holds no writer lock. This is plain
prune, without quarantine, crash-recovery machinery or hostile filesystem-race
protection. Saved `run show` and `request show` remain readable after pruning.

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
The generated stdio server uses the current executable's absolute path. The Claude
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
identity/Eval-definition claim after the verifier exits. Cross-repository followers refer to that
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
status counts, executed and reused requests by reviewer kind, wall time, actual
executor starts, attempts, tool counts and usage reporting completeness. Run totals
exclude reused source executions; the Run-level `usage.saved` sums their original
counters. Reused requests keep source attribution and the raw per-provider attempts
in `reusedUsage`; their own summaries report no attempts or tool calls (`usageState: "none"`).
Unreported usage is never represented as a known zero token count. There are no
separate summary commands or `--full` mode.

## Monitor

```sh
artifactize monitor [--repo PATH | --all] [--state-dir PATH]
```

A terminal UI for review progress. Like `run list`, it shows the canonical
`--repo` (default: the current directory) or, with `--all`, every repository.
The Run list (newest first: ID, repository, status, request counts, age) refreshes
every second and on `r`; `j`/`k` or arrows move (moving past the end loads older
Runs), Enter opens a Run, `q` quits. A Run shows state counts, validation,
durations, budgets, executed and reused counts, spent and saved usage, running
evals, waiting Human requests and errors above an
Artifact/eval tree built from the saved definitions: families group their
instances (collapsed until expanded with `l`/→ or Enter), each eval shows its
status glyph and dependency Artifacts, `⇐` rows show child/mount/reference inputs,
and `↻` marks cycles. The right pane details the selected Artifact, family or
request: result, actual and requested profile, identity, reuse source, claim, tool
calls, usage and errors (PgUp/PgDn scroll; Esc returns to the list). The monitor
only reads the state database (read-only connections): it runs no owner code,
needs no repository, keeps the last data with an error line if a read fails, and
is not a review console; Human claim and submit stay in `request`.

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
