# Team review store

Machines and CI can reuse each other's verdicts through one shared review store,
`artifactize server` ([design](https://github.com/lhj6102/artifactize/blob/main/docs/design/remote-store.md)). Each machine keeps its
own state. The store holds one immutable record per (fingerprint, Eval definition
hash), and the first writer wins. The [team walkthrough](team-walkthrough.md)
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
loopback. Back up `review-store.sqlite` with `sqlite3 .backup`. Upgrade the server
before its clients: a 0.4 server keeps serving 0.3 clients, so machines can follow
one at a time. See
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
- the declared profile and usage counters;
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
- `verify --force` makes no remote call, which bypasses a suspect entry.
  `cache rm` drops a local mirror, and `server rm` removes the entry from the store.
- Only `$STATE/remote.json` and the environment configure the store, never
  `artifactize.json`, so a cloned repository cannot send your token elsewhere.
  Tokens are stored 0600, bound to the store they were issued for, and never printed.
