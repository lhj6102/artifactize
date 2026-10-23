# Update and uninstall

## Update

| Installed with | Update |
|---|---|
| `install.sh` or `install.ps1` | Run the same command again; it updates the desktop app too |
| Homebrew | `brew upgrade artifactize`; the desktop app with `brew upgrade --cask artifactize` |
| Microsoft Store | Automatic; or **Library → Get updates** in the Store app |
| Disk image, `.deb` or AppImage | Install the newer file from the release the same way |
| `cargo binstall` | `cargo binstall artifactize artifactize-tools` |
| `cargo install` | `cargo install artifactize artifactize-tools --locked`; from a checkout, `git pull` and repeat `cargo install --path` |

The install scripts and Cargo get a release as soon as it is published. Homebrew
and the Microsoft Store follow stable releases only; a Store update can lag while
Microsoft reviews it. Updating keeps your state.

The [plugin page](../agent-plugin/plugin.md#updates) shows how to update the
plugin.

### Upgrading from 0.11

The state's format of saved review data (its schema) is upgraded automatically on first use,
from schemas 6 to 8 to the current schema 9; existing Runs, cache records and Human approvals
are kept. An older artifactize cannot open the upgraded state, so keep a backup before
switching versions.

`artifactize verify` now submits a durable Run, prints its ID and exits on acceptance.
Add `--wait` to scripts and CI that need the validation exit code. Closing a terminal
or interrupting a watcher leaves the Run running; `artifactize run cancel RUN_ID`
is the explicit cancellation action. The foreground Human deadline and
`verify --timeout-ms` are removed. `--request-key` makes retries return the same Run.

Gate bypass (`--ignore-gates` and `review_policy`, including
`review_policy.dependency_gates`) is removed; even an empty `review_policy` table fails
configuration validation, so delete it. Per-request `--speculate` and `--defer-human`
replace it, both off by default and neither waiving final approval; deferred sign-offs
end with `SIGNOFFS_PENDING` and `--wait` exit 5. See [Runs and status](../features/run.md).
Plain `artifactize monitor` starts on ALL; `monitor .`, a positional path or `--repo`
selects that repository initially.

### Upgrading to 0.9

Rewrite declarations as TOML `index.artf` for folders and `<file>.artf` for files. Keys
are snake_case and evals are `[evals.<id>]` tables; `.artfignore` replaces the old ignore
file. There is no converter. Have an agent rewrite and commit the files, remove the
legacy markers and run `artifactize config check`. Families are removed; use your own generator if
you want templates and commit its `.artf` output. See [Declarations (.artf)](../features/declarations.md).

A state from before 0.9 (schemas 1–5) is refused: set `ARTIFACTIZE_STATE_HOME` or
`--state-dir` to a new directory, or move the old one away. Later states upgrade by
themselves ([State](../features/others/state.md#state)).
Reuse-key v2 matches no pre-0.9 result, even in
a team review store or with a script fingerprint. The first full `verify` reviews
every eval once, Human sign-offs included. See [Upgrading to 0.9](../features/fingerprint.md#upgrading-to-09).

Monitor now opens an eval's Detail in its pane rather than a modal. Standalone
Review shares that Human Detail: claims are explicit, and tools and submission
never claim for you. The verdict forms, submit keys and editor shortcut changed;
see [Human reviews and Monitor](../features/human.md) for the current controls.

0.5.0 removes the `chatgpt` and `claude` Agent backends, with `login chatgpt`,
`logout chatgpt` and the internal `mcp` command, and adds `codex` for ChatGPT plan
users. `config check` names the replacements for each eval or profile variant that
still declares a removed backend. artifactize no longer uses ChatGPT
tokens saved by an earlier release: disconnect artifactize in ChatGPT Settings, then
delete `auth/chatgpt.json`, `auth/chatgpt-registration.json` and `auth/chatgpt.lock`
from the state directory.

## State

All state, including Runs, saved Agent conversations and sign-ins, lives in one
directory, by default `~/.local/state/artifactize` (`%LOCALAPPDATA%\artifactize` on
Windows). [State](../features/others/state.md#state) lists its contents, the
environment variables that move it and the Store version's private copy.

`--state-dir PATH` moves the whole state for one command. Use the same value for
`login`, `verify`, `request`, `run` and `monitor`. State must be outside the repository
under review. Tokens under `auth/` are also refused inside any git work tree or
artifactize project, such as a dotfiles repository at `$HOME` or a gitignored folder
in a checkout; `ARTIFACTIZE_CODEX_AUTH_FILE` needs no storage
([State](../features/others/state.md#state)).

## Cleanup and uninstall

```sh
artifactize prune --dry-run           # list removable scratch output of finished Runs
artifactize prune --older-than 7d     # remove it; saved results stay readable
artifactize logout codex              # if you signed in with Codex
artifactize remote logout             # if you signed in to a review store
```

Then remove the commands the way you installed them:

```sh
rm ~/.local/bin/artifactize ~/.local/bin/artifactize-tools   # install.sh (or your ARTIFACTIZE_INSTALL_DIR)
brew uninstall artifactize                                   # Homebrew
cargo uninstall artifactize artifactize-tools                # cargo install or cargo binstall
```

And the desktop app:

```sh
rm -rf ~/Applications/Artifactize.app                        # install.sh on macOS
rm -rf ~/.local/share/artifactize/desktop ~/.local/bin/artifactize-desktop \
  ~/.local/share/applications/artifactize.desktop             # install.sh on Linux
brew uninstall --cask artifactize                            # Homebrew cask, with its commands
sudo apt remove artifactize                                  # .deb
```

From a disk image, drag **Artifactize** from Applications to the Trash. If you
approved first-launch command setup, also remove its terminal path entry:

```sh
sudo rm -f /etc/paths.d/artifactize
```

For an AppImage run on its own, delete the AppImage. Remove only the command links
that point to the app's stable copies, then remove those copies:

```sh
bin="${XDG_DATA_HOME:-$HOME/.local/share}/artifactize/bin"
for command in artifactize artifactize-tools; do
  link="$HOME/.local/bin/$command"
  [ "$(readlink "$link")" != "$bin/$command" ] || rm "$link"
done
rm -rf "$bin"
```

Open a new terminal after removing a command's path entry.

On Windows:

```powershell
Remove-Item -Recurse "$env:LOCALAPPDATA\Programs\artifactize"   # install.ps1
Remove-Item "$([Environment]::GetFolderPath('Programs'))\Artifactize.lnk"   # its Start menu shortcut
```

Uninstall the Store version from **Settings → Apps → Installed apps**, or right-click
Artifactize in the Start menu. After removing an `install.ps1` install, also remove
its directory from your user `Path` (search the Start menu for "Edit environment
variables for your account").

Uninstalling the **Store app** removes its package-private state, including Runs,
saved results, sign-in and cache. Other install methods keep their state directory,
so a later unpackaged install finds those Runs and results again. `prune` never
touches active Runs, database rows or the repository. To remove retained unpackaged
state, delete the state directory that `artifactize doctor` prints on its first line.

Uninstall does not remove your project files, exported files, or state explicitly
placed outside AppData with `--state-dir` or an environment override. Files your evals' commands wrote, including in AppData, stay too; remove them
separately if you no longer need them. Existing unvirtualized
AppData files are also outside the Store package's cleanup; see the
[Windows behavior and overlay caveat](../features/others/state.md#state).
