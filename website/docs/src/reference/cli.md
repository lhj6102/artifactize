# CLI

Every command, its flags, output and exit codes, then the details of `verify`
policy, budgets, selection files and profile variants.

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
| `verify SELECTOR` | `--profile NAME`, `--recursive`, `--force`, `--ignore-gates`, `--jobs N` (4), `--fingerprint-jobs N` (CPUs), `--max-executions N`, `--timeout-ms MS` (600000), `--reuse-only KINDS` | text or JSON | outcome |
| `status [SELECTOR]` | `--profile NAME`, `--recursive`, `--force`, `--ignore-gates`, `--fingerprint-jobs N` (CPUs); default `--all` | text or JSON | 0 satisfied, 1 not |
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
| `session show REF` | `--summary` | text, or the stored events (or the summary) as JSON | 0 |
| `session send REF MESSAGE` | | text or JSON | 0 |
| `cache list` | `--history` | text or JSON | 0 |
| `cache show KEY` | `--history` | JSON | 0; 4 missing |
| `cache rm KEY` | | JSON | 0 |
| `tools check [EVAL]` | `--eval ID`, `--artifact ID`, `--audience agent\|human`, `--tool NAME`, `--execute`, `--args JSON` | JSON | 0 ready, 1 not |
| `login codex` | sign-in URL on stderr; may read a pasted redirect URL from terminal stdin | text or JSON | 0 |
| `logout codex` | revokes and removes artifactize's stored tokens; leaves read-only auth files alone | text or JSON | 0 |
| `remote login URL` | `--share summary\|full` (summary); token on stdin | text or JSON | 0 |
| `remote logout` | | text or JSON | 0 |
| `remote status` | | text or JSON | 0 signed in, 1 not |
| `remote push` | `--dry-run` | text or JSON | 0 |
| `models openai\|anthropic\|codex` | | text or JSON | 0 |
| `doctor` | | text or JSON | 0 ready, 1 hard error |
| `prune` | `--older-than DURATION`, `--dry-run` | text or JSON | 0 |
| `monitor` | `--all` (not with `--repo`) | terminal UI | 0 |
| `review [REQUEST_ID]` | `--all` (not with `--repo`), `--reviewer NAME` (`$USER`) | terminal UI | 0 |
| `server run` | `--listen ADDR` (`127.0.0.1:8417`) | listening address | 0 |
| `server token add NAME` | `--scopes read,publish,human` | the token, once | 0 |
| `server token list` | | text or JSON | 0 |
| `server token revoke NAME` | `--purge` | JSON (also without `--json`) | 0 |
| `server rm KEY` | | JSON | 0 |

Run outcome codes (`verify`, `run show --wait`): 0 GREEN, 1 RED, 2 ERROR or
cancelled, 3 Human wait timeout, 4 INCOMPLETE. `run show --wait` follows a RUNNING
Run until it finishes; when its own timeout expires first it prints the current
Run, exits 3 and leaves the Run running. Every command exits 2 for usage errors
(unknown, repeated, conflicting or missing options and values) and operational
errors: text on stderr, or `{"error":"..."}` on stdout with `--json`. There is no
`plan`, `history`, `run cancel` or `--full`.

`session show` and `session send` read and continue an Agent review's
[saved conversation](../guides/agent-evals.md#saved-conversations). `REF` is the
request's `session.ref` (`user@host/STATE_ID/RUN_ID/REQUEST_ID/SESSION_ID`), a request
id such as `run-Hq2b9X-1`, or a `sessionId`; a request that reused a result names the
review that produced it. `show` prints the conversation as text, and with `--json` the
stored events as they are: `{"reference","file","events"}`. `show --summary` prints
the [session summary](../guides/agent-evals.md#session-summary), computed from the
session file alone: the backend, model and time span, the turns with their tokens and
the total, the tool calls per tool with how many failed, and the follow-ups; `--json`
prints `{"reference","sessionId","requestId","backend","model","reasoning","startedAt","endedAt","durationMs","followUps","turns","tokens","toolCalls"}`.
`send` prints the answer under a header naming the session, the follow-up number and
the model, and `files changed since this review` when an Artifact the review covered
changed; `--json` prints `{"reference","sessionId","requestId","send","filesChanged","answer","usage"}`.
Both exit 2 for a reference to another machine or state, a session the GC removed, one
that was not saved, a request that is still running (`send`), and a failed follow-up.

Commands that read or write the state refuse a database written by an earlier
artifactize with exit 2, without migrating it ([State](state-cache-limits.md#state)).
`doctor` is the exception: it reports an incompatible schema as a hard error in its
normal text or JSON report and exits 1.

## Runtime CLI

```sh
cargo run -q -p artifactize -- --repo crates/artifactize/tests/fixtures/runtime verify --all
cargo run -q -p artifactize -- run show RUN_ID
```

The fixture intentionally includes GREEN, RED, a timeout ERROR, RED-blocked and
ERROR-waiting dependents, and a two-Artifact cycle. Its overall exit code is 2.

[Runs, status and validation](../concepts/runs-and-status.md#verify-and-runs) covers selectors, gates, outcomes and output.

Root `reviewPolicy.dependencyGates` defaults to `green`; `ignore` enables bypass.
The library's `project::VerifyOptions.ignore_gates` can explicitly override either
policy, including `Some(false)` to enforce gates. `--force` marks only explicitly
selected evals for a fresh review, not recursive dependencies; it neither expands
the execution scope nor bypasses gates. Runs record the resolved policy and each
request's force flag. Forced evals never read or join cached results or live
executions; a forced GREEN or RED is added as the key's newest record, and
published to a configured review store, which later runs reuse. Dependencies may
still reuse their own local records, but a forced Run reads nothing from the store.

`--jobs N` bounds the evals one `verify` runs at once. Agent reviews are further
bounded machine-wide per backend by `$STATE/limits.json`
([Backend capacity](state-cache-limits.md#backend-capacity)); a request waiting for a backend slot
holds no job slot and no executor start.

`--fingerprint-jobs N` (on `verify` and `status`, at least 1) bounds how many
fingerprints are computed at once; the default is the number of CPUs available to
the process. In `verify` the one bound covers preparation and the end-of-review
rechecks of all concurrent reviews, independently of `--jobs`, which bounds evals.
Results and output do not depend on the bound or on completion order.

`verify --max-executions N` sets a nonnegative, shared per-Run executor-start
budget (unlimited when omitted); it is not an Artifact declaration field.
The Run records `jobs`, `fingerprintJobs`, `maxExecutions` and `executionsStarted`. A prepared
Runtime or Agent invocation consumes one start, even when it fails to spawn or
later returns ERROR. Human waiting, claim and tool actions consume no starts. Fingerprint preparation/rechecks, cache hits, and joined waiters
consume none. Preparation failures before invocation consume none. Zero permits
reuse, joining and Human reviews; a budgeted waiter replacing a failed/dead owner uses
its own Run's remaining budget. Exhaustion never interrupts running evals, but
leaves remaining READY requests `BUDGET_EXHAUSTED` and ends the Run INCOMPLETE
with a reason (exit 4). Hitting the cap exactly without unmet starts is not an
error. There are no reservations, refunds, admission pools or restart ledger.

`verify` waits for Human results as it waits for runtime and Agent evals: while a
Human request of the Run waits, the Run stays RUNNING, and a submission resumes the
evals that depend on it in the same Run
([Human reviews](../guides/human-reviews.md)). `--timeout-ms MS` (1–2147483647,
default 600000) bounds the wait. When it expires, the Run ends INCOMPLETE with
`waitTimedOut: true` and exit 3; the waiting requests stay open for submission,
and running evals are never interrupted.

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
variant arguments rebuild scoped references and dependency gates. The selected
variant is an execution option: it is not part of the reuse key, so results of
different variants (and of the declared profile) reuse each other unless a runtime
variant changes the command or args. Stored request `profile` and `options`
describe the execution that produced the result; `requestedProfile` retains the
requested profile when a hit returns another one, and text output then adds
`profile NAME` to the reuse marker.

## Reuse only

`verify --reuse-only KINDS` takes a comma-separated list of `runtime`, `agent` and
`human`. An eval of a listed kind may only reuse a result: a completed record of its
reuse key in the local cache, or in the [review store](review-store.md) when one is
configured. When there is none, the eval is not executed: no executor start, no
provider call, no Human request, and no waiting, not even for a live execution of
the key. A forced eval reads no cached result, so `--force` on a listed kind never
executes it. Unlisted kinds run as usual. In CI,
`artifactize verify --all --reuse-only agent,human` runs the tests and takes every
review from the cache or the store ([CI](../guides/ci.md)).

An eval that was not executed is reported `STALE`, with the reason in its
`blockedReason`:

```text
  docs/review [run-Hq2b9X-4]: STALE — Not reused (--reuse-only agent): no cached or stored result, so it was not executed.
```

Evals that depend on it wait, or are blocked, as they would behind any missing
evidence. The Run ends INCOMPLETE with a reason (exit 4), even when another eval is
RED or ERROR, as with an exhausted budget, and records the kinds as `reuseOnly`.
`status` does not take the flag: it predicts what a plain `verify` would do.
