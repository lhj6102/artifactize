# CLI

Every command, its flags, output and exit codes, then the details of `verify`
policy, budgets, selection files and profile variants.

## Command reference

Every command accepts `--repo PATH`, `--state-dir PATH` and `--json`, before or
after the subcommand, at most once each. Repository-reading commands normally use
the current directory when `--repo` is omitted, but defaults are command-specific:
**doctor checks project declarations only with an explicit `--repo`**; Monitor
and Review start on ALL as described below. State defaults to the shared
[state home](../features/others/state.md#state). Commands that do not use a
repository or state ignore those options. Monitor and Review reject `--json`;
`mcp` always writes JSON-RPC on stdout and startup diagnostics on stderr.
Current-directory tool output is unchanged by `--json`; it is already text or JSON
as listed below. After the program operand of `tools help`, trailing flags belong
to that program, not artifactize.
`SELECTOR` is exactly one of positional `ARTIFACT` or qualified `ARTIFACT/EVAL`,
`--eval ID`, `--evals CSV`,
`--artifacts CSV`, `--evals-file PATH`, `--artifacts-file PATH` or `--all`.
`help [COMMAND]` and `--help` print help; `--version` prints the version.

| Command | Flags | Output | Exit |
|---|---|---|---|
| `verify SELECTOR` | `--profile NAME`, `--recursive`, `--force`, `--jobs N` (opt-in), `--fingerprint-jobs N` (CPUs), `--max-executions N`, `--wait` or `--watch`, `--request-key KEY`, `--reuse-only KINDS`, `--speculate`, `--defer-human` | Run ID or `{run,state}`; final text/JSON with `--wait` | 0 accepted; outcome with `--wait` |
| `status [SELECTOR]` | `--profile NAME`, `--recursive`, `--force`, `--fingerprint-jobs N` (CPUs); default `--all` | text or JSON | 0 satisfied, 1 not |
| `mcp` | | [MCP stdio protocol](../agent-plugin/mcp.md); diagnostics on stderr | 0 on clean disconnect |
| `config check` | | text or JSON | 0 |
| `config graph [ARTIFACT]` | `--impact` | text or JSON | 0 |
| `run list` | `--repo-only` \| `--all`, `--limit N` (50), `--offset N` (0) | text or JSON | 0 |
| `run watch RUN_ID` | | JSON-line updates, eval results, final validation | Run outcome |
| `run cancel RUN_ID` | | cancellation acknowledgement JSON | 0 |
| `run show RUN_ID` | `--wait`, `--timeout-ms MS` (600000, needs `--wait`) | JSON | 0; outcome with `--wait` |
| `request list` | `--run RUN_ID` | text or JSON | 0 |
| `request show ID` | | JSON | 0 |
| `request logs ID` | `--stream stdout\|stderr\|both`, `--tail N`, `--start N`, `--end N`, `--search TEXT` | bounded evidence JSON | 0 |
| `request claim ID` | `--reviewer NAME` (`$USER`) | JSON | 0 |
| `request unclaim ID` | `--reviewer NAME` (`$USER`) | JSON | 0 |
| `request tool ID TOOL` | `--reviewer NAME` | text or JSON | 0; 2 tool error |
| `request submit ID` | `--verdict GREEN\|RED`, `--fields JSON` \| `--fields-file PATH`, `--reviewer NAME` | JSON | 0, also for RED |
| `session show REF` | `--summary` | text, or the stored events (or the summary) as JSON | 0 |
| `session send REF MESSAGE` | | text or JSON | 0 |
| `cache list` | `--history` | text or JSON | 0 |
| `cache show KEY` | `--history` | JSON | 0; 4 missing |
| `cache rm KEY` | | JSON | 0 |
| `tools check [EVAL]` | `--eval ID`, `--artifact ID`, `--audience agent\|human`, `--tool NAME`, `--execute`, `--args JSON` | JSON | 0 ready, 1 not |
| `tools read PATH` | — | UTF-8 text | 0 success, 1 tool error, 2 usage |
| `tools list [PATH]` | — | JSON | 0 success, 1 tool error, 2 usage |
| `tools glob PATTERN [PATH]` | — | JSON | 0 success, 1 tool error, 2 usage |
| `tools grep PATTERN [PATH]` | `--glob GLOB`, `--case-insensitive` | JSON | 0 success, 1 tool error, 2 usage |
| `tools section PATH HEADING` | — | Markdown text | 0 success, 1 tool error, 2 usage |
| `tools help PROGRAM [SUBCOMMAND...]` | trailing arguments passed to PROGRAM before `--help` | text | 0 success, 1 tool error, 2 usage |
| `tools open TARGET` | — | opens default application | 0 success, 1 tool error, 2 usage |
| `login codex` | sign-in URL on stderr; may read a pasted redirect URL from terminal stdin | text or JSON | 0 |
| `logout codex` | removes local tokens and attempts revocation; leaves read-only auth files alone | text or JSON | 0 after local sign-out |
| `remote login URL` | `--share summary\|full` (summary); token on stdin | text or JSON | 0 |
| `remote logout` | | text or JSON | 0 |
| `remote status` | | text or JSON | 0 signed in, 1 not |
| `remote push` | `--dry-run` | text or JSON | 0 |
| `models openai\|anthropic\|codex` | | text or JSON | 0 |
| `doctor` | | text or JSON | 0 ready, 1 hard error |
| `prune` | `--older-than DURATION`, `--dry-run` | text or JSON | 0 |
| `monitor [PATH]` | starts on ALL; PATH or `--repo PATH` selects only the initial repository; `--all` conflicts with a path | terminal UI | 0 |
| `review [REQUEST_ID]` | `--repo PATH` (a `repo:` search term), `--all` (the default; not with `--repo`), `--reviewer NAME` (`$USER`) | terminal UI | 0 |
| `server run` | `--listen ADDR` (`127.0.0.1:8417`) | listening address | 0 |
| `server token add NAME` | `--scopes read,publish,human` | the token, once | 0 |
| `server token list` | | text or JSON | 0 |
| `server token revoke NAME` | `--purge` | JSON (also without `--json`) | 0 |
| `server rm KEY` | | JSON | 0 |

Run outcome codes (`verify --wait`, `run show --wait`): 0 GREEN, 1 RED, 2 ERROR or
cancelled, 4 INCOMPLETE, 5 SIGNOFFS_PENDING (only sign-offs remain, not approval). Plain `verify` exits 0 on acceptance, not validation.
Ctrl-C stops a watcher with exit 130; closing it never cancels the Run. `run show --wait` follows a RUNNING
Run until it finishes; when its own timeout expires first it prints the current
Run, exits 3 and leaves the Run running. Every command exits 2 for usage errors
(unknown, repeated nonrepeatable, conflicting or missing options and values) and operational
errors: text on stderr, or `{"error":"..."}` on stdout with `--json` (except
`mcp`, whose stdout remains protocol-only). There is no
`plan`, `history` or `--full`.

List options `--reuse-only` and `server token add --scopes` may be repeated;
their comma-separated values accumulate across occurrences. This does not apply
to the common `--repo`, `--state-dir` and `--json` options, which are accepted once.
`run show --timeout-ms` requires an integer from 1 through 2147483647 milliseconds;
a value outside that range is a usage error (exit 2).

`logout codex` returning 0 confirms local sign-out, not necessarily server-side
revocation. Revocation is best-effort: if it cannot be confirmed, the command warns
on stderr and its JSON result includes `"revoked":false` (true when confirmed).
Local tokens are still removed; a read-only external auth file is never changed.


### Identity arguments

Run/request ID arguments (including `request list --run` and `review REQUEST_ID`)
are validated before reading state: 1–200 ASCII letters, digits, `-`, `_` or `.`,
never only dots. Reuse-key arguments to `cache show`, `cache rm` and `server rm`
require exactly 64 lowercase hexadecimal characters. Malformed identities exit 2
as usage errors; valid but absent cache keys still make `cache show` print `null`
(or `[]` with `--history`) and exit 4, while `cache rm` reports `removed: false`.

`server token add NAME` and `server token revoke NAME` accept 1–64 ASCII
characters: letters, digits, `-`, `_` and `.`. Empty names, whitespace, slashes,
non-ASCII text and longer names are rejected with exit 2 **before opening the
token store**. This name is a display label, not the bearer secret.

`session show` and `session send` read and continue an Agent review's
[saved conversation](../features/agent.md#saved-conversations). `REF` is the
request's `session.ref` (`user@host/STATE_ID/RUN_ID/REQUEST_ID/SESSION_ID`), a request
id such as `run-Hq2b9X-1`, or a `sessionId`; a request that reused a result names the
review that produced it. `show` prints the conversation as text, and with `--json` the
stored events as they are: `{"reference","file","events"}`. `show --summary` prints
the [session summary](../features/agent.md#session-summary), computed from the
session file alone: the backend, model and time span, the turns with their tokens and
the total, the tool calls per tool with how many failed, and the follow-ups; `--json`
prints `{"reference","sessionId","requestId","backend","model","reasoning","startedAt","endedAt","durationMs","followUps","turns","tokens","toolCalls"}`.
`send` prints the answer under a header naming the session, the follow-up number and
the model, and `files changed since this review` when an Artifact the review covered
changed; `--json` prints `{"reference","sessionId","requestId","send","filesChanged","answer","usage"}`.
Both exit 2 for a reference to another machine or state, a session the GC removed, one
that was not saved, a request that is still running (`send`), and a failed follow-up.

Commands opening an existing state apply supported additive schema upgrades with write
access. Schemas 6 and 7 upgrade to 8 (6 → 7 → 8 or 7 → 8) while preserving Runs,
cache and Human approvals; schemas 1–5 and versions above 8 are refused ([State](../features/others/state.md#state)).
`doctor` reports an incompatible schema as a hard error and exits 1.

## Submission and watching

**Breaking change:** plain `verify` is submit-only. Scripts/CI requiring the old validation
exit codes must add `--wait`. Its JSON is `{"run":"run-…","state":"preparing"}`
(the state may already have advanced); only `verify --wait --json` renders the full final
saved Run. `--wait` has no Human timeout and all eval kinds follow Run settlement.
In strict default mode, a pending Human request keeps the Run RUNNING; submitting
its sign-off resumes dependent evals in that same Run.

`--request-key KEY` accepts 1–128 ASCII letters/digits/`._:-`. Repeating a key for the same
canonical repository returns its original Run without changing the accepted scope/options
or re-executing it, even after checkout changes. Omitting the key always creates a new Run.

```sh
artifactize verify --all --request-key build-42
artifactize run show RUN_ID --wait
artifactize run cancel RUN_ID
```

Cancellation acknowledges intent; inspect the Run for its eventual outcome after cleanup.
Repeated cancellation and cancellation after completion are safe. Only explicit cancel
stops execution; terminal closure and watcher interruption stop observation only.

## Verify modes

`--speculate` and `--defer-human` are independent per-request switches, both off
by default. The accepted Run records `modes: {speculate, defer_human}`; a retry
with the same `--request-key` retains the first request's modes.

| Modes | Dispatch and settlement |
|---|---|
| Neither | Wait for dependency GREEN evidence; the owned Run waits for people |
| `--defer-human` | Wait for machine gates; run past deferrable sign-offs provisionally |
| `--speculate` | Start all requested kinds before unresolved gates; wait for people |
| Both | Start machine reviews speculatively; ask for deferrable sign-offs last |

Speculation respects `--jobs`, backend capacity and `--max-executions`. Ready,
unblockable work is dispatched first, longest remaining dependency paths next,
with selection order breaking ties. Speculative work uses leftover capacity and
is cancelled first when ready work needs that capacity; redispatch consumes
another executor start. An upstream RED or ERROR cancels its transitive blocked
work: runtime process groups, Agent reviews and their tool sessions end, Human
requests are withdrawn, and not-started requests never start. Those requests end
BLOCKED with no cancelled result eligible for reuse. An upstream GREEN retains
speculative results: fingerprints cover files, not upstream verdicts.

With `--defer-human`, pending deferrable Human gates count as passing **for
execution**, never for final approval. Human requests are recorded as DEFERRED
only after their Artifact's machine evals finish and nothing blocks them.
Dependent results are PROVISIONAL. When every machine obligation passes and only
these sign-offs remain, the Run ends SIGNOFFS_PENDING; `--wait` returns 5.
Missing results, exhausted budgets, RED and ERROR retain their ordinary outcomes.
`gate = "blocking"` on an eval retains its pending Human gate and the owned Run
waits for it. CI's `--reuse-only agent,human` remains strict even with these flags.

A later Human GREEN reruns nothing: the next `verify` reuses machine evidence and
the sign-off and is SATISFIED, provided its fingerprints enable reuse and are
unchanged. A RED blocks dependents on the next verification. Completed Runs remain
historical records; submission does not restart them.

## Runtime CLI

These development examples require a source checkout, Rust and the platform
build prerequisites from [Installation](../getting-started/install.md#build-from-source-all-platforms).
Run them from the repository root; `cargo run` builds the CLI from that checkout.
For an installed CLI, use `artifactize` directly with your own project's path.

```sh
cargo run -q -p artifactize -- --repo crates/artifactize-core/tests/fixtures/runtime verify --all --wait
cargo run -q -p artifactize -- run show RUN_ID
```

The fixture intentionally includes GREEN, RED, a timeout ERROR, RED-blocked and
ERROR-waiting dependents, and a two-Artifact cycle. Its overall exit code is 2.

[Runs, status and validation](../features/run.md#verify-and-runs) covers selectors, gates, outcomes and output.

Gate bypass is removed: `--ignore-gates`, `VerifyOptions.ignore_gates` and
`review_policy.dependency_gates` are no longer accepted. Choose the two
per-request verify modes instead. Old saved keys are ignored on read; no state migration is needed for removal. `--force` marks only explicitly
selected executable evals for a fresh review, not recursive dependencies; it
neither expands
the execution scope nor bypasses gates. Runs record the resolved policy and each
request's force flag. Forced evals never read or join cached results or live
executions; a forced GREEN or RED is added as the key's newest record, and
published to a configured review store, which later runs reuse. Dependencies may
still reuse their own local records, but a forced Run reads nothing from the store.
Speculation and deferral never waive final requirements.

Concurrent Runs on the same machine share an in-flight eval when its reuse key
and effective profile match, including across worktrees. The waiting request names
the execution; its GREEN, RED or ERROR is joined reuse. Cancellation or an owner
process crash lets one waiter take over. Different profiles and forced evals execute
independently; completed GREEN/RED reuse rules are unchanged. This needs no option
and does not share in-flight work through a remote store.

There is no default eval-concurrency cap: every READY eval starts as soon as it is
ready. Add `--jobs N` (minimum 1) only when that concurrency causes trouble; it
then bounds the evals of this Run. Agent reviews are still
bounded machine-wide per backend by `$STATE/limits.json`
([Backend capacity](../features/others/state.md#backend-capacity)); a request waiting for a backend slot
holds no optional `--jobs` slot and no executor start.

`--fingerprint-jobs N` (on `verify` and `status`, at least 1) bounds how many
fingerprints are computed at once; the default is the number of CPUs available to
the process. In `verify`, one shared bound covers preparation and Runtime/Agent
end-of-review rechecks, independently of `--jobs`, which bounds evals. Human tool
actions and submissions separately use CPU-sized fingerprint parallelism and do
not inherit this setting. Results and output do not depend on the bound or on
completion order.

`verify --max-executions N` sets a nonnegative, shared per-Run executor-start
budget (unlimited when omitted); it is not an Artifact declaration field.
The Run records an explicit `jobs` cap (the field is absent when unlimited),
`fingerprintJobs`, `maxExecutions` and `executionsStarted`. A prepared
Runtime or Agent invocation consumes one start, even when it fails to spawn or
later returns ERROR. Human waiting, claim and tool actions consume no starts. Fingerprint preparation/rechecks, cache hits, and
joined waiters
consume none. Failures in initial executor preparation consume no start. Agent
`execution_paths` pinning happens after a start is charged: a pinning failure still
consumes that start and records ERROR/PREPARATION_FAILED without calling the model.
Zero permits reuse, joining and Human reviews; a budgeted waiter replacing a failed/dead owner uses
its own Run's remaining budget. Exhaustion never interrupts running evals, but
leaves remaining READY requests `BUDGET_EXHAUSTED` and ends the Run INCOMPLETE
with a reason (exit 4). Hitting the cap exactly without unmet starts is not an
error. There are no reservations, refunds, admission pools or restart ledger.

Human waiting and cancellation follow [Submission and watching](#submission-and-watching);
the [Human guide](../features/human.md) covers claims, tools and sign-offs.

Selection files must be regular files no larger than 4 MiB, containing a JSON
string array or one trimmed ID per line (UTF-8 BOM and CRLF are accepted). They
must contain 1–100000 IDs before deduplication. Empty lines are ignored; IDs cannot
contain ASCII whitespace, control characters or commas. Malformed JSON arrays
never fall back to line parsing. Relative file paths resolve from the CLI's cwd,
not `--repo`.

`verify ... --profile NAME` selects a complete declared `profile_variants` entry
for every executable eval included by the selection and recursion policy before
applying variants.
Each executable eval can declare up to 64 safely named variants,
all retaining its default reviewer kind. Unknown variants fail before creating a
Run, and source declarations are never rewritten. The library accepts
`project::selection::ProfileSelection::Named` or `ProfileSelection::Evals` (a
qualified-eval-to-name map); mappings outside that pre-variant included scope fail.
With `--recursive`, variants apply to executable dependencies already in that
scope. Runtime variant arguments then rebuild scoped references and dependency
gates, and the included scope is recomputed. Dependencies newly included through
those variant arguments retain their default profiles; the named variant is not
applied again to the expanded scope. The selected
variant is an execution option: it is not part of the reuse key, so results of
different variants (and of the declared profile) reuse each other unless a runtime
variant changes the command or args. Stored request `profile` and `options`
describe the execution that produced the result; `requestedProfile` retains the
requested profile when a hit returns another one, and text output then adds
`profile NAME` to the reuse marker.

## Reuse only

`verify --reuse-only KINDS` takes a comma-separated list of `runtime`, `agent`
and `human`. An Eval of a listed kind may only reuse a result: a completed record of its
reuse key in the local cache, or in the [review store](../features/team/reference.md) when one is
configured. When there is none, the eval is not executed: no executor start, no
provider call, no Human request, and no waiting, not even for a live execution of
the key. A forced eval reads no cached result, so `--force` on a listed kind never
executes it. Unlisted kinds run as usual. In CI,
`artifactize verify --all --wait --reuse-only agent,human` runs the tests and takes every
review from the cache or the store ([CI](../features/team/ci.md)).

An eval that was not executed is reported `STALE`, with the reason in its
`blockedReason`:

```text
  docs/review [run-Hq2b9X-4]: STALE — Not reused (--reuse-only agent): no cached or stored result, so it was not executed.
```

Evals that depend on it wait, or are blocked, as they would behind any missing
evidence. The Run ends INCOMPLETE with a reason (exit 4), even when another eval is
RED or ERROR, as with an exhausted budget, and records the kinds as `reuseOnly`.
`status` does not take the flag: it predicts what a plain `verify` would do.

## Dependency selection and output

`artifactize verify app/e2e` selects the qualified Eval just like `--eval app/e2e`.
Add `--recursive` to execute the Evals in the required dependency scope. Without
it, input evidence must already be available. File Artifacts are selected by their
declared names; a sidecar creates no automatic relation to the surrounding folder.

Only the Eval that references an input waits for its reviews. Its independent
siblings remain runnable. Plain verify is dependency-first; `--speculate` changes
scheduling, not approval. Actual Eval-cycle peers run independently of one another,
retain their own verdicts, and still honor external gates. A failed cycle does not
release downstream consumers. See [Eval dependencies](../features/declarations.md#eval-dependencies).

Status uses `execute`, `reuse`, `wait` and `blocked` actions. Requests, validation
and status carry `blockedBy` qualified Eval IDs. The monitor groups gate Evals by
Artifact under `Waits for`. Tags, file kinds and paths remain visible in graph,
status and monitor output.

## Saved Run evidence and terminal views

`run show RUN_ID --json` and `verify --wait --json` include top-level `evidence` when
nonempty. Run fields are flattened into the output object, without a `run` wrapper.
It maps qualified eval IDs to saved verdicts used without a request in this Run,
such as cached upstream evidence in a partial Run. It is not the latest global
verdict. See [Runs](../features/run.md#verify-and-runs).
For the terminal keys and layout, see [Monitor](../tui/index.md#monitor)
and [Human reviews](../tui/index.md#review).

## Current-directory tools

`artifactize tools` also exposes the [standalone scoped tools](../features/tools/builtin.md#artifactize-tools-cli),
using the same CLI definition and runner as `artifactize-tools`. Filesystem
operands are relative to the current directory (not `--repo`), with `.` the
default for list/glob/grep; absolute filesystem targets, `..` and symlinks are
refused. This does not restrict **help's literal executable**: it uses ordinary
cwd/PATH lookup, allowing absolute or parent paths and executable symlinks, and
forwards subcommand arguments without scope resolution. Only `open` accepts
HTTP(S) URLs. `artifactize tools --help` lists both `check` and these tools, and
`artifactize tools SUBCOMMAND --help` documents each one. The `help` subcommand
runs a trusted program with `--help`; it is not help navigation.


### Agent streaming and runtime logs

`verify SELECTOR --watch` submits and immediately follows the saved Run;
`run watch RUN_ID` attaches later. Both emit flushed JSON lines, regardless of
`--json`: `kind: update` for live state/counts, `kind: eval_result` for a concise
saved request result, and `kind: outcome` for final Run validation. `--watch` and
`--wait` are mutually exclusive. The watcher exits with the Run outcome; interrupt
exits 130 and stops observation only. Reattachment replays completed evals, not an
exact historical event journal. Follow an outcome's `next_cursor` through MCP
`verify_result`, or inspect the saved Run when full detail is needed.

For a failed runtime finding:

```sh
artifactize request logs REQUEST_ID --stream stderr --tail 30
artifactize request logs REQUEST_ID --stream stdout --start 100 --end 150
artifactize request logs REQUEST_ID --search 'failure' --tail 10
artifactize config graph INPUT_ARTIFACT --impact --json
```

Log queries return explicit retention/missing evidence and original line numbers,
not the full request audit. Tail defaults to 50 matching lines per stream; range
queries return at most 200 lines per stream and a bounded byte budget, with
`next_start` for continuation. Tail and range are exclusive. `search` is a literal,
case-sensitive 1–2048-byte string. Lines longer than 2048 raw bytes are marked
`clipped` with bounded head/tail text. Full raw runtime streams, including tails beyond the 128 KiB saved
audit, stay in Run output until pruning; older/remote/pruned evidence may have
only the saved audit and says so. See [MCP focused diagnosis](../agent-plugin/mcp.md#streaming-and-focused-diagnosis)
for the shared evidence/event schemas and client delivery limits.
