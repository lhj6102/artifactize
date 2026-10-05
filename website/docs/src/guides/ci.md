# CI

Run `artifactize verify --all` as a CI step. It exits 0 only when every selected
eval is GREEN, so RED (1), ERROR (2) and INCOMPLETE (4) fail the job; the
[command reference](../reference/cli.md#command-reference) lists every exit code.
A Human eval leaves the Run INCOMPLETE until someone signs off, so CI passes only
when it can reuse a sign-off, which needs a fingerprint.

## Reuse verdicts from the team store

**CI.** Configure CI through the environment only:

```yaml
- run: artifactize verify --all
  env:
    ARTIFACTIZE_REMOTE: https://reviews.example/
    ARTIFACTIZE_REMOTE_TOKEN: ${{ secrets.ARTIFACTIZE_READ_TOKEN }}
```

- A read token reuses but never publishes.
- An outage never turns CI red: `verify` warns once and reviews locally.
- A rejected token, a TLS failure or an invalid configuration fails the job (exit 2),
  so a misconfiguration is never skipped silently.
- `ARTIFACTIZE_REMOTE=off` disables the store for one command.

The [team walkthrough](team-walkthrough.md#5-ci-reuses-both) runs this end to end.

## Test Agent evals without a model

A CI job can run Agent evals against a fake provider that it starts on loopback, with
a throwaway state directory and the store off. See
[Test against a fake provider](agent-evals.md#test-against-a-fake-provider).
