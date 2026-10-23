# CI

**Continuous integration (CI)** automatically runs checks when a team pushes code
or opens a pull request. A CI job is a sequence of commands on a runner machine;
it should reuse a developer's Agent/Human approvals, not make those decisions.

Run `artifactize verify --all --wait --reuse-only agent,human` as a CI step: tests run, while reviews must come from the cache or
the team store, so CI never calls a model or waits for a person. It exits 0 only
when every selected eval is GREEN, so RED (1), ERROR (2) and INCOMPLETE (4) fail the
job; the [command reference](../../cli/index.md#command-reference) lists every exit code. An Agent review or Human sign-off with
nothing to reuse is [not executed](../../cli/index.md#reuse-only) and leaves the Run INCOMPLETE, so CI passes only once
a developer's `verify` has produced it for the current key. Artifactsum enables
reuse by default; `fingerprint = false` on a target or dependency disables it.

## Keep pending sign-offs out of the merge gate

A developer's `verify` finishes machine work while a person is away, since sign-offs
never hold it up. YELLOW (exit 5), DEFERRED sign-offs and PROVISIONAL machine results
are not approval. Keep CI's `--reuse-only agent,human` gate: missing sign-offs are not
requested, leave INCOMPLETE, and cannot be counted as passing.
Once a person submits GREEN, CI reuses unchanged machine evidence and the
sign-off. RED blocks dependents. Do not treat plain verify acceptance as CI success.

## Reuse verdicts from the team store

The YAML below is **one GitHub Actions step**, not a complete workflow. Put it in
an existing `.github/workflows/NAME.yml` file under `jobs.YOUR_JOB.steps`, after
checking out the reviewed commit and making `artifactize` available on the runner
(see [Installation](../../getting-started/install.md)). GitHub's
[workflow guide](https://docs.github.com/en/actions/writing-workflows/quickstart)
explains creating the surrounding workflow and job if this is your first one.

Ask the store administrator for a token with **read** scope only. In the GitHub
repository, open **Settings → Secrets and variables → Actions**, add a repository
secret named `ARTIFACTIZE_READ_TOKEN`, and enter that token as its value. Replace
the example URL below with your team's reachable store URL. The `${{ secrets... }}`
expression supplies the secret at runtime; never paste the token into this file.
Configure other CI services through their equivalent secret/environment settings.

```yaml
- run: artifactize verify --all --wait --reuse-only agent,human
  env:
    ARTIFACTIZE_REMOTE: https://reviews.example/
    ARTIFACTIZE_REMOTE_TOKEN: ${{ secrets.ARTIFACTIZE_READ_TOKEN }}
```

- A read token reuses but never publishes.
- An outage is not an error: `verify` warns once and goes on with the local cache.
  A review that only the store holds is then not reused, so the Run is INCOMPLETE.
- A rejected token, a TLS failure or an invalid configuration fails the job (exit 2),
  so a misconfiguration is never skipped silently.
- `ARTIFACTIZE_REMOTE=off` disables the store for one command. A `remote.json` that
  cannot be read still fails the command: fix or remove it first.

The [team walkthrough](walkthrough.md#5-ci-reuses-both) runs this end to end.

## Test Agent evals without a model

A CI job can run Agent evals against a fake provider that it starts on loopback, with
a throwaway state directory and the store off. See
[Test against a fake provider](../agent.md#test-against-a-fake-provider).
