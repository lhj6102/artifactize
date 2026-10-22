# Review store server and client

Start a store with `artifactize server token add alice --scopes read,publish` (it prints
the token once) and `artifactize server run`, then sign a machine in with
`printf '%s\n' "$TOKEN" | artifactize remote login https://reviews.example/`, which prints
`Signed in to … as alice`. This page is the exact behavior of the server and of the client;
the [Team review store](../features/team/index.md) guide walks through a setup.

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
It keeps every record of each [reuse key](../features/fingerprint.md#the-reuse-key) and
answers a lookup with the latest one by completion time, GREEN or RED, whoever
published it and in whatever order records arrived. `server run` serves plain HTTP
on loopback by default (it warns when bound elsewhere) and stops on Ctrl-C;
put a TLS proxy or tunnel in front, because clients require HTTPS except on
loopback. Token commands work on the same file while the server runs, and take
effect immediately.

Token names are bounded ASCII display labels, not secrets; the
[CLI identity rules](index.md#identity-arguments) give the syntax and
pre-store validation.

`token add` prints a random bearer token once; the store cannot show it again, so save it then.
Scopes are `read` (look up), `publish` (publish runtime and Agent records) and
`human` (additionally publish Human sign-offs, together with `publish`). Give
untrusted CI `read` only. `token list` shows names, scopes and creation/revocation
times, never tokens. `token revoke NAME` rejects the token at once; `--purge`
also deletes every record it published, and may be repeated later. Revocation always
prints a JSON report, even without `--json`:
`{"name":"alice-laptop","revokedAt":"2026-10-06T09:14:02Z","purgedEntries":0}`, where
`purgedEntries` counts the records `--purge` deleted. Names of revoked tokens are never reused. `rm KEY` deletes every record of a key and prints
`{"removed":true}` (false if absent).

The API takes `Authorization: Bearer TOKEN` and answers JSON (`{"error":...}` on
failure; 401 for a missing, unknown or revoked token, 403 for a missing scope):

| Route | Scope | Result |
|---|---|---|
| `GET /v1/whoami` | any | `{"principal":NAME,"scopes":[...]}` |
| `POST /v1/lookup` with `{"keys":[KEY,...]}` (at most 1000) | `read` | `{"entries":[record,...]}`: the latest record of each found key |
| `PUT /v1/entries/{key}` with a record | `publish` (+ `human` for Human records) | 201 `{"created":true}`, or 200 `{"created":false}` when the store already holds this execution of the key |

The server refuses a malformed record and limits its size to 256 KiB for a summary
and 16 MiB for a full record. It sets `publisher` (the token name) and `publishedAt`
(its own clock). Clients use a record only if it matches the key they asked for, so a
store cannot make a client reuse a result for different criteria or inputs. Lookups update the last use of the record they
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
upgrades: schemas 1–5 need replacement, while existing schemas 6 to 8 upgrade
automatically to schema 9 with Runs, cache records and Human approvals preserved.

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
`$STATE/auth/remote-token.json`, which only your account can read and writes
`$STATE/remote.json`: `{"url":"https://reviews.example/","share":"summary"}`.
URLs must use HTTPS with the system trust store; plain `http://` is accepted only
for `localhost`, `127.0.0.0/8` and `[::1]`. Credentials, queries and fragments are
rejected. A stored token is bound to the origin it was issued for.

`ARTIFACTIZE_REMOTE` (a URL, or `off`), `ARTIFACTIZE_REMOTE_TOKEN` (for CI) and
`ARTIFACTIZE_REMOTE_SHARE` (`summary` or `full`) override `remote.json` and the
stored token. `remote status` prints the URL, share level, token source
(`env`, `file` or `none`), reachability, and then the principal and scopes or the store's error; it exits 0 when
the store accepts the token and 1 otherwise. Its JSON is
`{"configured":true,"url":…,"share":…,"tokenSource":…,"reachable":BOOL,"principal":…,"scopes":[…],"error":…}`,
with `principal` and `scopes` null unless signed in and `error` null when signed in; a
refused token is `reachable: true` with an error, an unreachable store `reachable: false`.
Without a configured store it prints `No remote review store is configured.` (JSON
`{"configured":false}`) and exits 1. Requests use a 2 s connect and 5 s total timeout and never
follow redirects. `remote logout` deletes the stored token and `remote.json`; the
server keeps the token valid until `server token revoke`. Tokens are never printed
or logged.

With a store configured, `verify` looks up every review before running it and reuses
whichever result completed last, the local one or the store's. A store result is copied
into the local state with its `origin`, so `run show`, `cache` and the monitor show it as
reuse. While a review is still missing, or waits for a person, verify keeps checking the
store about once a second: a sign-off someone published elsewhere completes the waiting
request, whose own pending sign-off ends as ERROR (`SUPERSEDED`). Text output names the
source:

```text
  app/check [run-Hq2b9X-1]: GREEN (reused from remote: alice@laptop, run-x05qFq)
  brand/signoff [run-Hq2b9X-2]: GREEN (reused from remote: Human sign-off by alice, published by alice-signoff, run-Ksl1Qr)
```

The producer (`user@host`) and the Human reviewer are what the publishing machine
recorded; the publisher is the server-authenticated token name. An Agent result's
`producer` also carries `session`, the reference to its
[saved conversation](../features/agent.md#saved-conversations) on the producing
machine; the conversation itself is never published. Older clients ignore the field,
and the server stores records as they are, so the protocol and record `schema` (2) are
unchanged. A reused remote result's request shows the reference, and `session show`
on it names the machine and state where the conversation lives. When a GREEN or RED result with a reuse key is saved locally, verify publishes its
summary record, or the full record with share `full`. If publishing fails, the local
result stays and the failure is reported. A `--speculate` Run holds that automatic publication until its scheduling
finishes, even while Human reviews are still pending; it then publishes only the
results that survived, after blocked work has been withdrawn from reuse. Records carry the key, the Eval definition hash, the fingerprint of
each Artifact the key covers, its `artifactKinds` map (`folder`/`file`/`graph`), and the
execution `options` (backend, model,
reasoning, limits and profile variant) and an Agent result's `executionPaths` pins. `request submit` publishes Human sign-offs.
A token without `read` looks nothing up and one without `publish` publishes nothing,
so a read-only CI token only reuses. Human sign-offs also need `human`; with any
other token they stay local with a warning. Nothing is published for evals without a
reuse key. `--force` never reads
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

Connection errors, request timeouts and 5xx answers fail open: one warning, then the
process continues without the remote, reusing the local latest record of each key and
reviewing locally what has none. 401/403, TLS failures, other error answers, invalid
configuration, and a successful answer whose body cannot be read or decoded (a body
timeout included) fail closed. A 401/403 gets artifactize's own message that
the store rejected the token; another 4xx answer carries the store's own error message
when it sends one. Verification looks up remote evidence after durably accepting
the Run: a failed lookup leaves an ERROR Run, not a missing Run. Submit-only
`verify` exits 0 on acceptance; `verify --wait` observes that ERROR and exits 2.
Invalid configuration can still reject submission before acceptance. Synchronous
commands such as `status` report a fail-closed lookup with exit 2 directly.
A record that does not validate is ignored with a warning, and its eval is
reviewed locally.
