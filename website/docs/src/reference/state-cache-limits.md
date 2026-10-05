# State, cache and limits

Where artifactize keeps its state, how to inspect and limit the reuse cache, and
the local diagnostics and maintenance commands.

## State

One `state.sqlite` holds Runs from every repository (bundled SQLite, WAL, schema 4),
with the canonical repository path recorded on each Run. The state home is
`$ARTIFACTIZE_STATE_HOME`, falling back to `$XDG_STATE_HOME/artifactize` or
`~/.local/state/artifactize`. `--state-dir PATH` moves the whole state, including
private output directories under `PATH/runs`. An older database (schema 1 to 3,
artifactize 0.1 to 0.4) is upgraded in place to schema 4 before any read or write:
the 0.2 and 0.4 renames are applied to columns, the active index and saved JSON,
and saved declarations take the current fingerprint shape. Schema 4 keys reuse
differently ([the reuse key](../concepts/fingerprints-and-reuse.md#the-reuse-key)), and earlier reuse
records cannot be mapped to it, so the upgrade drops them: **the first `verify`
after the upgrade reviews again, once.** Runs, executions, their audit and waiting
Human requests survive. A Human request recorded before the upgrade can still be
claimed and submitted; its result settles its own Run, and the next `verify` asks
for a new sign-off. There is no migration from earlier receipt layouts. Saved Runs
stay readable after the original repository is removed. State/output inside the
reviewed repository is rejected, including through symlink ancestors; database
files and their WAL sidecars must be regular files. No writer transaction spans a
subprocess or async suspension. Completed results with a reuse key are shared
within this database; cross-process claims and polling waiters prevent duplicate
execution for the same key.

## Cache inspection

```sh
artifactize cache list [--history] [--json]
artifactize cache show KEY [--history]
artifactize cache rm KEY
```

These commands use the shared state home or `--state-dir PATH`, without loading a
repository. Each reuse key keeps every completed GREEN/RED record, and the latest by
completion time is the one `verify` reuses. `list` shows the latest record of each
key: key, eval, verdict, completion time, producer, repository (or the remote store
it came from), how many records the key holds, retained JSON bytes and last use (a
text table, or a JSON array that also carries the Eval definition hash, the target's
fingerprint and the profile variant). `--history` lists every record, latest first
within each key. `show` prints the key's latest saved execution as JSON, with
result, actual `profile`, `options` (backend, model, reasoning, limits and
variant), `fingerprints` (each Artifact the key covers), provenance, usage and
`producer` (`user@host` and artifactize version; submitted Human results also
record their `reviewer`); a missing key prints `null` and exits 4. `show
--history` prints all of the key's records as a JSON array, latest first. Entries
mirrored from a [shared remote review store](review-store.md) carry an `origin`
(store, publisher, publication time), which `list` shows in place of the
repository. Reads neither create missing state nor update access times. `rm`
removes every record of the key and prints `{"removed":true}` (false if absent),
preserving saved Runs and execution audit. `rm` refuses a key with an active
execution or waiter. Keys come from `cache list` or from a request's `key` in
`verify --json` and `run show`.

How reuse keys are built and what a hit returns is in
[Completed result reuse](../concepts/fingerprints-and-reuse.md#completed-result-reuse); entry and byte
limits are in [Cache limits](#cache-limits).

## Cache limits

Recording a new reusable result triggers LRU GC over every record of every key: at
most 10,000 records and 1 GiB of retained execution JSON, with a 16 MiB per-record
limit. Oversized results still reach their Run and existing waiters through the
saved execution, but later calls execute again. A reuse hit updates the last use of
the record it reused; inspection does not. A key's older records are therefore
evicted before its latest. GC evicts the least recently used eligible records first,
using the key then the completion time to break ties, and skips keys with an active
execution and records an in-flight waiter needs. Protected entries can temporarily exceed the caps, and
automatic maintenance failures are reported on stderr without replacing an already
completed verdict; the next publication retries collection.

GC and `rm` remove only reuse records, never execution or receipt rows or Run
output. These limits are not a bound on total database size or active scratch
space, and there is no semantic TTL, protected-reader registry or scratch cleanup.

## Local diagnostics and maintenance

```sh
artifactize doctor [--repo PATH] [--state-dir PATH] [--json]
artifactize models openai|anthropic [--json]
artifactize prune [--older-than 7d] [--dry-run] [--state-dir PATH] [--json]
```

`prune` operates on the selected state's Runs across repositories, not the current
repository. It skips unfinished Runs, nonterminal/waiting requests and live execution
owners (PID plus process start time). `--older-than` compares Run completion time;
use a whole-number `s`, `m`, `h`, `d` or `w` duration. With no age filter, every
eligible Run is considered. `--dry-run` returns `wouldRemove` without deleting;
normal JSON returns `removed`, and both include `skippedRuns`.

What `doctor` checks, the `models` output and exactly which directories `prune`
removes are in the [reference](#doctor-models-and-prune).

### Doctor, models and prune

`doctor` makes no provider calls and creates no Run, verdict, cache entry or auth
file. It reports the resolved state directory and tests writability with a temporary
directory, removed immediately (in the nearest existing ancestor when state does
not yet exist). It reads the state database's schema without changing the file:
an older schema (1 to 3) passes with a note that the next artifactize command
upgrades it, and a database written by a newer artifactize is a hard error. `--repo`
additionally runs the same static validation as `config check`. API keys are
reported only as present/absent, never validated or printed. Missing keys are
warnings: optional backends need not all be configured. A backend with a
[test endpoint](../guides/agent-evals.md#test-against-a-fake-provider) is a warning
that reports it as `testEndpoint`; an invalid test endpoint is a hard error. The remote review store
check is also offline: it reports the resolved URL, share level and token source; a
missing token is a warning, and an invalid configuration (including plain HTTP to a
non-loopback host) or an unsafe token file is a hard error. Invalid config,
unsafe/unwritable state or invalid auth storage are hard errors. Exit is 0 without
hard errors, 1 with hard errors, and 2 for invocation errors.

`models openai` and `models anthropic` call the provider's models endpoint with the
corresponding API key through rig; Anthropic pagination is followed. A test
endpoint replaces the provider's API root here too. There is no account or backend
fallback. Text lists tab-separated slug/name pairs; JSON is
consistently `{"backend":"openai","models":[{"slug":"model-id","display_name":"model-id"}]}`.
Listings preserve provider order.

Only known scratch directories below `state/runs/<run-id>` are removed: runtime
`output`/`tmp`/`home`/`cache`, leftover tool output and the Claude CLI invocation
directories of Runs made before 0.5.0.
Run roots, unknown files/directories, database rows, tool audit, results and reuse
records remain. Symlinks (including nested links), non-directory targets and
repository content are refused before deletion. Database reads have a five-second
busy timeout and finish before deletion; prune holds no writer lock. This is plain
prune, without quarantine, crash-recovery machinery or hostile filesystem-race
protection. Saved `run show` and `request show` remain readable after pruning.
