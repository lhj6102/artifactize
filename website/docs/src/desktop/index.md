# Desktop

The desktop app is a window onto the same machine-wide state as the CLI and TUI.
Use it to browse Runs, inspect reviews and handle Human sign-offs. It does not run
a second review engine or store a separate copy of the evidence. See
[Installation](../getting-started/install.md) for availability and setup.

## Open the app

```sh
artifactize-desktop
artifactize-desktop monitor --repo PATH
artifactize-desktop review [REQUEST_ID] [--repo PATH | --all] [--reviewer NAME]
artifactize-desktop graph --repo PATH
```

The default screen is Monitor with **ALL** repositories on this machine selected,
regardless of the current directory. `--repo` chooses an initial repository;
`--all` is the default and conflicts with it. `--state-dir PATH` uses a particular
state directory; otherwise the app uses the shared state home. `--reviewer NAME`
sets the cooperative claim name (normally the user name). `review REQUEST_ID`
opens that saved request; `graph` requires `--repo` and reads current declarations.
Invalid arguments exit 2.

Ctrl+1 opens Monitor, Ctrl+2 Review and Ctrl+, Settings. Ctrl+K or the palette
button opens a searchable command palette; Enter runs a command and Esc closes it.
`?` shows the current screen's keys. Ctrl+R or F5 refreshes from disk. Dividers
between panes can be dragged to resize them.

## Monitor

Start in **Scope**, choose a repository or worktree, then a Run and an Artifact or
eval in its saved tree. Arrow keys or `j`/`k` select a row; Enter or → moves deeper;
← or Esc moves back. Space or `h`/`l` folds tree rows. The scope stays navigable
back to ALL: choosing an initial repository does not hide the others.

Enter or `o` on an eval opens Detail with the instruction, result, request and
provenance. `t` toggles the technical record. Runtime evidence is saved output;
Agent evidence includes the locally available conversation. `!` goes to the next
ERROR, RED or waiting Human eval, and `b` follows an upstream blocker. New Runs,
claims and submissions made by other processes appear without a manual reload.
A Run's end verdict is historical; a later Human submission can change the live
request display without rewriting that verdict.

### Run graph

`v` switches the selected Run between its tree and Artifact graph. Nodes and
relations show review states and recolor as results arrive. ←/→ moves upstream or
downstream; ↑/↓ selects a neighbor in the same column. Enter or a click zooms into
an Artifact's evals; ↑/↓ selects an eval and Detail follows; Esc zooms out. Large
graphs load the visible neighborhood as navigation moves beyond it.

`artifactize-desktop graph --repo PATH` opens the **current** Artifact Graph without
requiring a Run. It uses the same navigation, but has no saved verdicts. Expand
**Structure and evaluation criteria** to read the full tree, criteria and labeled
relations. Refresh or Ctrl+R/F5 reloads declarations. A declared Human `graph`
tool opens this viewer; opening it is not an approval.

## Review

Review starts with the waiting Human requests across every repository in the
state. A selected request previews read-only; Enter opens it. Other eval kinds
and settled requests remain read-only. The context menu or palette can open a
Human request with Tools focused; Ctrl+O opens a request by ID. PgDn/PgUp pages
older/newer requests. A waiting follower opens its original review rather than
creating another sign-off.

`/` or Ctrl+F edits the search. Free text and `key:value` terms must all match:
`repo`, `eval`, `request`, `reviewer`, `tag`, `kind`, `status` and `is`.
`is:mine`, `is:claimed` and `is:unclaimed` filter Human claims. Suggestions complete
keys and values. Esc closes suggestions, then clears the search, then returns to
the list. An explicit `--repo` supplies the initial repository search term.

### Human sign-offs

Opening or previewing a review claims nothing. Press `c` to claim it and `u` to
release it. A claim held elsewhere is visible and prevents your actions. In Tools,
Enter runs the selected predefined tool: an output tool shows captured text; a
launch tool opens its application; a failure displays an error without a verdict.
The current saved definition must still match before an action can run.

After claiming, `g` opens the GREEN form and `r` the RED form, built from the eval's
owner schema. Ctrl+G/Ctrl+R switches verdicts while editing; Ctrl+S submits.
Schema errors show the failing path and leave the form correctable. A completed
result stays open. Text fields accept input-method composition, including Korean;
keys typed during composition never trigger shortcuts. Esc cancels active work,
then leaves the form, then leaves Detail. Editing keys belong to the focused field.

`q` outside a field, or closing the window, quits. If this window still holds
Human claims, it asks first: Esc stays and `u` releases those claims and quits.
Claims are cooperative reviewer locks, not authenticated accounts; only the
claimant can run tools or submit. See [Human](../features/human.md) for the shared
lifecycle and deferred sign-offs.

## Settings

- **API keys:** shows whether the OpenAI and Anthropic key environment variables
  are set, never their values. Set those variables in the app's launch environment.
- **Codex:** signs in through the browser or signs out. If the browser cannot
  return, paste its redirect URL; a sign-in in progress can be cancelled.
- **Models:** lists the model IDs offered by the chosen backend.
- **Review store:** signs in with the store URL, token and share level, displays
  status, pushes results (including a dry run), and signs out. These are the same
  accounts and store operations the CLI uses; credentials do not belong in `.artf`.
