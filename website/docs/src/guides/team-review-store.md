# Team review store

Machines and CI can reuse each other's verdicts through one shared review store,
`artifactize server` ([design](https://github.com/lhj6102/artifactize/blob/main/docs/design/remote-store.md)). Each machine keeps its
own state. The store keeps every record of each reuse key and returns the latest
one, so a review done later (a forced re-review, say) replaces an earlier verdict
for everyone. The [team walkthrough](team-walkthrough.md)
runs two machines and CI end to end.

**Server.** Run the store on a small host, with its own state directory:

```sh
artifactize --state-dir /srv/artifactize server token add alice-laptop --scopes read,publish,human
artifactize --state-dir /srv/artifactize server token add bob-laptop --scopes read,publish
artifactize --state-dir /srv/artifactize server token add ci --scopes read
artifactize --state-dir /srv/artifactize server run     # http://127.0.0.1:8417/
```

`token add` prints each token once. `server run` binds loopback by default. Put a
TLS reverse proxy or a tunnel in front, because clients require HTTPS except on
loopback. Back up `review-store.sqlite` with `sqlite3 .backup`. Upgrade the server and its
clients together: a 0.5 server drops the records 0.4 stored, whose keys no 0.5
client computes, and tells older clients to upgrade. See
[Review store server](../reference/review-store.md#review-store-server) for the full reference.

**Clients.** Each machine signs in once. The token is read from stdin and is not
echoed:

```sh
artifactize remote login https://reviews.example/     # paste the token
artifactize remote status
```

From then on:

- `verify` reuses results from the store and publishes its own GREEN/RED results;
- `status` predicts remote reuse;
- `request submit` publishes Human sign-offs;
- `remote push` sends results produced while the store was unreachable.

See [Remote review store client](../reference/review-store.md#remote-review-store-client) for the full reference.

**CI.** Configure CI through the environment only; see [CI](ci.md).

**Share levels.** `summary`, the default, sends:

- the verdict;
- the schema-validated owner fields (for runtime evals, only the exit code,
  duration and truncation flag);
- the reuse key, the Eval definition hash and the fingerprint of each Artifact
  the key covers;
- the profile, its execution options (backend, model, reasoning, limits, variant)
  and usage counters;
- the producer (`user@host`), the Human reviewer and timestamps.

It never sends argv, stdout/stderr, the tool-call audit or repository paths.
`full` (`remote login --share full` or `ARTIFACTIZE_REMOTE_SHARE=full`) also sends
the saved execution as is, including captured output. Owner fields are free text
at both levels, so keep secrets out of the fields that `passSchema` and
`failSchema` allow.

**Trust.**

- The server stamps the authenticated `publisher`, the token name. The `producer`
  and the Human `reviewer` are what the publishing machine claims.
- Any `publish` token can assert any verdict for any key. Give untrusted CI (fork
  pull requests) `read` only, and `human` only to people who sign off. If a token
  leaks or is misused, `server token revoke NAME --purge` also deletes the
  entries it published.
- `verify --force` reads nothing from the store, which bypasses a suspect record,
  and publishes its fresh result, which becomes the store's latest.
  `cache rm` drops a local key's records, and `server rm` removes a key's records
  from the store.
- Only `$STATE/remote.json` and the environment configure the store, never
  `artifactize.json`, so a cloned repository cannot send your token elsewhere.
  Tokens are stored 0600, bound to the store they were issued for, and never printed.
