# Shared remote review store (#45)

Status: approved by the owner on 2026-10-04 with the decisions in [section 9](#9-owner-decisions-2026-10-04). Implementation follows the plan in [section 8](#8-implementation-plan).

**Summary.** An optional HTTP store, `artifactize server`, holds one immutable, reduced record per (staleKey, Eval definition hash). Local `state.sqlite` keeps Runs and acts as a read-through/write-through cache. No remote claims; the first writer wins.

**Since 0.5** ([#92](https://github.com/lhj6102/artifactize/issues/92)), records are keyed by the reuse key, `hash(eval strategy, (name, fingerprint) of each Artifact the eval depends on)`, and the store (schema 3) appends every record of a key and answers lookups with the latest by `completedAt`. This replaces first-writer-wins. `verify` looks up every key once per Run and reuses whichever completed last, the local latest or the store's latest, falling back to the local latest while the store is unreachable; `--force` reads nothing from the store but publishes its results. Routes are `POST /v1/lookup` with `{"keys":[KEY,...]}`, `PUT /v1/entries/{key}` and `server rm KEY`, records are schema 2, and the 0.3 `staleKey` alias is gone. The rest of this document describes the 0.3 design.

## 1. Backend options

| | (a) `artifactize server` (HTTP + SQLite) | (b) S3-compatible bucket | (c) Postgres | (d) git ref in the project remote |
|---|---|---|---|---|
| Ops burden | One process and state dir on a small VM; TLS via proxy or tunnel; `sqlite3 .backup` | None if a bucket exists, else MinIO | Run or rent a DB reachable from laptops and CI | None; reuses the forge |
| Auth/ACL | Scoped bearer tokens (`read`/`publish`/`human`); server stamps the authenticated publisher; revoke with purge | IAM keys, prefix policies; publisher self-asserted in the object | DB roles, `current_user` stamping; one login per person | Push access = publish; author self-asserted; public repos expose verdicts |
| Atomic publish / claim | `INSERT … ON CONFLICT DO NOTHING`, as locally; leases easy if ever needed | `If-None-Match: *` create-only; leases need `If-Match` ETag CAS and trusted clocks; support varies across S3 clones | `ON CONFLICT`, advisory locks | Ref update is CAS; one ref contends, per-key refs bloat ref advertisement; no expiry, so no claims |
| Offline | Local execution; the server is a single point of failure | Same fallback; managed buckets rarely fail | Same fallback | Same fallback; fetched refs stay readable |
| Rust size | ~500 lines server, ~350 client | ~400 client (SigV4, conditional PUT) | ~300 client plus migrations; the schema becomes the public API | ~350, shelling out to `git`; needs a checkout with credentials |
| New deps | `axum` (hyper/tower already in the lock) | `rusty-s3` or `object_store` | `tokio-postgres` + rustls glue | none (`git` binary) |

**Recommendation: (a).** It is the only option where the server, not the client, records who published, which the trust model depends on (the Human scope, revocation with purge). Its semantics are today's local ones, so the server reuses store code and SQLite stays a regular local file. It adds one crate and looks up N keys in one request (S3 needs N GETs). The cost is running a process. (b) is the runner-up for teams that already have a bucket. (d) suits only small trusted teams: it ties sharing to one repository, losing today's cross-repository reuse.

## 2. What is shared

Remote record (`schema: 1`), default share level `summary`:

- `staleKey`, `evalDefHash`, `verdict` (GREEN/RED);
- `result`: for Agent and Human evals the schema-validated owner fields; for runtime evals only `verdict`, `exitCode`, `durationMs` and `truncated`;
- `profile` (declared effective profile, not resolved argv) and per-attempt `usage` (`turn`, `attempt` and the counters only, without provider error text), so #46 can report tokens saved;
- `evalId`, the source `runId`/`requestId`/`executionId` (pointers back to the producer's machine), and the Human `reviewer`;
- `producer` (`{"name":"user@host","version":"0.1.0"}`) and `publisher` (token name, stamped by the server);
- `startedAt`, `completedAt`, and `publishedAt` (server clock).

Summaries (max 256 KiB of JSON) never contain argv, stdout/stderr, tool-call audit or the local repository path. At share level `full` the record additionally carries `execution`, the saved `Execution` JSON as is, under the local 16 MiB cap; there is no separate share field, since `execution` is present exactly for full records. Owner fields can still hold free text, so owners keep secrets out of `passSchema`/`failSchema`.

Every new local execution records its `producer`; a submitted Human execution also records its `reviewer` (the claimant). Executions saved before these fields existed stay readable without them, and the state schema stays at `user_version` 1 because no table changes. A mirrored execution is a self-contained `executions` row with the id `remote-<executionId>`, an empty `provenance.repoPath` (summaries carry no path), and `origin: {store, publisher, publishedAt}`; a local entry for the same key always wins over a mirror, and mirrors are never published again.

## 3. Semantics

- **Lookup.** Local `cache_entries` first; on a miss, `POST /v1/lookup` with `{staleKey, evalDefHash}` keys just before `claim_execution`. A hit is mirrored into a self-contained `executions` row with an `origin` block (store, producer, publisher) plus a `cache_entries` row, then reused through `cache::reuse`, so `run show`, `cache`, GC and the monitor work unchanged. `status` looks up read-only, without mirroring.
- **Publication.** After `complete_execution` commits, and only if the local settle published a non-mirror entry, `PUT /v1/entries/{evalDefHash}/{staleKey}` runs outside any transaction. Existing rules carry over: only GREEN/RED, only after the staleKey recheck, nothing without a staleKey or under `--force`. Human results publish from `request submit`.
- **Conflicts.** Entries are immutable, and the first writer wins (a later PUT returns `created: false`). A local `cache rm` drops only the mirror. Removal from the store is `server rm` on the server host.
- **Unreachable remote: fail open.** Connection errors, timeouts (2 s connect, 5 s request) and 5xx fall back to local execution with one warning, and the process skips the remote afterwards. Executing is always correct, so a cache outage must not turn CI red. 401/403, TLS failures and invalid configuration fail closed: silently not sharing would hide a misconfiguration or an attack. `remote push` publishes entries produced offline.

## 4. Cross-machine claims

**Recommendation: accept duplicates.** Local claims still deduplicate within a machine. Remote claims would need leases, heartbeats, TTL tuning and stale-owner handling: the machinery PLAN dropped ("no lease timers", no Human reservation or expiry). A duplicate needs two machines to start one key before either publishes; the target flow (developer verifies and pushes, CI reuses) is sequential, and first-writer-wins keeps duplicates correct. Looking up again before each claim and on every `verify --wait` poll narrows the window. If #46's executed/reused numbers show real duplicate spend, add an advisory lease that correctness never depends on.

## 5. Human verdicts

**Recommendation: reuse them team-wide.** A sign-off is only as trustworthy as its publishing token, so the server accepts Human records only from `human`-scoped tokens; CI never gets that scope. Output shows both names: `brand/signoff: GREEN (reused; Human sign-off by alice, published by alice-laptop, 2026-10-04)`. `reviewer` is the self-asserted claim name, while `publisher` is authenticated. When a local WAITING_HUMAN execution's key appears remotely during `verify` or a `--wait` poll, the local request settles from the mirror, as followers do today.

## 6. Trust and security

- **Publishing** requires the `publish` scope; untrusted CI (fork PRs) gets `read` only. A publisher can assert any verdict for any key and the server cannot check it, so poisoning is contained by scopes, immutable entries, the server-stamped publisher and `server token revoke NAME --purge` (deletes that token's entries). `--force` bypasses a suspect entry.
- **Tokens.** The server stores only SHA-256 hashes. Clients store the token in `$STATE/auth/remote-token.json` through `auth::storage::Storage` (0700 directory, 0600 single-link file, `O_NOFOLLOW`, atomic persist), with its ChatGPT-specific messages generalised. CI uses `ARTIFACTIZE_REMOTE_TOKEN`. A token is bound to the origin it was issued for.
- **The remote is never configured from `artifactize.json`**, because a cloned repository naming a store would receive the user's token.
- **Transport.** Clients require HTTPS with the system trust store (existing reqwest/rustls), and allow `http://` only for loopback. `server run` binds `127.0.0.1` by default and sits behind a TLS proxy or tunnel.

## 7. Configuration and CLI

`$STATE/remote.json` holds `{"url":"https://reviews.example/","share":"summary"}`. Environment overrides are `ARTIFACTIZE_REMOTE` (a URL, or `off`), `ARTIFACTIZE_REMOTE_TOKEN` and `ARTIFACTIZE_REMOTE_SHARE`.

- **Client.** `remote login URL` (token from stdin, never argv; verified with `GET /v1/whoami`), `remote logout`, `remote status` (URL, share level, token source, reachability, principal, scopes), `remote push [--dry-run]` (re-sends local non-mirror entries; existing keys are no-ops).
- **Server.** `server run [--listen ADDR]`, `server token add NAME --scopes read,publish[,human]` (prints the token once), `server token list|revoke NAME [--purge]`, `server rm STALE_KEY [EVAL_HASH]`, all on `--state-dir` with a separate `review-store.sqlite`, LRU-capped like the local cache.
- **`doctor` stays offline.** It reports URL, share level and token presence; a plain-HTTP non-loopback URL or unsafe token file is a hard error, a missing token a warning. Reachability belongs to `remote status`.

## 8. Implementation plan

1. **Producer and record.** `producer` on new executions, `origin` on mirrors, summary/full projection. Check: a runtime summary has no stdout/stderr/tool calls, and a mirrored execution renders in `cache show`.
2. **`artifactize server`.** axum routes (`whoami`, `lookup`, `PUT entries`), scoped tokens, revoke/purge, `server rm`, size caps, LRU. Check: a second PUT of a key loses, a `read` token gets 403 on publish, and a Human record without `human` scope is rejected.
3. **Client configuration and auth.** `remote.json`, environment overrides, the 0600 token, `remote login/logout/status`, HTTPS-or-loopback, `doctor`. Check: `doctor` reports a 0644 token as a hard error without opening a socket.
4. **Read-through/write-through.** Lookup in `verify` and `status`, publication in `verify` and `request submit`, Human settlement, fail-open. Check: with two state dirs and one server, A executes, then B reuses with 0 executions and A as producer; with the server stopped B executes and warns; `--force` makes no remote call.
5. **`remote push`, README/PLAN, two-machine example.** Check: an entry produced offline appears remotely after `remote push`.

## 9. Owner decisions (2026-10-04)

1. **Backend:** `artifactize server`, an HTTP server over its own SQLite file, using axum, with scoped bearer tokens `read`/`publish`/`human`. The server stamps the authenticated publisher. Tokens are revocable with `--purge`.
2. **Default share level:** `summary`. `full` is opt-in.
3. **Unreachable remote:** fail open (run locally, warn once) on network errors, timeouts and 5xx. Fail closed on 401/403, TLS and configuration errors.
4. **Cross-machine claims:** none. Accept duplicates, first writer wins, look up again before each local claim and on each `--wait` poll.
5. **Human sign-offs:** reusable team-wide, accepted only from `human`-scoped tokens. Output shows the reviewer and the publisher.
6. **Publishing:** scoped tokens; untrusted CI gets read-only.
7. **TLS:** the server binds 127.0.0.1 by default, with a proxy or tunnel in front. Clients require HTTPS except on loopback.
8. **Remote configuration:** only in the state dir (`$STATE/remote.json`) and env (`ARTIFACTIZE_REMOTE`, `ARTIFACTIZE_REMOTE_TOKEN`, `ARTIFACTIZE_REMOTE_SHARE`). Never in `artifactize.json`.
9. **Naming:** the owner-computed reuse key is the **staleKey** throughout: remote records (`staleKey`), routes (`{staleKey, evalDefHash}` lookup keys, `PUT /v1/entries/{evalDefHash}/{staleKey}`) and `server rm STALE_KEY [EVAL_HASH]`, and locally the `staleKey` declaration, `cache show/rm STALE_KEY [EVAL_HASH]` and the state database columns.
   Since 0.4 that key is the **fingerprint** in all of these places ([#72](https://github.com/lhj6102/artifactize/issues/72)); the server still accepts a 0.3 client's `staleKey` through 0.4.x.
   Since 0.5 the key is the reuse key described in the summary, and 0.3 and 0.4 clients are told to upgrade.
