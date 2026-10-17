# Working as a team

Machines and CI can reuse each other's verdicts through one shared review store,
`artifactize server` ([design](https://github.com/lhj6102/artifactize/blob/main/docs/design/remote-store.md)). Each machine keeps its
own state. The store keeps every record of each reuse key and returns the latest
one, so a review done later (a forced re-review, say) replaces an earlier verdict
for everyone. The [team walkthrough](walkthrough.md)
runs two machines and CI end to end.

**Server.** Run the store on a small host, with its own state directory:

```sh
artifactize --state-dir /srv/artifactize server token add alice-laptop --scopes read,publish,human
artifactize --state-dir /srv/artifactize server token add bob-laptop --scopes read,publish
artifactize --state-dir /srv/artifactize server token add ci --scopes read
artifactize --state-dir /srv/artifactize server run     # http://127.0.0.1:8417/
```

`token add` prints each token once; keep it private. `server run` listens on
**loopback** (`127.0.0.1`): only programs on that same computer can connect directly.
To let teammates connect without sending tokens over an unencrypted network, use
[the secure connection setup below](#connect-teammates-securely).
Back up the store with the SQLite command-line tool. This complete example reads
the database from the state directory above and writes a separate backup file:

```sh
sqlite3 -readonly /srv/artifactize/review-store.sqlite ".backup '/srv/backups/artifactize-reviews.sqlite'"
```

Replace both paths for your host. The source must be the server's actual
`review-store.sqlite`; choose an existing, writable backup directory outside a
project checkout (`/srv/backups` in this example). `-readonly` prevents a mistyped
source path from silently creating a new empty source database.

Upgrade the server and its clients together. The 0.9 reuse-key v2 matches no older record, so every executable eval,
Human sign-offs included, needs one new review. Dependency evals have no store
records: each client derives them from current required evidence. See [Review store server](reference.md#review-store-server) for
the full reference.

Share reusable results through the team review store, then let [CI](ci.md) reuse
Agent and Human verdicts with a read-only token. The [walkthrough](walkthrough.md)
shows two machines and CI; the [reference](reference.md) covers server and client
commands, record formats and security.

## Connect teammates securely

A **TLS reverse proxy** is a server in front of the store that accepts encrypted
HTTPS connections and forwards them locally. A **tunnel** carries a connection
through another encrypted connection instead. Here is a complete SSH tunnel setup
for a teammate who already has an SSH account on the store's host:

1. Leave `artifactize server run` running on that host, as above.
2. On the teammate's computer, open a terminal and replace `you@reviews-host` with
   their SSH account and the host's name:

   ```sh
   ssh -N -L 127.0.0.1:18417:127.0.0.1:8417 you@reviews-host
   ```

   Keep this terminal open. `-N` starts no remote command; `-L` forwards local port
   18417 to the store's port 8417 through SSH. The local end is also restricted to
   this computer, and SSH encrypts the connection between the computers.
3. In another terminal on the teammate's computer, sign in through that tunnel:

   ```sh
   artifactize remote login http://127.0.0.1:18417/   # enter this machine's token
   artifactize remote status
   ```

Plain HTTP is allowed here because the address is local; network traffic travels
inside encrypted SSH. Do not expose the store's plain-HTTP port directly to the
network. Without a tunnel, clients require an HTTPS address served by a TLS reverse
proxy. Follow SSH's normal host verification and authentication; do not bypass it.

## Sign in and share results

Each machine signs in once. The token is read from stdin, never from
command arguments. Interactive input is hidden when the terminal supports it. On
Windows, if hiding input fails, the CLI warns before reading the token visibly;
on Linux and macOS, that failure aborts login. Piped input is read without changing
terminal modes:

```sh
artifactize remote login https://reviews.example/     # paste the token
artifactize remote status
```

From then on:

- `verify` reuses results from the store and publishes its own GREEN/RED results;
- `status` predicts remote reuse;
- `request submit` publishes Human sign-offs;
- `remote push` sends results produced while the store was unreachable.

See [Remote review store client](reference.md#remote-review-store-client) for the full reference.

**CI.** Configure CI through the environment only; see [CI](ci.md).

**Share levels.** `summary`, the default, sends:

- the verdict;
- the schema-validated owner fields (for runtime evals, only the exit code,
  duration and truncation flag);
- the reuse key, the Eval definition hash and the fingerprint of each Artifact
  the key covers, including each covered Artifact's `folder`/`file`/`graph` kind
  (`artifactKinds` in the record);
- the profile, its execution options (backend, model, reasoning, limits, variant),
  usage counters and an Agent result's workspace-relative `executionPaths` pins;
- the producer (`user@host`), the Human reviewer and timestamps;
- for an Agent result, the [reference](../agent.md#saved-conversations) to its
  saved conversation (`producer.session`: the producer, its state id, and the Run,
  request and session ids).

`summary` omits captured stdout/stderr, the separate invocation audit, local
repository provenance and the Agent conversation itself. It still sends the
complete declared runtime profile, including its command and arguments: any paths
or sensitive literals authored there are shared, not redacted. The
`executionPaths` pins also name workspace-relative paths.
`full` (`remote login --share full` or `ARTIFACTIZE_REMOTE_SHARE=full`) additionally
sends the saved execution as is, including captured output. Owner fields are free
text at both levels, so keep secrets out of shared declarations and the fields
that `pass_schema` and `fail_schema` allow.

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
  `.artf` declarations, so a cloned repository cannot send your token elsewhere.
  Tokens are stored 0600, bound to the store they were issued for, and never printed.
