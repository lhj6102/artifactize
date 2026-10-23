# Cache and maintenance

Run `artifactize prune --older-than 30d --dry-run` to list the finished Runs older than
30 days whose output would be removed (`wouldRemove`), then again without `--dry-run` to
remove it.
This page covers that and the other commands that inspect and remove reusable results and
check this machine's setup. [State, cache and limits](../features/others/state.md) explains
what they work on.

## Cache inspection

```sh
artifactize cache list [--history] [--json]
artifactize cache show KEY [--history]
artifactize cache rm KEY
```

These commands use the shared state or `--state-dir PATH` and need no repository. Each
reuse key keeps every completed GREEN/RED record, and the latest is the one `verify`
reuses. `list` shows the latest record of each key: key, eval, verdict, completion time,
producer, repository (or the remote store it came from), how many records the key holds,
their size and last use. Its JSON also has the eval definition hash, the target's
fingerprint and the profile variant. `--history` lists every record, latest first.

`show` prints the key's latest record as JSON: result, the actual `profile`, `options`
(backend, model, reasoning, limits and variant), `fingerprints` of each Artifact the key
covers, `artifactKinds` (each covered name with `folder`, `file` or `graph`), usage and
`producer` (`user@host` and artifactize version, plus the `reviewer` of a Human result).
Records from a [shared remote review store](review-store.md) carry an `origin`
(store, publisher, publication time). A missing key prints `null` and exits 4;
`--history` prints all records as an array. Reading changes nothing.

`rm` stops every record of the key from being reused and prints `{"removed":true}`
(false if absent); saved Runs keep their results. It refuses a key that is being reviewed
right now. Keys come from `cache list` or from a request's `key` in `verify --wait --json`
and `run show`. How keys are built and what a hit returns is in
[Completed result reuse](../features/fingerprint.md#completed-result-reuse).

## Local diagnostics and maintenance

```sh
artifactize doctor [--repo PATH] [--state-dir PATH] [--json]
artifactize models PROVIDER [--json]
artifactize login openai|anthropic|gemini|codex [--json]
artifactize logout openai|anthropic|gemini|codex [--json]
artifactize provider list [--json]
artifactize provider add NAME (--alias PROVIDER | --api API --url URL [--key]) [--json]
artifactize provider remove NAME [--json]
artifactize prune [--older-than 7d] [--dry-run] [--state-dir PATH] [--json]
```

`prune` works on the selected state's Runs across repositories, not only the current one.
It removes the output of Runs that ended **GREEN, RED, ERROR or INCOMPLETE** and have
nothing still running. SIGNOFFS_PENDING Runs are **always skipped**, even after their
sign-offs are submitted, and so are unfinished Runs. `--older-than` compares the Run's
end time, as a whole number of `s`, `m`, `h`, `d` or `w`; without it every eligible Run is
pruned. `--dry-run` returns `wouldRemove` without deleting; a real prune returns
`removed`; both include `skippedRuns`. `prune` also cleans the
[Agent session store](../features/others/state.md#agent-sessions) down to its size bounds, whatever `--older-than`
says: `removedSessions` lists what it deleted, and `--dry-run` lists
`wouldRemoveSessions`. A dry run does not know which sessions a running `session send`
uses, so the real prune can delete fewer.

## Doctor, models and prune

`doctor` calls no provider and creates no Run, verdict, cache entry or sign-in. It
reports the state directory, checks that it is writable and outside a workspace, and
checks that the state database has a supported schema, upgrading it
[if needed](../features/others/state.md#state); an unsupported schema is a hard error that shows its `schema` and
the `supported` one. `--repo` also runs the checks of `config check`. Further checks:

- `limits`: the [backend capacity](../features/others/state.md#backend-capacity); an invalid `limits.json` is a hard
  error.
- `sessions`: the [Agent session store](../features/others/state.md#agent-sessions): how many sessions and bytes,
  `maxBytes`, `targetBytes`, `enabled`, its `directory` and the state's `stateId`.
- `openai`, `anthropic`, `gemini`: each API key reported as present in its variable, saved,
  or absent, never checked or printed; a missing key is a warning, since you need only the
  backends you use. A
  [test endpoint](../features/agent.md#test-against-a-fake-provider) is a warning that names it as
  `testEndpoint`; an invalid one is a hard error.
- `codex`: the sign-in, read without refreshing or going online. No sign-in is a warning;
  a stored sign-in passes even when its access token expired (the next use refreshes it);
  an expired `ARTIFACTIZE_CODEX_AUTH_FILE` token is a warning. A `$STATE/auth`
  [inside a repository](../features/others/state.md#state) is a warning, with the refusal message in `refused`, until a
  sign-in is stored there, then a hard error. Details name the `source` (`stored`, `file`
  or `none`), `expiresAt`, `expired` and any test endpoints.
- `providers`: the [custom providers](../features/agent.md#custom-providers) of this machine,
  each with what it reaches, never a key. A `$STATE/auth` inside a repository is a warning
  while nothing is saved there, then a hard error.
- The remote review store, also offline: its URL, share level and token source. A missing
  token is a warning; an invalid configuration (including plain HTTP to a non-loopback
  host) or an unsafe token file is a hard error.

Exit is 0 without hard errors, 1 with hard errors, and 2 for invocation errors.

`models PROVIDER` takes any name a profile's `backend` may use. `models openai`,
`models anthropic` and `models gemini` list the models the API key can use; `models codex`
lists those of the signed-in [Codex](../features/agent.md#codex) account; a custom provider lists
what its alias or endpoint offers. A test endpoint replaces the provider here too, and there
is no fallback to another account or backend. Text lists tab-separated slug/name pairs; JSON
is `{"backend":"openai","models":[{"slug":"model-id","display_name":"model-id"}]}`, in the
provider's order.

## Keys and custom providers

`login openai|anthropic|gemini` reads an API key from stdin (hidden when typed in a
terminal) and saves it; `logout` with the same name forgets it. The key's variable, when
set, is still used first. `login codex` and `logout codex` are the
[Codex sign-in](../features/agent.md#codex).

`provider add NAME` saves a [custom provider](../features/agent.md#custom-providers), or
replaces the one with that name: `--alias PROVIDER` for a built-in provider under another
name, or `--api openai-responses|openai-chat|anthropic|gemini` with `--url` for an endpoint,
with `--key` to read its key from stdin. `provider remove NAME` removes it. `provider list`
prints each built-in key's `source` (`environment`, `saved` or `absent`) and every custom
provider, never a key:

```json
{"keys":[{"provider":"openai","variable":"OPENAI_API_KEY","source":"saved"}],
 "custom":{"local":{"kind":"endpoint","api":"openai-chat","url":"http://localhost:11434/v1","key":false},
           "work":{"kind":"alias","provider":"codex"}}}
```

`prune` removes only the output folders inside `state/runs/<run-id>`: runtime `output`
(and `tmp`, `home` and `cache` left by older versions) and leftover tool output. Run
records, results, reuse records and unknown files stay, and `run show` and `request show`
still work. It refuses symbolic links and anything that looks like a project (a `.git` or
an Artifact declaration), also on `--dry-run`. Do not run it while another program is
changing those folders.
