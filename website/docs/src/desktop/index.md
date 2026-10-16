# Desktop

The desktop app is a window onto the same machine-wide state as the CLI and TUI.
It is a source-built preview, not a bundled CLI release component. Browse Runs,
inspect evidence and handle Human sign-offs without a second review engine or a
separate evidence store. See [Installation](../getting-started/install.md) for setup.

## Open the app

```sh
artifactize-desktop
artifactize-desktop monitor --repo PATH
artifactize-desktop review [REQUEST_ID] [--repo PATH | --all] [--reviewer NAME]
artifactize-desktop graph --repo PATH
```

The default is Monitor on **ALL** repositories in this state, regardless of the
current directory. `--repo` chooses an initial repository; it conflicts with
`--all`, the default. `--state-dir PATH` chooses a state directory; otherwise the
app uses the shared state home. `--reviewer NAME` sets the cooperative claim name
(normally the user name). `review REQUEST_ID` opens a saved request; `graph`
requires `--repo` and reads current declarations. Invalid arguments exit 2.

Shortcut labels below use Ctrl; macOS uses its primary modifier, Command, instead.
Ctrl+1 opens Monitor, Ctrl+2 Review and Ctrl+, Settings. Ctrl+K or the palette
button opens a searchable command palette; Enter runs a command and Esc closes it.
`?` shows this screen's keys; Ctrl+R or F5 refreshes from disk. Drag the dividers to
resize panes.

## Monitor

Navigate **Scope → Runs → Run tree → Detail** with Enter or → to move deeper and
← or Esc to return. Arrow keys or `j`/`k` select rows; Space or `h`/`l` folds tree
rows. Scope remains navigable back to ALL even after an initial `--repo` selection.
Worktrees without Runs can be folded into a group: Space or a click reveals or
hides them. Runs appear newest first. Move past the list's first or last row to
load newer or older pages, or use its newer/older arrow buttons.

Enter or `o` opens Detail for a **Run, Artifact or eval**. The Run row shows saved
validation and unmet obligations, request counts, waiting work, timing, budgets,
executed/reused/derived work and usage. Artifact Detail summarizes that Artifact's
review state. New Runs, claims and submissions made elsewhere appear without a
manual reload. Run-end validation stays historical: a later Human submission can
change the current request display without rewriting the old verdict. Damaged
records are [marked unreadable](../features/others/state.md#unreadable-records),
not treated as successful evidence.

`!` jumps to the next ERROR, RED or waiting Human eval. Upstream markers identify
the selected row's dependencies; `b` cycles through them, unmet first, then
completed inputs. Backspace or the context menu's return action goes back to the
original row.

### Detail and conversations

Record sections show Outcome, Waits for, What, Provenance and Technical when
applicable. `t` toggles Technical. In Waits for, `w` or the pending-evals control
reveals the unfinished checks behind an upstream wait. Runtime evidence is saved
stdout/stderr, not a live log stream. Human Detail provides the explicit review
controls described below.

An **Agent** request opens its locally available conversation first. Press `d`
or click **Details** to inspect its instruction, result, provenance and Technical
sections; `d` switches back. `t` and `w` act on the sections, not the default
transcript view. Missing or remote conversations explain why they are unavailable
on this machine.

Transcripts update live. In a focused transcript, ↑/↓ or `j`/`k` moves by row,
PgUp/PgDn by page, Home/End or **Top/Bottom** goes to either end, and the mouse wheel
scrolls. Moving up pauses following; return to Bottom to follow new messages.
Space or Enter toggles the visible grouped content; clicking a group header also
expands or folds it. Only public reasoning summaries are shown, not hidden reasoning.

### Run graph

`v` switches the selected Run between tree and Artifact graph. Node and relation
colors show review states and update as results arrive. ←/→ moves upstream or
downstream; ↑/↓ selects a neighbor in the same column. Enter or a click opens an
Artifact's evals; ↑/↓ selects an eval and Detail follows; Esc zooms out of that
Artifact.

This opening of an Artifact is separate from camera zoom: **Zoom In**, **Zoom Out**
and **Fit View** change the viewport in both graph modes. Above 40 nodes, a pannable,
zoomable minimap helps navigate the whole graph. Runs with **more than 300
Artifacts** ask before layout: choose **Lay out all** to lay out the whole graph,
or keep using the tree. Only visible elements are rendered, but the layout is not
an incrementally loaded neighborhood.

`artifactize-desktop graph --repo PATH` opens the **current** Artifact Graph
without needing a Run or showing saved verdicts. Expand **Structure and evaluation
criteria** for the full tree, criteria and labeled relations. Refresh or Ctrl+R/F5
reloads declarations. A Human `graph` tool opens this viewer; opening it approves
nothing.

## Review

Review starts with waiting Human requests across all repositories in this state.
A selected request previews read-only; Enter opens it. Other eval kinds and
settled requests remain read-only. The context menu or palette can open a Human
request with Tools focused; Ctrl+O opens a request by ID. PgDn/PgUp pages older
and newer requests. Waiting followers open the original review, not another sign-off.

`/` or Ctrl+F edits the search. Free text and `key:value` terms must all match:
`repo`, `eval`, `request`, `reviewer`, `tag`, `kind`, `status` and `is`.
`is:mine`, `is:claimed` and `is:unclaimed` filter Human claims. Suggestions complete
keys and values. Esc closes suggestions, then clears the search, then returns to
the list. An explicit `--repo` supplies the initial repository search term.

### Human sign-offs

Opening or previewing a review claims nothing. Press `c` to claim and `u` to
release. Another person's claim prevents your actions. Claims are cooperative
reviewer locks, not authenticated accounts; only the claimant can run tools or
submit. See [Human](../features/human.md) for shared lifecycle and deferred sign-offs.

In Tools, ↑/↓ or `j`/`k` selects an action and previews its resolved command,
description or reason it cannot run. A click focuses and selects a tool. **Enter,
or a second click on the selected tool while Tools is focused, runs it immediately**.
Output tools show captured text, launch tools open an application, and failures
show an error without a verdict. The saved definition must still match before an
action runs.

After claiming, `g` opens GREEN and `r` RED. Flat owner schemas get typed fields;
complex schemas open an owner-fields JSON editor. **Edit as JSON** or Ctrl+E turns
a typed form into JSON. Include only owner fields, not a `verdict` wrapper. Tab
indents JSON; Shift+Tab leaves that editor. In typed forms, Tab moves between
controls. Ctrl+G/Ctrl+R switches verdicts; Ctrl+S submits, never plain Enter.
Schema errors identify the failing path and leave the form correctable. Input-method
composition, including Korean, belongs to the text field and never fires shortcuts.
Esc cancels active work, then leaves the form, then leaves Detail.

A completed result stays open. Remote publication can fail **after local
settlement**: the form closes and a saved-locally notice explains the failure.
The verdict is already recorded; retry publication with Settings' **Push results**
(or `artifactize remote push`), not by submitting again.

### Leave and quit

`q` quits outside a text field, except while Human Detail is editing or busy:
leave or cancel that work first. Closing the window also starts quitting. If
this window still holds unsubmitted Human claims, choose **Stay** (Esc),
**Release and quit** (`u`) or **Keep and quit** (`k`). Keeping a claim leaves it
held for this reviewer; it is not a verdict.

## Settings

- **API keys:** reports whether OpenAI and Anthropic key environment variables
  are set, never their values. Set them in the app's launch environment.
- **Codex:** shows not signed in, artifactize's stored token and expiry, a read-only
  `ARTIFACTIZE_CODEX_AUTH_FILE`, or why storage is refused. An expired stored access
  token refreshes on next use; an expired external file needs a new Codex sign-in.
  External-file and refused-storage modes disable sign-in here; sign-out is
  available only for artifactize's own stored account. Browser sign-in can be
  cancelled, and a redirect URL can be pasted when the browser cannot return.
- **Models:** lists model IDs offered by the chosen backend.
- **Review store:** signs in with URL, token and sharing level, displays status,
  pushes results (or a dry run) and signs out. **Summary** excludes argv, captured
  output, tool calls and paths; **Full** also sends the saved execution. Summary
  still shares review metadata and owner results, so choose according to the
  team's privacy needs. Signing out removes local configuration, **not the server
  token**; a store administrator must [revoke it](../features/team/reference.md#review-store-server).
  See [sharing and push behavior](../features/team/reference.md#remote-review-store-client).

Settings uses the same accounts and store operations as the CLI; credentials do
not belong in `.artf`.
