# Quick start (5 minutes, no model needed)

The tour uses the example projects in the repository. If you installed with the
install script, `cargo binstall` or `cargo install`, clone it first:

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

The [runtime-relations README](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations) also shows how to make a check RED. Each folder declares itself in
TOML `index.artf`; omission of `fingerprint` uses artifactsum. To disable reuse, set
`fingerprint = false`. See [Declarations (.artf)](../reference/declarations.md) for snake_case keys and the JSON/output naming
distinction.

## Files and dependency readiness

A sidecar such as `hero.png.artf` declares the neighboring file as an independent
Artifact. It requires a `name`, uses the same TOML schema as a folder and hashes
only that file by default. Built-in tools see the file and admitted mounts and
references, not siblings.

A dependency eval collects readiness from other Artifacts without a new review:

```toml
# player/index.artf
name = "player"

[evals.ready]
title = "Movement and art are ready"
profile = { kind = "dependency", depends_on = ["player-movement", "hero-art"] }
```

Once those Artifacts are declared, `artifactize verify player/ready` includes their required evals
automatically. The result is GREEN only when required evidence is GREEN; status and
monitor show unfulfilled requirements. See [Artifacts and evals](../concepts/artifacts-and-evals.md).

## Agent tools and the Human flow

```sh
cd ../agent-tools
artifactize tools check     # static: schemas, executables and scoped paths
artifactize tools check --execute --artifact spec --audience agent --tool coverage --args '{}'
artifactize verify --eval spec/signoff   # records a WAITING_HUMAN request and waits
# In a second terminal, in the same directory:
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
fields in a form. `verify` waits for Human results like any other, so dependents
continue in the same Run; once the sign-off arrives it exits 4 here, because
`spec/review` has no result yet.

For a real Agent review, put an exact model ID in the profile you use in `examples/agent-tools/spec/index.artf`,
then run `artifactize verify --eval spec/review`, adding `--profile anthropic` or `--profile codex` for the other backends. Without
credentials the review is recorded as ERROR and exits 2. See the [agent-tools README](https://github.com/lhj6102/artifactize/tree/main/examples/agent-tools).

## Monitor

```sh
artifactize monitor         # Initially select this repository and worktree
artifactize monitor --all   # Initially select every repository in this state
```

Start it in a second terminal while `verify` runs to watch progress. Three panes
show repository/worktree → newest Runs → artifacts/evals; ALL rows switch scope
without restarting. ←/→ moves between panes (Tab cycles), `j`/`k`
selects, Enter or a double-click opens detail and `q` quits. Artifact rows
show tags. Agent details show saved conversations; Runtime details show saved logs;
dependency details show derived requirements. A waiting Human detail has Claim,
tools, GREEN/RED fields, Submit and Release in the monitor itself. Ctrl-S submits;
nested JSON is edited in the TUI (Enter adds a newline). F2 toggles mouse capture
for terminal text selection; keyboard paste works either way. Browsing does not
execute owner code; explicit Human tools and submissions use the review APIs. See
[Human reviews and monitor](../guides/human-reviews.md#monitor).

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
artifactize remote login https://reviews.example/     # paste the token; input is hidden when the terminal supports it
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
