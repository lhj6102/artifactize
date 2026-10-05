# Quick start (5 minutes, no model needed)

The tour uses the example projects in the repository. If you installed with
`cargo install --git`, clone it first:

```sh
git clone https://github.com/lhj6102/artifactize
cd artifactize
```

From the repository root, with state kept apart from your real state:

```sh
export ARTIFACTIZE_STATE_HOME=$(mktemp -d)
cd examples/runtime-relations
artifactize config check    # static validation; runs no owner code
artifactize status          # exit 1: two evals would execute, guide/terms waits for them
artifactize config graph    # Artifacts, evals and child/mount/reference relations
artifactize verify --all    # three GREEN results, exit 0; prints "Run: RUN_ID"
artifactize run show RUN_ID # the saved Run as JSON: argv, stdout, fingerprint
artifactize verify --all    # exit 0 and nothing executes: all three results are reused
artifactize status          # exit 0: every eval shows "PASS — reuse"
```

Then try a family: three Artifacts from one declaration.

```sh
cd ../family
artifactize verify --all    # release-notes/style, tip/style, welcome/style: GREEN
artifactize verify posts    # the family name selects every instance; all reused
```

[runtime-relations](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations) and
[family](https://github.com/lhj6102/artifactize/tree/main/examples/family) also show how to make a check RED.

## Agent tools and the Human flow

```sh
cd ../agent-tools
artifactize tools check     # static: schemas, executables and scoped paths
artifactize tools check --execute --artifact spec --audience agent --tool coverage --args '{}'
artifactize verify --eval spec/signoff   # records a WAITING_HUMAN request, exit 4
artifactize request list --run RUN_ID    # shows REQUEST_ID
artifactize request claim REQUEST_ID
artifactize request tool REQUEST_ID notes_spec
artifactize request submit REQUEST_ID --verdict GREEN --fields '{"approved":true,"comment":"Both questions are answered."}'
artifactize run show RUN_ID
```

`claim`, `tool` and `submit` use `$USER` as the reviewer unless you pass
`--reviewer NAME`; `request unclaim REQUEST_ID` gives a claim back. `artifactize
review` does the same in a terminal UI: it lists waiting requests, claims on the
first tool run or submission, asks before a tool's first run and fills the owner
fields in a form. `verify --all --wait` keeps one Run alive until Human results
arrive, so dependents continue in the same Run.

For a real Agent review, put an exact model ID in the profile you use in
`examples/agent-tools/spec/artifactize.json`, then run
`artifactize verify --eval spec/review`, adding `--profile anthropic` or
`--profile codex` for the other backends. Without credentials the review is recorded as ERROR and exits 2. See the
[agent-tools README](https://github.com/lhj6102/artifactize/tree/main/examples/agent-tools).

## Monitor

```sh
artifactize monitor         # Runs of the current directory's repository
artifactize monitor --all   # Runs of every repository in this state
```

The monitor reads the state database only. Start it in a second terminal while
`verify` runs to watch progress. Keys: `j`/`k` move, Enter opens a Run, `h`/`l`
collapse and expand families, PgUp/PgDn scroll details, Esc goes back, `r`
refreshes, `q` quits. On a WAITING_HUMAN eval, `o` opens `artifactize review` for
it in the same terminal and returns to the monitor when the review exits.

## Team review store (optional)

To share verdicts across machines and CI, run `artifactize server` on one host and
give each machine a token:

```sh
artifactize --state-dir /srv/artifactize server token add alice-laptop --scopes read,publish,human
artifactize --state-dir /srv/artifactize server token add ci --scopes read
artifactize --state-dir /srv/artifactize server run   # loopback; put a TLS proxy or tunnel in front
```

On each machine:

```sh
artifactize remote login https://reviews.example/     # paste the token; it is not echoed
artifactize remote status
```

`verify` then reuses verdicts from the store and publishes its own. CI needs
only `ARTIFACTIZE_REMOTE` and a read-only `ARTIFACTIZE_REMOTE_TOKEN`. If the
store is unreachable, `verify` warns once and reviews locally. `doctor` checks
the remote configuration offline. See
[Team review store](../guides/team-review-store.md) and the
[team walkthrough](../guides/team-walkthrough.md).

## Next steps

The [examples](examples.md) show each feature in a small project, and
[Artifacts and evals](../concepts/artifacts-and-evals.md) explains the declaration
you just ran.
