# Desktop

The desktop app is a window onto the same machine-wide state as the CLI and TUI.
Browse Runs, inspect evidence and handle Human sign-offs without a second review
engine or a separate evidence store. Every release ships it beside the commands; see
[Desktop app](../getting-started/install.md#desktop-app) to install it.

For first-launch command setup, PATH details, updates and cleanup, see the
[installation guide](../getting-started/install.md#desktop-app).

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
Ctrl+1 opens Monitor, Ctrl+2 Review and Ctrl+, Settings. Ctrl+K, F1 or the palette
button opens a searchable command palette; Enter runs a command and Esc closes it.
`?` shows this screen's keys. Ctrl+R or F5 refreshes Monitor, or reloads current
Graph declarations when Graph is open; it does not reload Review or Settings.
Drag the dividers to resize panes. The app follows the system's light or dark
appearance.

## Monitor

Navigate **Scope → Runs → Run graph** with Enter or → to move deeper and Esc to
return; ← also returns from Runs. Arrow keys or `j`/`k` select Scope and Runs rows,
and `r` refreshes Monitor from Scope, Runs or the Run graph (a Human review's `r`
opens its RED form instead). Scope remains navigable
back to ALL even after an initial `--repo` selection. Worktrees without Runs can
be folded into a group: Space or a click reveals or hides them. Runs appear
newest first. Move past the list's first or last row to load newer or older
pages, or use its newer/older arrow buttons.

Scope lists ALL, then each Git repository, whose row covers every worktree that
shares its Git directory, with its worktrees below it; a folder outside Git is a
workspace row of its own. Selecting a row shows the Runs of that scope.

**Attention counters** read `!N ✗N ◐N ?N`: requests in ERROR and RED, Runs still
running, and sign-offs waiting for a person, deferred ones included even after
their Run ended. The window header always shows the totals of the whole catalog,
every repository (the ALL row), whatever scope, Run or page is selected; each
Scope row shows its own totals, and each Run row its own request counts.
Followers of one Human execution count as one waiting sign-off.

Selecting a Run updates its graph while Runs keeps the keyboard; Enter moves the
keyboard into the graph. The divider between the left side and the graph area is
always there: with no Run selected, while loading, on an error or with an empty
graph, the area stays and says so. What went wrong, such as Runs on the page that
cannot be read, shows as one line in the Run pane's header; click it to read it
in full and again to fold it. It never moves or hides the graph. New Runs, claims
and submissions made elsewhere appear without a manual reload. A read that fails
leaves what was last read on screen: graphs, request states and claims can be
stale while ages keep counting, so refresh the Monitor (Ctrl+R) or the Review
list after fixing what the notice names. Run-end validation
stays historical: a later Human submission can change the current request display
without rewriting the old verdict. Damaged records are
[marked unreadable](../features/others/state.md#unreadable-records), not treated
as successful evidence.

Hovering only highlights what is under the pointer; nothing opens on hover.
Information opens on a click.

### Detail

A click on an Artifact opens its **Detail**, floating over the graph's right side
with the graph still visible behind it. Clicking another Artifact switches the
Detail to it; clicking the graph's background, **Close** or Esc closes it. On a
Human review's Detail all three step back the same way: they cancel active work
first, then leave the form, and only then close the Detail. Click
the Run's headline, or use the palette command **Open the Run's Detail**, for the
Run's own Detail: saved validation and unmet obligations, request counts, waiting
work, timing, budgets, executed/reused work and usage. Drag the Detail's left edge
to resize it; it keeps that width, also in the next launch.

An Artifact's Detail has chips that read as a path, `[icon] {name} / [eval] …`:
the Artifact's own chip, then one per eval with its verdict's glyph and colour.

The Artifact's chip shows it between its neighbours, drawn with the Run graph's
nodes and edges: the Artifacts its evals use on the left, the Artifacts whose evals
use it on the right. The drawing holds still; a click on a neighbour opens that
neighbour's Detail. Below it are its evals with their verdicts (click one to show
its chip) and its path; every other fact, what it waits for included, folds under
**Debug**.

An eval's chip shows its **Summary**: its status, with what running it took and
spent (time, tokens in `k` or `M`, model) or what reuse saved; its instruction; its
result, with findings as numbered items headed by their `item` and `where`; then
**Execution options**, **Execution metadata** and **Fingerprint** when it has them.
Other facts fold under **Debug**, which `t` shows or hides on an Artifact's or a
non-Human eval's Summary. An eval with a log switches
below the chips between its Summary, shown first, and its log, a plain-text choice
whose chosen view is bold and higher-contrast: an Agent eval's **Agent log** (its conversation,
with its own scrollbar) or a runtime eval's **Runtime log** (saved stdout/stderr,
not a live log stream). Missing or remote conversations explain why they are
unavailable on this machine. A Human eval's chip holds its review and sign-off, with
the controls described below. An eval outside the Run's selection, such as one a
partial `verify` left out, has no request in that Run: its chip shows its sections
read-only, with no Summary of a result, no Human controls and no Tool log. A Run's Detail keeps its sections, where `t` toggles
Technical and `w` or the pending-evals control reveals the unfinished checks behind
an upstream wait.

Agent logs update live. In a focused Agent log, ↑/↓ or `j`/`k` moves by row,
PgUp/PgDn by page, Home/End or **Top/Bottom** goes to either end, and the mouse wheel
scrolls. Moving up pauses following; return to Bottom to follow new messages.
Space or Enter toggles the visible grouped content; clicking a group header also
expands or folds it. Only public reasoning summaries are shown, not hidden reasoning.

### Run graph

Monitor always displays the selected Run as an Artifact graph; there is no
Run-tree mode. Node and relation colors show review states and update as results
arrive. Inspect dependencies through the graph's input-to-consumer relations and
**Waits for** under an Artifact's or eval's **Debug**. The graph takes the pointer: Tab reaches its
Artifacts and Enter on one opens its Detail, as a click does; it has no other
keys of its own.

Drag the graph background to pan; wheel, pinch or a background double-click
zooms the camera. **Zoom In**, **Zoom Out** and **Fit View** also change its
viewport. These gestures move the camera, not the pane dividers or the Detail.
Above 40
nodes, a pannable, zoomable minimap helps navigate the graph. Runs with **more
than 300 Artifacts** ask before layout: choose **Lay out all** to display the
whole graph. Above **80 Artifacts**, only elements in view are rendered: panning or
zooming draws those it brings into view. Smaller graphs render every element.
Layout is not an incrementally loaded neighborhood, and there is no tree fallback.

`artifactize-desktop graph --repo PATH` opens the **current** Artifact Graph
without needing a Run or showing saved verdicts. A click on an Artifact shows its
evals over the graph's right side; Esc, **Close** or a click on the background
closes them. Expand **Structure and evaluation
criteria** for the full tree, criteria and labeled relations. Refresh or Ctrl+R/F5
reloads declarations. A Human `graph` tool opens this viewer; opening it approves
nothing.

## Review

Review starts with waiting Human requests across all repositories in this state.
A selected request previews read-only. Arrow keys or `j`/`k` select request rows,
and Enter or → opens the selected request; Esc or ← returns from a read-only Detail
to the list. Other eval kinds and
settled requests remain read-only. The context menu or palette can open a Human
request with Tools focused; Ctrl+O opens a request by ID. PgDn/PgUp pages older
and newer requests. Waiting followers open the original review, not another sign-off.
One click selects a request; clicking an already-selected request opens it, as
does a double-click. Clicking its preview also opens Detail. A read-only request
with a log switches between its Summary and its Agent or Runtime log, as an
eval's chip does in Monitor, and `t` shows or hides its **Debug** facts.

`/` edits search from the Review request list. Ctrl+F focuses search from an
open request Detail too, including while a Human form is open or review work is
running. Clicking the Requests pane or switching screens also leaves the Detail
without closing it: an open form keeps its draft and running review work goes on.
Free text and `key:value` terms must all match:
`repo`, `eval`, `request`, `reviewer`, `tag`, `kind`, `status` and `is`. A plain
word matches part of the repository, eval, request ID or claimant, ignoring case;
`repo:`, `eval:`, `request:`, `reviewer:` and `tag:` match part of that one field
the same way, so `eval:sign` finds `release/signoff`. `kind:` and `status:` take
the eval kind and request status; `is:mine`, `is:claimed` and `is:unclaimed`
filter Human claims. Quote a value with spaces (`repo:"/work/my repo"`); the
[terminal review](../tui/index.md#review) uses the same search, term by term. Suggestions complete
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

A Human review reads top to bottom as its instruction, the **Tools** to use, their
**Tool log**, and the **Result** to write. The instruction is shown in full within
the review body's shared scroller; scroll that body to read long criteria.
`t` toggles the Human request's Technical record. Tab or a click
moves to Tools; Ctrl+U releases your claim even while editing.

Tools are grouped under the Artifact that declares them, the reviewed one first.
Each is a `▶ operation` button with its description, and the reason when it cannot
run. **A click, or Enter or Space on the focused button, runs it immediately**; Tab
moves between them. Opened with Tools focused (from the Review list's context menu or
palette), the Tools take the keyboard. A launch tool opens its application and
returns at once; a failing tool says so without a verdict. The saved definition must
still match before an action runs.

The Tool log shows every run of this review as it happens: `▶ HH:MM:SS · operation ·
Artifact` in local time, then each line the tool prints to stdout, or in red to
stderr, then its end line, such as `■ exit 0`, `■ launched`, `■ done`,
`■ cancelled` or `■ error: …`. A launched application's output keeps arriving
under its own run until it exits, even while you write the result. The log wraps,
scrolls on its own and follows the newest line; scroll up to read and it stays,
scroll back to the bottom and it follows again. Its text is ordinary selectable
text, so copy what a tool printed into the result. It lasts while the app runs and
is cleared by a submission; nothing is saved.

After claiming, `g` opens GREEN and `r` RED, each a form built from the eval's
schema ([the shapes it may use](../features/declarations.md#human-result-schemas)):
a text box, a number box, true/false, a choice, a list of text with **+ Add** and
✕ per item, or repeated groups of fields with **+ Add** for a group and ✕ per
group (one empty group always stays to type in). Constants are filled in and not
shown; `*` marks a required field. An empty optional text or number field, a text
list item included, says *Optional* in grey; an optional yes/no or choice shows `–`
while unset. Tab moves between controls. Ctrl+G/Ctrl+R switches verdicts. Enter in a field
does not submit; Ctrl+S or activating the **Submit** button submits, including
Enter while that button has keyboard focus.
Schema errors identify the failing path and leave the form correctable. Input-method
composition, including Korean, belongs to the text field and never fires shortcuts.
Esc cancels active work, then leaves the form, then leaves Detail.
Leaving a form keeps its verdict's draft; reopening it restores that draft, and
GREEN and RED keep separate drafts. Monitor and Review share the same request's
review state, Tool log included, within this app session, rather than creating
independent forms. This holds while the state stays the same: when the app finds its
state database replaced, it closes open Details, forgets cached reviews, drafts and
Tool logs, and cancels running Human tool and submit jobs.

A completed result stays open. Remote publication can fail **after local
settlement**: the form closes and a saved-locally notice explains the failure.
The verdict is already recorded; retry publication with Settings' **Push results**
(or `artifactize remote push`), not by submitting again.

### Leave and quit

`q` quits outside a text field, except while the keyboard is in a Human Detail that
is editing or busy: leave or cancel that work first. With the keyboard elsewhere,
such as after closing Commands, `q` starts quitting even while that review stays
open. Closing the window also starts quitting. If
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
  If another process holds port 1455, the app explains that a paste is required:
  copy the full `http://localhost:1455/auth/callback?code=…` URL from the browser's
  address bar after a failed load, or DevTools → Network if it keeps loading,
  paste it in Settings, then choose **Finish sign-in**.
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
