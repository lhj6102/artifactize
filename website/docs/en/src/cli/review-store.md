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
[CLI identity rules](index.md#identity-arguments) give the syntax.

`token add` prints a random bearer token once; the store cannot show it again, so save it then.
`--scopes` is required and has no default; omitting it exits 2. Scopes are `read`
(look up), `publish` (publish runtime and Agent records) and `human` (additionally publish
Human sign-offs, together with `publish`). Give
untrusted CI `read` only. `token list` shows names, scopes and creation/revocation
times, never tokens. `token revoke NAME` rejects the token at once; `--purge`
also deletes every record it published, and may be repeated later. Revocation always
prints a JSON report, even without `--json`:
`{"name":"alice-laptop","revokedAt":"2026-10-06T09:14:02Z","purgedEntries":0}`, where
`purgedEntries` counts the records `--purge` deleted. Names of revoked tokens are never reused. `rm KEY` deletes every record of a key and prints
`{"removed":true}` (false if absent).

`artifactize` clients are the only writers: `verify`, `request submit` and `remote push`
publish, and `verify` and `status` look up. The server answers them over these routes,
useful for proxy and firewall rules. Each takes `Authorization: Bearer TOKEN` and answers
JSON, `{"error":...}` on failure, 401 for a missing, unknown or revoked token and 403 for a
missing scope:

| Route | Scope | Used by |
|---|---|---|
| `GET /v1/whoami` | any | `remote login`, `remote status` |
| `POST /v1/lookup` | `read` | `verify`, `status`, `remote push` |
| `PUT /v1/entries/{key}` | `publish` (+ `human` for Human sign-offs) | `verify`, `request submit`, `remote push` |

A record names the eval, its verdict, the fingerprints it covered, the Run that produced
it and the producer. A summary record (share `summary`) also carries the eval's declared
profile, including a runtime eval's command and arguments, but no captured output, no
record of what the command actually ran and no repository path; a full record (share
`full`) adds the saved execution with its output. Keep secrets out of declared arguments
if you share summaries. A record is at most 256 KiB as a summary and 16 MiB as a full
record. The server refuses a record that is malformed, has no completion time or is sent
for another key than its own, and stamps the publishing token's name and its own time.
Each client then checks that a record really matches the criteria and fingerprints it
asked for and ignores one that does not, with a warning, so a store cannot make a client
reuse a result for different criteria or inputs. To see what is shared, use
`artifactize cache list` on a client; to remove a key's records, `artifactize server rm
KEY`. Lookups count as use, and the store removes the least recently used records above
100,000 records or 4 GiB.

**Upgrading.** Upgrade the server and its clients together. A store from 0.3 or 0.4
keeps its tokens but drops its records, so clients review once more and publish again;
0.3 and 0.4 clients are told to upgrade.

Records from before 0.9 are not reused; see
[Upgrading to 0.9](../getting-started/update.md#upgrading-to-09). Local state upgrades
separately; see [State](../features/others/state.md#state).

## Remote review store client

```sh
printf '%s\n' "$TOKEN" | artifactize remote login https://reviews.example/ [--share full]
artifactize remote status [--json]
artifactize remote push [--dry-run] [--json]
artifactize remote logout
```

A client is configured only through the state directory and the environment,
never `.artf` declarations, so a cloned repository cannot redirect a token. `remote
login URL` reads the token from stdin, never argv: paste it as one line. A token over 4096
bytes is refused. It attempts to hide
terminal input; on Windows, if that fails, it warns that the token will be visible
and continues reading. Non-terminal stdin is read without changing terminal echo.
It verifies the token with `GET /v1/whoami`, then stores it in
`$STATE/auth/remote-token.json`, which only your account can read, and writes
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
recorded; the publisher is the server-authenticated token name. An Agent result also
carries a reference to its [saved conversation](../features/agent.md#saved-conversations)
on the producing machine; the conversation itself is never published, and `session show`
on the reused request names the machine and state where it lives. Older clients work with
the same store without upgrading.

**What is published.** When a GREEN or RED result with a reuse key is saved locally,
verify publishes its summary record, or the full record with share `full`; if publishing
fails, the local result stays and the failure is reported. `request submit` publishes
Human sign-offs. A `--speculate` Run publishes only after its scheduling finishes, and only
the results that survived ([speculation](../features/run.md#speculation)).
A token works within its [scopes](#review-store-server): without `read` it looks nothing
up, without `publish` it publishes nothing, so a read-only CI token only reuses, and
without `human` sign-offs stay local with a warning. Evals without a reuse key publish
nothing, and a [`--force`](../features/fingerprint.md#completed-result-reuse) result is
published and becomes the latest. `status` makes the same comparison with a read-only
lookup, so its `reuse` prediction includes newer remote results; `status --force` makes no
remote call.

`remote push` sends, for each key, the latest GREEN/RED record this machine
produced: results produced while the store was unreachable, before `remote login`
or with `ARTIFACTIZE_REMOTE=off`. It never sends mirrors. A token with `read` first looks up
the store's latest record of each key, and a record that already is the latest is
not sent again. The store keeps each execution once, so a record it already holds
answers `created: false`. Both count as `existing`, and pushing twice is harmless.
Human sign-offs without the `human` scope, and records over their size limit, are
`skipped` with a reason on stderr. The output is `Pushed N, already in the store M, skipped K.`, or JSON
`{"dryRun":false,"pushed":N,"existing":M,"skipped":K}`. `--dry-run` sends nothing and
prints `Would push N, already in the store M, skipped K.` (JSON `"dryRun":true`). Unlike `verify`, `push`
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
