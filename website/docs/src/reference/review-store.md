# Review store server and client

The exact behavior of `artifactize server` and of the client side of the shared
review store. The [Team review store](../guides/team-review-store.md) guide shows
how to set one up.

## Review store server

```sh
artifactize server token add alice-laptop --scopes read,publish
artifactize server token add ci --scopes read
artifactize server run [--listen 127.0.0.1:8417]
artifactize server token list
artifactize server token revoke alice-laptop [--purge]
artifactize server rm KEY
```

`artifactize server` keeps a [shared remote review store](https://github.com/lhj6102/artifactize/blob/main/docs/design/remote-store.md)
in its own `review-store.sqlite` under `--state-dir`, separate from `state.sqlite`.
It keeps every record of each [reuse key](../concepts/fingerprints-and-reuse.md#the-reuse-key) and
answers a lookup with the latest one by completion time, GREEN or RED, whoever
published it and in whatever order records arrived. `server run` serves plain HTTP
on loopback by default (it warns when bound elsewhere) and stops on Ctrl-C/SIGTERM;
put a TLS proxy or tunnel in front, because clients require HTTPS except on
loopback. Token commands work on the same file while the server runs, and take
effect immediately.

`token add` prints a random bearer token once; the store keeps only its SHA-256.
Scopes are `read` (look up), `publish` (publish runtime and Agent records) and
`human` (additionally publish Human sign-offs, together with `publish`). Give
untrusted CI `read` only. `token list` shows names, scopes and creation/revocation
times, never tokens. `token revoke NAME` rejects the token at once; `--purge`
also deletes every record it published, and may be repeated later. Revocation always
prints a JSON report, even without `--json`. Names of revoked tokens are never reused. `rm KEY` deletes every record of a key and prints
`{"removed":true}` (false if absent).

The API takes `Authorization: Bearer TOKEN` and answers JSON (`{"error":...}` on
failure; 401 for a missing, unknown or revoked token, 403 for a missing scope):

| Route | Scope | Result |
|---|---|---|
| `GET /v1/whoami` | any | `{"principal":NAME,"scopes":[...]}` |
| `POST /v1/lookup` with `{"keys":[KEY,...]}` (at most 1000) | `read` | `{"entries":[record,...]}`: the latest record of each found key |
| `PUT /v1/entries/{key}` with a record | `publish` (+ `human` for Human records) | 201 `{"created":true}`, or 200 `{"created":false}` when the store already holds this execution of the key |

The server checks a record's envelope (schema 2, the path's key, a GREEN/RED
verdict, a profile kind, an execution ID and an RFC 3339 `completedAt`) and size:
256 KiB for a summary, 16 MiB for a full record carrying `execution`. It stamps
`publisher` (the token name) and `publishedAt` (server clock), replacing any client
values. Clients check that a record's key matches its `evalDefHash` and
`fingerprints` before they use it. Lookups update the last use of the record they
return; inserts evict least-recently-used records above 100,000 records or 4 GiB.

**Upgrading.** A schema 1 or 2 `review-store.sqlite` (artifactize 0.3 or 0.4) is
upgraded in place to schema 3 when the server opens it. Tokens are kept. Stored
records are dropped: they were keyed by fingerprint and Eval definition hash as 0.4
computed them, which no 0.5 key matches, so clients review again once and publish
anew. A 0.5 server answers 0.4 and 0.3 clients (their lookup keys and their
`PUT /v1/entries/{evalDefHash}/{fingerprint}` route) with 410 and a message to
upgrade; the 0.3 `staleKey` alias is gone. Upgrade the server and its clients
together.

0.9 uses reuse-key v2, so it reuses no pre-0.9 record, including script-fingerprint
results and Human sign-offs. Existing store records cannot satisfy the new keys;
clients review once and publish new records. This is separate from local state
schema 6, which requires a new state without migration.

## Remote review store client

```sh
printf '%s\n' "$TOKEN" | artifactize remote login https://reviews.example/ [--share full]
artifactize remote status [--json]
artifactize remote push [--dry-run] [--json]
artifactize remote logout
```

A client is configured only through the state directory and the environment,
never `.artf` declarations, so a cloned repository cannot redirect a token. `remote
login URL` reads one token line from stdin (never argv). It attempts to hide
terminal input; on Windows, if that fails, it warns that the token will be visible
and continues reading. Non-terminal stdin is read without changing terminal echo.
It verifies the token with `GET /v1/whoami`, then stores it in
`$STATE/auth/remote-token.json` (0700 directory,
0600 single-link file, never followed through a symlink) and writes
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

With a store configured, `verify` reads and writes through it. Before the Run claims
anything, it looks up every key once, in one batched `POST /v1/lookup`, including
keys that already have local records, and reuses whichever record completed last:
the key's latest local record or the store's latest. A store record that completed
after the local latest (or with no local record at all) is appended to the local
history as a self-contained `remote-<executionId>` execution with an `origin`, and
reused like a local record, so `run show`, `cache`, GC, the monitor and the Run
summary count it as reuse. A key that still has no result is looked up again just
before a local claim and on each poll of `verify`'s Human wait, at most once per second. A remote result for a key that waits
for a Human settles the waiting requests; their never-reviewed waiting execution
becomes ERROR (`SUPERSEDED`). Text output names the source:

```text
  app/check [run-Hq2b9X-1]: GREEN (reused from remote: alice@laptop, run-x05qFq)
  brand/signoff [run-Hq2b9X-2]: GREEN (reused from remote: Human sign-off by alice, published by alice-signoff, run-Ksl1Qr)
```

The producer (`user@host`) and the Human reviewer are what the publishing machine
recorded; the publisher is the server-authenticated token name. An Agent result's
`producer` also carries `session`, the reference to its
[saved conversation](../guides/agent-evals.md#saved-conversations) on the producing
machine; the conversation itself is never published. Older clients ignore the field,
and the server stores records as they are, so the protocol and record `schema` (2) are
unchanged. A reused remote result's request shows the reference, and `session show`
on it names the machine and state where the conversation lives. Once the local settle
records a GREEN/RED with a reuse key (after the fingerprint recheck), verify sends
its summary record, or the full record with share `full`, outside any database
transaction. Records carry the key, the Eval definition hash, the fingerprint of
each Artifact the key covers, the execution `options` (backend, model,
reasoning, limits and profile variant) and an Agent result's `executionPaths` pins. `request submit` publishes Human sign-offs.
A token without `read` looks nothing up and one without `publish` publishes nothing,
so a read-only CI token only reuses. Human sign-offs also need `human`; with any
other token they stay local with a warning. Nothing is published for evals without a
reuse key. Dependency evals have no key or store record and are derived locally
from current required evidence, not looked up or published. `--force` never reads
from the store, but a forced result is a new record
and is published like any other, so it becomes the store's latest too (unless a
newer one exists). `status` makes the same comparison with a read-only lookup,
without mirroring, so its `reuse` prediction includes newer remote results;
`status --force` makes no remote call.

`remote push` sends, for each key, the latest GREEN/RED record this machine
produced: results produced while the store was unreachable, before `remote login`
or with `ARTIFACTIZE_REMOTE=off`. It never sends mirrors. A token with `read` first looks up
the store's latest record of each key, and a record that already is the latest is
not sent again. The store keeps each execution once, so a record it already holds
answers `created: false`. Both count as `existing`, and pushing twice is harmless.
Human sign-offs without the `human` scope, and records over their size limit, are
`skipped` with a reason on stderr. `--dry-run` sends nothing and reports what would
be pushed. The output is `Pushed N, already in the store M, skipped K.`, or JSON
`{"dryRun":false,"pushed":N,"existing":M,"skipped":K}`. Unlike `verify`, `push`
fails on any remote failure, and it needs the `publish` scope.

Connection errors, timeouts and 5xx answers fail open: one warning, then the process
continues without the remote, reusing the local latest record of each key and
reviewing locally what has none. 401/403, TLS failures, other error answers and
invalid configuration fail closed (exit 2), with the store's own error message when
it sends one. A lookup that fails before the Run
starts leaves no Run; a later failure records the Run as ERROR. A record that does
not validate is ignored with a warning, and its eval is reviewed locally.
