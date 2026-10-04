# Install artifactize

## Prerequisites

- Linux or WSL 2. Other platforms are not supported.
- A Rust toolchain from [rustup](https://rustup.rs) and a C compiler (SQLite is
  built from source). The repository's `rust-toolchain.toml` selects stable Rust.
- `python3` and `grep` for the example projects.
- Optional, one per Agent backend you plan to use: `OPENAI_API_KEY`,
  `ANTHROPIC_API_KEY`, a ChatGPT plan that supports Sign in with ChatGPT, or the
  official `claude` CLI, installed and signed in. Runtime and Human evals need none.

## Install

```sh
git clone https://github.com/lhj6102/artifactize
cd artifactize
cargo install --path crates/artifactize --locked
artifactize --version          # artifactize 0.2.0
```

The binary goes to `~/.cargo/bin`, which must be on `PATH`; `--root DIR` installs
it under `DIR/bin` instead. Run the same command again to upgrade.

## State

All state lives in one directory: `state.sqlite` (Runs, requests, executions, the
staleKey cache and Human claims), Run output under `runs/`, and ChatGPT
credentials under `auth/`. The directory is `$ARTIFACTIZE_STATE_HOME`, else
`$XDG_STATE_HOME/artifactize`, else `~/.local/state/artifactize`.

`--state-dir PATH` moves the whole state for one command. Use the same value for
`login`, `verify`, `request`, `run` and `monitor`. State must be outside the
repository under review. ChatGPT credentials are also refused inside any Git
checkout or artifactize project, such as a dotfiles repository at `$HOME`;
`doctor` reports this as a failed `chatgpt` check.

## Backends

An Agent eval names exactly one backend and one exact model ID. There is no model
catalog and no fallback to another backend or model.

| Backend | Setup | List models |
|---|---|---|
| `openai` | `export OPENAI_API_KEY=...` | `artifactize models openai` |
| `anthropic` | `export ANTHROPIC_API_KEY=...` | `artifactize models anthropic` |
| `chatgpt` | `artifactize login chatgpt` | `artifactize models chatgpt` |
| `claude` | install the official `claude` CLI and sign in once with `claude` | `artifactize models claude` explains model names |

`login chatgpt` prints a sign-in URL and tries to open it with `xdg-open` or
`wslview`. Finish in a browser on the same machine (on WSL 2, a Windows browser
works); the callback listens on `127.0.0.1`. `logout chatgpt` asks ChatGPT to
revoke the refresh token and deletes the stored tokens. The `claude` backend runs
the CLI found on `PATH`; artifactize never reads Claude credentials.

```sh
artifactize doctor                 # state, API-key presence, ChatGPT login, claude --version
artifactize doctor --repo PROJECT  # also validates PROJECT's declarations
```

`doctor` calls no provider and creates no Run. Missing keys, logins or a missing
`claude` binary are warnings, because every backend is optional. It exits 1 only
for hard errors such as unusable state or invalid configuration.

## Quick start (5 minutes, no model needed)

From the repository root, with state kept apart from your real state:

```sh
export ARTIFACTIZE_STATE_HOME=$(mktemp -d)
cd examples/runtime-relations
artifactize config check    # static validation; runs no owner code
artifactize status          # exit 1: two evals would execute, guide/terms waits for them
artifactize graph           # Artifacts, evals and child/mount/reference relations
artifactize verify --all    # three GREEN results, exit 0; prints "Run: RUN_ID"
artifactize run show RUN_ID # the saved Run as JSON: argv, stdout, staleKey
artifactize verify --all    # exit 0 and nothing executes: all three results are reused
artifactize status          # exit 0: every eval shows "PASS — reuse"
```

Then try a family: three Artifacts from one declaration.

```sh
cd ../family
artifactize verify --all    # release-notes/style, tip/style, welcome/style: GREEN
artifactize verify posts    # the family name selects every instance; all reused
```

[runtime-relations](../examples/runtime-relations/README.md) and
[family](../examples/family/README.md) also show how to make a check RED.

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
`--reviewer NAME`. `verify --all --wait` keeps one Run alive until Human results
arrive, so dependents continue in the same Run.

For a real Agent review, put an exact model ID in the profile you use in
`examples/agent-tools/spec/artifactize.json`, then run
`artifactize verify --eval spec/review`, adding `--profile anthropic`,
`--profile chatgpt` or `--profile claude` for the other backends. Without
credentials the review is recorded as ERROR and exits 2. See the
[agent-tools README](../examples/agent-tools/README.md).

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
[Team review store](../README.md#team-review-store) and the
[team walkthrough](team-walkthrough.md).

## Monitor

```sh
artifactize monitor         # Runs of the current directory's repository
artifactize monitor --all   # Runs of every repository in this state
```

The monitor reads the state database only. Start it in a second terminal while
`verify` runs to watch progress. Keys: `j`/`k` move, Enter opens a Run, `h`/`l`
collapse and expand families, PgUp/PgDn scroll details, Esc goes back, `r`
refreshes, `q` quits.

## Cleanup and uninstall

```sh
artifactize prune --dry-run           # list removable scratch output of finished Runs
artifactize prune --older-than 7d     # remove it; saved results stay readable
artifactize cache gc                  # enforce the cache entry/byte limits now
artifactize logout chatgpt            # if you signed in
cargo uninstall artifactize
```

`prune` never touches active Runs, database rows or the repository. To remove
everything, delete the state directory that `artifactize doctor` prints on its
first line.
