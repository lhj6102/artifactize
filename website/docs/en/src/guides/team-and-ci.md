# Team Review Store and CI

A team shares reusable verdicts across developer machines and CI through one self-hosted review store (`artifactize server`). Each machine keeps its own local state and caches; the store holds completed records keyed by [reuse key](../concepts/reuse.md#the-reuse-key).

## 1. Run the review store server (`artifactize server`) {#review-store-server}

On your server host, create one bearer token per teammate or CI job and start the store in its own state directory:

```sh
artifactize --state-dir /srv/artifactize server token add alice --scopes read,publish,human
artifactize --state-dir /srv/artifactize server token add bob   --scopes read,publish
artifactize --state-dir /srv/artifactize server token add ci    --scopes read
artifactize --state-dir /srv/artifactize server run --listen 127.0.0.1:8417
```

- **Token scopes**: `--scopes` is required and takes a comma-separated list of `read` (look up records), `publish` (upload `runtime` and `agent` verdicts), and `human` (upload Human sign-offs; requires `publish`). `token add` prints the secret token once.
- **Managing tokens and records**:
  ```sh
  artifactize --state-dir /srv/artifactize server token list
  artifactize --state-dir /srv/artifactize server token revoke alice [--purge]  # --purge deletes records alice published
  artifactize --state-dir /srv/artifactize server rm REUSE_KEY                  # delete records for one 64-hex key
  ```
- **HTTPS or SSH tunnel**: `server run` listens on plain HTTP (`127.0.0.1:8417` by default), while clients require `https://` for any non-loopback address. Place a TLS reverse proxy in front of the server, or connect teammates over an SSH local port forward (`ssh -N -L 127.0.0.1:18417:127.0.0.1:8417 user@reviews-host` and log in to `http://127.0.0.1:18417/`), then run `artifactize remote status` to verify the connection.

## 2. Sign in on developer machines (`artifactize remote`) {#remote-review-store-client}

Configure the store client in your local state (or in the desktop app under **Settings → Remote**), never in `.artf` files:

```sh
printf '%s\n' "$TOKEN" | artifactize remote login https://reviews.example/
artifactize remote status
artifactize remote push --dry-run
artifactize remote push
artifactize remote logout
```

Pass `--share full` to `remote login` if you want to share full execution records instead of the default `summary` level.

- **Automatic lookup and publish**: when signed in with the corresponding token scopes, `verify` looks up and publishes records for evals that have a reuse key (see [Completed result reuse](../concepts/reuse.md#completed-result-reuse) for how local and remote records are selected and how `--force` disables remote lookups). `request submit` publishes Human sign-offs when the token has `human`+`publish`, and an open non-forced Run waiting on a Human sign-off settles the waiting request automatically when a matching sign-off is published elsewhere, even without a CLI watcher attached.
- **Share levels (`summary` vs. `full`)**: `summary` (the default) lets teammates see the verdict, owner result fields, reuse key, fingerprints, declared profile (including runtime `command` and `args`), declared tool/session `execution_paths` digests, token usage, author/reviewer names, and Agent session reference, while leaving out the local repository root, captured `stdout`/`stderr`, and the full Agent conversation transcript. `full` (`--share full`) also shares the local repository root and captured runtime output (see [`remote` in the CLI Reference](../reference/cli-and-mcp.md#command-reference) for the exact record fields). Because `summary` still shares declared `execution_paths` and any text written in declared commands, `args`, or owner result fields, never put secrets in declared arguments or result fields.
- **Offline resilience and `remote push`**: if the store is unreachable or returns HTTP 5xx, `verify` warns once and continues with local state; an authentication (401/403), TLS, or protocol error fails the Run (`ERROR`, exit `2` with `verify --wait` or `run show RUN_ID --wait`) so you can fix the connection or re-run `artifactize remote login` (remote records whose criteria or fingerprints do not match your local project are ignored in favor of valid local cache entries or fresh execution when allowed by `--reuse-only`). Run `artifactize remote push --dry-run` to preview uploads and `artifactize remote push` to upload the latest locally produced `GREEN` or `RED` verdict per reuse key across your state directory.
- **Environment overrides**: `ARTIFACTIZE_REMOTE` (URL or `off`), `ARTIFACTIZE_REMOTE_TOKEN`, and `ARTIFACTIZE_REMOTE_SHARE` (`summary` or `full`) override local settings.

## 3. Configure CI (`--reuse-only agent,human`) {#ci}

CI should run deterministic runtime checks (tests, linters, builds) and reuse developers' Agent reviews and Human sign-offs rather than calling LLM providers or waiting for a person.

Give CI a token with the **`read`** scope only and run `artifactize verify --all --wait --reuse-only agent,human`:

```yaml
# Inside .github/workflows/ci.yml jobs.<job>.steps:
- run: artifactize verify --all --wait --reuse-only agent,human
  env:
    ARTIFACTIZE_REMOTE: https://reviews.example/
    ARTIFACTIZE_REMOTE_TOKEN: ${{ secrets.ARTIFACTIZE_READ_TOKEN }}
```

With `--reuse-only agent,human`, CI runs `runtime` checks (or reuses cached ones) and never executes `agent` or `human` checks, reusing the latest matching verdict from the team store or local cache ([Fingerprints and Reuse](../concepts/reuse.md#completed-result-reuse)). If a `--reuse-only` check that is not already blocked by an upstream `RED` prerequisite has no matching cached verdict (for example, because a sign-off is still pending at `YELLOW` or files changed after review), that check stays `STALE` without executing, dependents gated on it do not run, and the Run fails as **`INCOMPLETE`** (exit `4`; see [Sign-offs and the end of a Run](../concepts/runs.md#sign-offs-and-the-end-of-a-run) for Run outcome precedence).
