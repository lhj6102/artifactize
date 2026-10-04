# Install artifactize

artifactize is one binary, built from source with Cargo.

> [!NOTE]
> The repository is private for now, so cloning it needs access to
> [lhj6102/artifactize](https://github.com/lhj6102/artifactize).

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
artifactize --version          # artifactize 0.4.0
```

The binary goes to `~/.cargo/bin`, which must be on `PATH`; `--root DIR` installs
it under `DIR/bin` instead. Run the same command again to upgrade. The next
command that opens an older state database upgrades it in place; `artifactize
doctor` reports its schema without changing it. 0.4 calls the reuse declaration
`fingerprint`; `config check` shows the new shape for each `artifactize.json` that
still uses the old field.

## State

All state lives in one directory: `state.sqlite` (Runs, requests, executions, the
fingerprint cache and Human claims), Run output under `runs/`, and ChatGPT
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
artifactize doctor                 # state and its schema, API-key presence, ChatGPT login, claude --version
artifactize doctor --repo PROJECT  # also validates PROJECT's declarations
```

`doctor` calls no provider and creates no Run. Missing keys, logins or a missing
`claude` binary are warnings, because every backend is optional. It exits 1 only
for hard errors such as unusable state or invalid configuration.

## Cleanup and uninstall

```sh
artifactize prune --dry-run           # list removable scratch output of finished Runs
artifactize prune --older-than 7d     # remove it; saved results stay readable
artifactize logout chatgpt            # if you signed in
cargo uninstall artifactize
```

`prune` never touches active Runs, database rows or the repository. To remove
everything, delete the state directory that `artifactize doctor` prints on its
first line.

Next, take the [Quick start](quick-start.md): a five-minute tour that needs no model.
