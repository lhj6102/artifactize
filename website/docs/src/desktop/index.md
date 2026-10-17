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
`?` shows this screen's keys. Ctrl+R or F5 refreshes Monitor, or reloads current
Graph declarations when Graph is open; it does not reload Review or Settings.
Drag the dividers to resize panes.

## Monitor

Navigate **Scope → Runs → Run graph** with Enter to move deeper and Esc to
return. Arrow keys or `j`/`k` select Scope and Runs rows. Scope remains navigable
back to ALL even after an initial `--repo` selection. Worktrees without Runs can
be folded into a group: Space or a click reveals or hides them. Runs appear
newest first. Move past the list's first or last row to load newer or older
pages, or use its newer/older arrow buttons.

Selecting a Run updates its graph while Runs keeps the keyboard; Enter moves the
keyboard into the graph. The divider between the left side and the graph area is
always there: with no Run selected, while loading, on an error or with an empty
graph, the area stays and says so. What went wrong, such as Runs on the page that
cannot be read, shows as one line in the Run pane's header; click it to read it
in full and again to fold it. It never moves or hides the graph. New Runs, claims
and submissions made elsewhere appear without a manual reload. Run-end validation
stays historical: a later Human submission can change the current request display
without rewriting the old verdict. Damaged records are
[marked unreadable](../features/others/state.md#unreadable-records), not treated
as successful evidence.

Hovering only highlights what is under the pointer; nothing opens on hover.
Information opens on a click.

### Detail

A click on an Artifact opens its **Detail**, floating over the graph's right side
with the graph still visible behind it. Clicking another Artifact switches the
Detail to it; clicking the graph's background, **Close** or Esc closes it. Click
the Run's headline, or use the palette command **Open the Run's Detail**, for the
Run's own Detail: saved validation and unmet obligations, request counts, waiting
work, timing, budgets, executed/reused work and usage. Drag the Detail's left edge
to resize it; it keeps that width, also in the next launch.

An Artifact's Detail has chips: **Artifact** first, then one per eval with its
verdict's glyph and colour. Chips that do not fit wrap onto the next line. The
Artifact chip lists the Artifact's evals with their verdicts; click one to show
its chip.

An eval's chip shows its **Summary**: Outcome, Waits for, What, Provenance and
Technical when applicable. `t` toggles Technical. In Waits for, `w` or the
pending-evals control reveals the unfinished checks behind an upstream wait. An
eval with a log switches below the chips between its Summary, shown first, and
its log: an Agent eval's **Agent log** (its conversation) or a runtime eval's
**Runtime log** (saved stdout/stderr, not a live log stream). Missing or remote
conversations explain why they are unavailable on this machine. A Human eval's
chip holds its review and sign-off, with the controls described below.

Agent logs update live. In a focused Agent log, ↑/↓ or `j`/`k` moves by row,
PgUp/PgDn by page, Home/End or **Top/Bottom** goes to either end, and the mouse wheel
scrolls. Moving up pauses following; return to Bottom to follow new messages.
Space or Enter toggles the visible grouped content; clicking a group header also
expands or folds it. Only public reasoning summaries are shown, not hidden reasoning.

### Run graph

Monitor always displays the selected Run as an Artifact graph; there is no
Run-tree mode. Node and relation colors show review states and update as results
arrive. Inspect dependencies through the graph's input-to-consumer relations and
an eval's **Waits for** section. The graph takes the pointer: Tab reaches its
Artifacts and Enter on one opens its Detail, as a click does; it has no other
keys of its own.

Drag the graph background to pan; wheel, pinch or a background double-click
zooms the camera. **Zoom In**, **Zoom Out** and **Fit View** also change its
viewport. These gestures move the camera, not the pane dividers or the Detail.
Above 40
nodes, a pannable, zoomable minimap helps navigate the graph. Runs with **more
than 300 Artifacts** ask before layout: choose **Lay out all** to display the
whole graph. Only visible elements are rendered: panning or zooming draws the
Artifacts it brings into view. Layout is not an incrementally loaded
neighborhood, and there is no tree fallback.

`artifactize-desktop graph --repo PATH` opens the **current** Artifact Graph
without needing a Run or showing saved verdicts. A click on an Artifact shows its
evals over the graph's right side; Esc, **Close** or a click on the background
closes them. Expand **Structure and evaluation
criteria** for the full tree, criteria and labeled relations. Refresh or Ctrl+R/F5
reloads declarations. A Human `graph` tool opens this viewer; opening it approves
nothing.

## Review

Review starts with waiting Human requests across all repositories in this state.
A selected request previews read-only; Enter opens it. Other eval kinds and
settled requests remain read-only. The context menu or palette can open a Human
request with Tools focused; Ctrl+O opens a request by ID. PgDn/PgUp pages older
and newer requests. Waiting followers open the original review, not another sign-off.
One click selects a request; clicking an already-selected request opens it, as
does a double-click. Clicking its preview also opens Detail. A read-only request
with a log switches between its Summary and its Agent or Runtime log, as an
eval's chip does in Monitor.

`/` edits search from the Review request list. Ctrl+F returns to search from an
open request Detail too, including while a Human form is open or review work is
running; it is not blocked by that Detail's normal focus lock.
Free text and `key:value` terms must all match:
`repo`, `eval`, `request`, `reviewer`, `tag`, `kind`, `status` and `is`.
`is:mine`, `is:claimed` and `is:unclaimed` filter Human claims. Suggestions complete
keys and values. ↑/↓ selects a suggestion (↓ can also open suggestions); Tab or
Enter accepts it, and a click accepts it with the mouse. Enter without an open
suggestion returns focus to the list. Esc closes suggestions, then clears the
search, then returns to the list. An empty search includes all request kinds,
not just waiting Human reviews. An explicit `--repo` supplies the initial repository search term. Use
`r` in the Review list, or **Refresh the list** in the palette, to reload that list.
The palette's **Show waiting Human reviews** restores the launch search,
including the initial repository restriction when the app was started with `--repo`.

### Human sign-offs

Opening or previewing a review claims nothing. Press `c` to claim and `u` to
release. Another person's claim prevents your actions. Claims are cooperative
reviewer locks, not authenticated accounts; only the claimant can run tools or
submit. See [Human](../features/human.md) for shared lifecycle and deferred sign-offs.

The instruction comes first. `i` or its expand/fold button changes how much is
shown; Ctrl+PgUp/PgDn scrolls long criteria from any sub-area. `t` toggles the
Human request's Technical record. Tab or a click moves to Tools; Ctrl+U releases
your claim even while editing.

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
controls. Ctrl+G/Ctrl+R switches verdicts. Enter inside owner-field editors does
not implicitly submit; Ctrl+S or activating the **Submit** button submits,
including Enter while that button has keyboard focus.
Schema errors identify the failing path and leave the form correctable. Input-method
composition, including Korean, belongs to the text field and never fires shortcuts.
Esc cancels active work, then leaves the form, then leaves Detail.
Leaving a form keeps its verdict's draft; reopening it restores that draft, and
GREEN and RED keep separate drafts. Monitor and Review share the same request's
review state within this app session, rather than creating independent forms.

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

Settings reloads account/store information when shown. Reopen it or use the
palette's **Refresh settings** command to reload it; Ctrl+R/F5 is Monitor/Graph
refresh, not a settings reload.

- **API keys:** reports whether OpenAI and Anthropic key environment variables
  are set, never their values. Set them in the app's launch environment.
- **Codex:** shows not signed in, artifactize's stored token and expiry, a read-only
  `ARTIFACTIZE_CODEX_AUTH_FILE`, or why storage is refused. An expired stored access
  token refreshes on next use; an expired external file needs a new Codex sign-in.
  External-file and refused-storage modes disable sign-in here; sign-out is
  available only for artifactize's own stored account. Browser sign-in can be
  cancelled, and a redirect URL can be pasted when the browser cannot return.
  Sign-out removes local tokens and warns when server revocation cannot be confirmed.
- **Models:** lists model IDs offered by the chosen backend.
- **Review store:** signs in with URL, token and sharing level, displays status,
  pushes results (or a dry run) and signs out. **Summary** omits invocation details,
  captured output and tool-call history; **Full** also sends the saved execution.
  Summary still shares review metadata, owner results, declared Runtime commands
  and arguments, and Agent execution-path metadata. Those declarations can contain
  paths or sensitive literals: Summary is not secret redaction. Choose according
  to the team's privacy needs. Signing out removes local configuration, **not the server
  token**; a store administrator must [revoke it](../features/team/reference.md#review-store-server).
  See [sharing and push behavior](../features/team/reference.md#remote-review-store-client).

Settings uses the same accounts and store operations as the CLI; credentials do
not belong in `.artf`.
