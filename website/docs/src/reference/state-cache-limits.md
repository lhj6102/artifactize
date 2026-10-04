# State, cache and limits

Where artifactize keeps its state, how to inspect and limit the reuse cache, and
the local diagnostics and maintenance commands.

## State

One `state.sqlite` holds Runs from every repository (bundled SQLite, WAL, schema 3),
with the canonical repository path recorded on each Run. The state home is
`$ARTIFACTIZE_STATE_HOME`, falling back to `$XDG_STATE_HOME/artifactize` or
`~/.local/state/artifactize`. `--state-dir PATH` moves the whole state, including
private output directories under `PATH/runs`. A schema 1 or 2 database
(artifactize 0.1 to 0.3) is upgraded in place to schema 3 (the fingerprint rename)
before any read or write: columns, the active index and saved JSON fields take the
new name, and saved declarations take the fingerprint shape. Cache entries and
waiting Human requests survive, so unchanged fingerprints keep reusing their
results. There is no migration from earlier receipt layouts. Saved Runs stay readable after
the original repository is removed. State/output inside the reviewed repository
is rejected, including through symlink ancestors; database files and their WAL
sidecars must be regular files. No writer transaction spans a subprocess or async
suspension. Completed results with a fingerprint are shared within this database; cross-process
claims and polling waiters prevent duplicate execution for the same fingerprint.

## Cache inspection

```sh
artifactize cache list --json
artifactize cache show FINGERPRINT [EVAL_HASH]
artifactize cache rm FINGERPRINT [EVAL_HASH]
```

These commands use the shared state home or `--state-dir PATH`, without loading a
repository. `list` shows fingerprint, Eval definition hash, original verdict/repository/eval, retained JSON
bytes and last use (a text table, or a JSON array). `show` always prints the full
saved execution with result, actual profile, provenance, usage and `producer`
(`user@host` and artifactize version; submitted Human results also record their
`reviewer`); a missing entry prints `null` and exits 4. Entries mirrored from a
[shared remote review store](https://github.com/lhj6102/artifactize/blob/main/docs/design/remote-store.md) carry an `origin`
(store, publisher, publication time), which `list` shows in place of the repository. Reads neither create missing state nor update
access times. `rm` prints `{"removed":true}` (false if absent), preserving saved
Runs and execution audit. For `show` and `rm`, the hash may be omitted when the
fingerprint has only one entry; multiple definitions require the full hash from
`cache list`. `rm` refuses a key with active executions or waiters (without a hash,
any active definition for that fingerprint prevents removal).

How reuse keys are built and what a hit returns is in
[Completed result reuse](../concepts/fingerprints-and-reuse.md#completed-result-reuse); entry and byte
limits are in [Cache limits](#cache-limits).

## Cache limits

Publishing a new reusable entry triggers LRU GC: at most 10,000 entries and 1 GiB
of retained execution JSON, with a 16 MiB per-entry limit. Oversized results still
reach their Run and existing waiters through the saved execution, but later calls
execute again. Reuse hits update last use; inspection does not. GC evicts oldest
eligible entries first, using fingerprint then definition hash to break ties, and skips active executions
and in-flight waiters. Protected entries can temporarily exceed the caps, and
automatic maintenance failures are reported on stderr without replacing an already
completed verdict; the next publication retries collection.

GC and `rm` remove only reuse mappings, never execution or receipt rows or Run
output. These limits are not a bound on total database size or active scratch
space, and there is no semantic TTL, protected-reader registry or scratch cleanup.

## Local diagnostics and maintenance

```sh
artifactize doctor [--repo PATH] [--state-dir PATH] [--json]
artifactize models openai|anthropic|chatgpt|claude [--json]
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
lock. It reports the resolved state directory and tests writability with a temporary
directory, removed immediately (in the nearest existing ancestor when state does
not yet exist). It reads the state database's schema without changing the file:
an older schema (1 or 2) passes with a note that the next artifactize command
upgrades it, and a database written by a newer artifactize is a hard error. `--repo`
additionally runs the same static validation as `config check`. API keys are reported only as present/absent, never validated or printed.
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
