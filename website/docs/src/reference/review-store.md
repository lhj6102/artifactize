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
artifactize server rm FINGERPRINT [EVAL_HASH]
```

`artifactize server` keeps a [shared remote review store](https://github.com/lhj6102/artifactize/blob/main/docs/design/remote-store.md)
in its own `review-store.sqlite` under `--state-dir`, separate from `state.sqlite`.
It holds one immutable record per (Eval definition hash, fingerprint); the first
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
tokens are never reused. `rm` deletes a fingerprint's entry, requiring the hash when
the fingerprint has several definitions.

The API takes `Authorization: Bearer TOKEN` and answers JSON (`{"error":...}` on
failure; 401 for a missing, unknown or revoked token, 403 for a missing scope):

| Route | Scope | Result |
|---|---|---|
| `GET /v1/whoami` | any | `{"principal":NAME,"scopes":[...]}` |
| `POST /v1/lookup` with `{"keys":[{"fingerprint","evalDefHash"}]}` (at most 1000) | `read` | `{"entries":[record,...]}` for the found keys |
| `PUT /v1/entries/{evalDefHash}/{fingerprint}` with a record | `publish` (+ `human` for Human records) | 201 `{"created":true}`, or 200 `{"created":false}` when the key exists |

The server checks a record's envelope (schema 1, the path's fingerprint and hash,
a GREEN/RED verdict and a profile kind) and size: 256 KiB for a summary, 16 MiB
for a full record carrying `execution`. It stamps `publisher` (the token name) and
`publishedAt` (server clock), replacing any client values. Lookups update last
use; inserts evict least-recently-used entries above 100,000 entries or 4 GiB.

A 0.3 client calls the fingerprint `staleKey`. Throughout 0.4.x the server accepts
that name as an alias in published records (also inside a full record's
`execution`) and in lookup keys, stores records under `fingerprint`, and answers a
lookup whose keys use `staleKey` with records in the 0.3 shape. A mixed team can
therefore upgrade the server first and then one machine at a time; a 0.4 client
needs a 0.4 server. The alias is removed in 0.5.0. A schema 1 `review-store.sqlite`
is upgraded in place to schema 2 when the server opens it.

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
publishes a GREEN/RED with a fingerprint to the local cache (after the fingerprint
recheck), verify sends its summary record, or the full record with share `full`,
outside any database transaction. `request submit` publishes Human sign-offs.
A token without `read` looks nothing up and one without `publish` publishes nothing,
so a read-only CI token only reuses. Human sign-offs also need `human`; with any
other token they stay local with a warning. Nothing is published for evals without a
fingerprint, and `--force` makes no remote calls at all. `status` looks up read-only,
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
