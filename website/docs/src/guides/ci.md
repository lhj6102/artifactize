# CI

Continuous integration (CI) runs automated checks when you submit project changes,
helping your team decide whether those changes are ready to join the shared project.

Run `artifactize verify --all --wait --reuse-only agent,human` as a CI step: tests run, while reviews must come from the cache or
the team store, so CI never calls a model or waits for a person. It exits 0 only
when every selected eval is GREEN, so RED (1), ERROR (2) and INCOMPLETE (4) fail the
job; the [command reference](../reference/cli.md#command-reference) lists every exit code. An Agent review or Human sign-off with
nothing to reuse is [not executed](../reference/cli.md#reuse-only) and leaves the Run INCOMPLETE, so CI passes only once
a developer's `verify` has produced it for the current key. Artifactsum enables
reuse by default; `fingerprint = false` on a target or dependency disables it.

Dependency evals still derive current readiness under `--reuse-only agent,human`; they run nothing
and are never cached or fetched from the store themselves. A missing or waiting
required review leaves them WAIT_DEPENDENCY; a required RED makes them BLOCKED.
`--reuse-only dependency` does not prevent derivation.

## Keep deferred work out of the merge gate

Developers may use `--defer-human` to finish machine work while a person is away,
optionally with `--speculate`. SIGNOFFS_PENDING (exit 5), DEFERRED sign-offs and
PROVISIONAL machine results are not approval. Keep CI's
`--reuse-only agent,human` gate: missing sign-offs are not executed, leave
INCOMPLETE, and cannot be counted as passing even if `--defer-human` is supplied.
Once a person submits GREEN, CI reuses unchanged machine evidence and the
sign-off. RED blocks dependents. Do not treat plain verify acceptance as CI success.

## Reuse verdicts from the team store

Configure the team store through environment variables. This example is a GitHub
Actions step: add it under `steps:` in the job of your `.github/workflows/*.yml`
workflow file, after installing Artifactize. Create a GitHub Actions secret named
`ARTIFACTIZE_READ_TOKEN` for the store's read token.

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
- `ARTIFACTIZE_REMOTE=off` disables the store for one command.

The [team walkthrough](team-walkthrough.md#5-ci-reuses-both) runs this end to end.

## Test Agent evals without a model

A CI job can run Agent evals against a fake provider that it starts on loopback, with
a throwaway state directory and the store off. See
[Test against a fake provider](agent-evals.md#test-against-a-fake-provider).
