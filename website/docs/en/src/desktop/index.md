# Desktop

Open the app to see, on one screen, every live branch of every project with Runs on
this machine and every Human review waiting for a person. Browse Runs, see what each
branch would run now, inspect evidence and handle Human sign-offs. The app works on the
same state as the CLI and TUI, so a change made in one shows in the others. Every release ships it beside the
commands; see [Desktop app](../getting-started/install.md#desktop-app) to install it.

For first-launch command setup, PATH details, updates and cleanup, see the
[installation guide](../getting-started/install.md#desktop-app).

## Open the app

```sh
artifactize-desktop
artifactize-desktop monitor --repo PATH
artifactize-desktop review [REQUEST_ID]
artifactize-desktop graph --repo PATH
```

Without arguments the app opens with nothing chosen, whatever the current directory.
`--repo PATH` selects and expands the branch of that folder and shows its newest entry; `--all`, the
default, selects none, and every branch is listed either way. `review` opens the
newest waiting Human review, `review REQUEST_ID` that request. `graph` requires
`--repo` and shows that branch's [Current](#current). `--state-dir PATH` chooses a
state directory; otherwise the app uses the shared state home. `--reviewer NAME`
sets the cooperative claim name (normally the user name). `-h`, `--help` or `help`
(also after a mode) prints help on stderr and exits 2, as invalid arguments do. The app
exits 0 when you quit and 1 if it cannot start.

Shortcut labels below use Ctrl; macOS uses its primary modifier, Command, instead.
Ctrl+K, F1 or the **Commands** button opens a searchable command palette; Enter runs a
command and Esc closes it. Ctrl+, or the **Settings** button shows
[Settings](#settings) in the Main area. Ctrl+O opens a request by ID. `?` lists the
keys of what has focus. Ctrl+R or F5 reloads the state from disk, scans the branches
again and, on a Current, scans its worktree again. While a Human review has the
keyboard and runs nothing, Ctrl+R is its RED command instead, with or without its
form open: it opens or switches to RED once claimed, and says the review must be
claimed first otherwise. There, F5 or the palette's **Refresh** reloads the state.
The app follows the system's light or dark appearance.

Hovering only highlights; information opens on a click.

## Pane

The window is one screen: the **Pane** on the left, the **Main area** beside it.
The Pane is one column: a search field at the top, the branches below it, and the
reviews waiting for a person at the bottom. Drag the divider between the Pane and
the Main area to resize them; the split stays as dragged whatever the Main area shows.

### Branches

A branch is a line of work in a Git repository. A worktree is a folder where one
branch is checked out; a repository can have several, each on its own branch, or
detached: on a commit rather than a branch. Each entry reads as its project and
branch, such as `artifactize / feat/x`: a worktree that exists now with the branch it
has checked out (a detached one by its folder's name), or a folder outside Git that
exists now and holds its declarations.
Its projects are those with Runs on this machine, and the one the app was started for
with `--repo` or `graph --repo`, listed even before its first Run. A worktree or folder that was
removed, or one Git no longer lists, is not shown; worktrees added or removed appear
or go within seconds, without a reload. Who ran a Run, an agent session or a person,
makes no entry of its own: a branch's Runs are that project's Runs on that branch,
from whichever folder they ran in.

A folded branch shows its newest Run under its name: a dot in its status colour,
its status and how long ago it ran (a running one, its time so far). Click a branch
to expand it and show its newest entry; the chevron before its name expands or folds
it without showing anything. Expanded, its timeline lists its Runs newest first, each
with its status, age and Run ID. The first entry is **Current**, the worktree as it is
now; while a Run is in progress, that Run takes Current's place, and Current returns when
the Run ends. **Older runs** at the end brings
older Runs. A click on an entry shows it in the Main area, in a grey pill. Any number
of branches can be expanded; the one the app was started for starts expanded.

The ☆ at the end of a branch's name stars it: starred branches are listed first,
each group in the search's order, and stay starred the next time you open the app.
Click the ★ again to take the star away.

↑/↓ or `j`/`k` move through the branches, the timelines of expanded ones and then
the waiting reviews, showing each branch's newest entry or the entry reached. →
expands the selected branch or enters its timeline; ← steps back out or folds it. ↓
past a timeline's oldest entry brings older Runs. Enter moves into the Main area, and
Esc there, with no Detail open, returns to the Pane.

### Search

`/` or Ctrl+F focuses the search, whose starting query is `sort:recent`. Free text
and `key:value` terms must all match, and they narrow the branches and the waiting
reviews alike. A plain word matches part of a project, a branch or a waiting review's
eval, ignoring case; `repo:` and `branch:` match part of that one field.
`sort:recent` puts the branch with the most recent Run first and branches without
Runs last; `sort:name` orders them by project and branch. Suggestions complete keys
and values; in an empty field or after a space, ↓ opens the key suggestions. ↑/↓ chooses one, Tab or Enter takes it, and a click takes it with the
mouse. Enter without an open suggestion returns to the list. A query that cannot be
read, such as an unknown key, shows its error in place of the branches, and neither list
shows entries until it is fixed. Esc closes suggestions, then puts back `sort:recent`,
then returns to the list.

### Waiting reviews

The bottom of the Pane lists the waiting Human reviews of every repository on this
machine, those of removed branches included, newest at the top, each with its eval,
its project and branch, how long it waits and who claimed it. With none waiting it
is a thin bar reading **Waiting (0)**; each waiting review adds a row and the area
grows upward like a stack, up to half of the Pane's height, beyond which the list
scrolls. A click or Enter on one opens the [review](#human-reviews) in the Main area.
Followers of one Human execution wait as one review: opening a follower opens the
original review.

New Runs, results, claims and submissions made elsewhere, and worktrees added or
removed, appear without a manual reload.

## Main area

The Main area shows what was chosen in the Pane: a Run, a branch's Current, a
review, a request opened by ID, or Settings. With nothing chosen, while loading, on
an error or with an empty graph, the graph's place stays and says so. A problem
reading this machine's records, such as other Runs that cannot be read, shows as one
line in the Main area's header without moving or hiding the graph; a click on it shows
it in full. After such a failed read, what was last read stays on screen and can be
stale, so refresh (Ctrl+R) once its cause is fixed. The chosen Run itself turning
unreadable, or its graph failing to load, is different: a message takes the graph's
place, and you choose another. Damaged records are
[marked unreadable](../features/others/state.md#unreadable-records), not treated
as successful evidence.

### Run

A Run chosen in the Pane fills the Main area with the graph of its Artifacts and their verdicts,
under the Run's headline and what needs attention. Node and relation colours show
review states (GREEN, running or waiting for a Human, waiting, RED, ERROR, blocked)
and recolour in place as results arrive. Inspect dependencies through the graph's
input-to-consumer relations and **Waits for** under an Artifact's or eval's
**Debug**. Run-end validation stays historical: a later Human submission can change
the current request display without rewriting the old verdict. Click the Run's
headline, or use the palette command **Open the Run's Detail**, for the Run's own
Detail: saved validation and unmet obligations, request counts, waiting work,
timing, budgets, executed/reused work and usage. There `t` toggles Technical and `w`
or the pending-evals control reveals the unfinished checks behind an upstream wait.

### Current

A branch's **Current** scans its worktree as it is now, as
[`artifactize status`](../features/run.md#status-and-static-graph) does, and draws
the graph of its current declarations without needing a Run. Each Artifact is coloured
reused (every eval's result would be reused without evaluation), would run, waiting or
blocked; like a Run's Artifact, its node shows only its name, and its accessible name
says which, with how many of its evals would run. Its header says when it was scanned. It scans again
when chosen again, on **Refresh** or Ctrl+R/F5, and when a Run of the branch ends.
Expand **Structure and evaluation criteria** for the full tree, criteria and labeled
relations as text. A Human [`graph` tool](../features/tools/builtin.md#current-project-graph)
and `artifactize-desktop graph --repo PATH` open the app on this view; opening it
approves nothing.

### Graphs

Both graphs, a Run's and the Current, take the pointer: Tab reaches their named
Artifacts and Enter on one does what a click does; they have no other keys of their
own.
Drag the graph background to pan; wheel, pinch or a background double-click zooms
the camera. **Zoom In**, **Zoom Out** and **Fit View** also change its viewport.
These gestures move the camera, not the divider or the Detail. A graph shows whole
when it first appears; from then on only you move
its camera. Opening, switching, resizing or closing the Detail, a new result or scan
and a new layout never zoom or pan it; **Fit View** shows the whole graph again in
the whole canvas, the same whether the Detail floats over it or not.

Every graph is laid out at once, upstream on the left. An Artifact that a great many
others use (such as one criteria file every review reads) stands in its own column on
the left, its edges still drawn to each Artifact that uses it. Above 40 nodes, a
pannable, zoomable minimap helps navigate the graph. **More than 500 Artifacts:** the
graph has no minimap, and while more than 500 are in view they are colored boxes without names
that open only with a click. Zoom in until fewer are in view to see names again and reach
them with Tab and Enter. When a Run changes, the graph's colors update in
place; it is arranged again only when Artifacts or edges change. An Artifact's Detail draws at most 16
neighbours on each side, failing ones first, and counts the rest.

### Detail

A click on an Artifact opens its **Detail**, floating over the graph's right side
with the graph still visible behind it, never beside the Pane. It leaves the camera
where it is: pan to bring an Artifact under it into view.
Clicking another Artifact switches
the Detail to it; clicking the graph's
background, **Close**, Esc or ← closes it. On a Human review's Detail, ← leaves only
when you are not editing or running something; the others step back the same way: they cancel active work first, then leave the form, and only then
close the Detail. Drag the Detail's left edge to resize it, between a readable column
and leaving some graph visible; it keeps that width, also in the next launch.

An Artifact's Detail has chips that read as a path, `[icon] {name} / [eval] …`: the
Artifact's own chip, then one per eval with its verdict's glyph and colour, on a
Current whether it would run or be reused.

The Artifact's chip shows it between its neighbours, drawn with the graph's nodes and
edges: the Artifacts its evals use on the left, the Artifacts whose evals use it on
the right. The drawing holds still; a click on a neighbour opens that neighbour's
Detail. Below it are its evals with their verdicts (click one to show its chip), its
**Tools**, and its path; every other fact, what it waits for included, folds under
**Debug**.

The **Tools** are the [Human tools](../features/tools/human.md) the Artifact
declares, as `▶ operation` buttons with their own **Tool log**, like a review's
(below). A click runs one at once in the branch's worktree as it is now, whichever
Run is shown, with no claim, and records nothing as a review's evidence. While one
runs, the Artifact's other tool buttons wait and a **Cancel** button beside its name
stops it; closing the Detail does not stop it, and its output keeps arriving in the
Artifact's Tool log, shown when the Detail opens again. Its runs stay in that Tool
log while the app runs. The graph's nodes carry no tool buttons.

An eval's chip shows its **Summary**: its status, with what running it took and
spent (time, tokens in `k` or `M`, model) or what reuse saved; its instruction; its
result, with findings as numbered items headed by their `item` and `where`; then
**Execution options**, **Execution metadata** and **Fingerprint** when it has them.
On a Current it shows whether the eval would run or be reused and why, and the result
it would reuse. Other facts fold under **Debug**, which `t` shows or hides on an
Artifact's or a non-Human eval's Summary. An eval with a log switches below the chips
between its Summary, shown first, and its log, a plain-text choice whose chosen view
is bold and higher-contrast: an Agent eval's **Agent log** (its conversation, with
its own scrollbar) or a runtime eval's **Runtime log** (saved stdout/stderr, not a
live log stream). Missing or remote conversations explain why they are unavailable on
this machine. A Human eval's chip in a Run holds its review and sign-off, with the
controls described below. An eval outside the Run's selection, such as one a partial
`verify` left out, has no request in that Run: its chip shows its sections read-only,
with no Summary of a result, no Human controls and no Tool log.

Agent logs update live. In a focused Agent log, ↑/↓ or `j`/`k` moves by row,
PgUp/PgDn by page, Home/End or **Top/Bottom** goes to either end, and the mouse wheel
scrolls. Moving up pauses following; return to Bottom to follow new messages.
Space or Enter toggles the visible grouped content; clicking a group header also
expands or folds it. Only public reasoning summaries are shown, not hidden reasoning.

### Requests by ID

Ctrl+O, or the palette's **Open a request by ID**, opens a saved request of any
repository in the Main area: a waiting Human review actionable, as from the Pane, and
any other request read-only. A finished Human request opens read-only in the Human
review view, where `t` shows or hides Technical. Another read-only request with a log
switches between its Summary and its Agent or Runtime log, as an eval's chip does, and
`t` shows or hides its **Debug** information; **Close** returns to the Pane. It follows its request's changes
made elsewhere, and Ctrl+R or F5 reads it again, without opening it again.

## Human reviews

A waiting review opened from the Pane, or by ID, fills the Main area as a Human
review, its eval and Artifact above it. Choosing a branch or a timeline entry leaves it without
closing it: an open form keeps its draft and running review work goes on. Esc,
after cancelling running work and leaving the form, returns to the Pane.

Opening a review claims nothing. Press `c` to claim and `u` to release. Another
person's claim prevents your actions. Claims are cooperative reviewer locks, not
authenticated accounts; only the claimant can run tools or submit. See
[Human](../features/human.md) for shared lifecycle and deferred sign-offs.

A Human review reads top to bottom as its instruction, the **Tools** to use, their
**Tool log**, and the **Result** to write. The instruction is shown in full within
the review body's shared scroller; scroll that body to read long criteria.
`t` toggles the Human request's Technical record. Tab or a click
moves to Tools; Ctrl+U releases your claim even while editing.

Tools are grouped under the Artifact that declares them, the reviewed one first.
Each is a `▶ operation` button with its description, and the reason when it cannot
run. **A click, or Enter or Space on the focused button, runs it immediately**; Tab
moves between them. A launch tool opens its application and returns at once; a
failing tool says so without a verdict. The saved definition must still match before
an action runs.

The [Tool log](../features/tools/human.md#tool-log) shows every run of this review
as it happens, its start times in local time. The log wraps,
scrolls on its own and follows the newest line; scroll up to read and it stays,
scroll back to the bottom and it follows again. Its text is ordinary selectable
text, so copy what a tool printed into the result. It lasts while the app runs and
is cleared when you submit in this window; a submission made with the CLI or in
another window ends the editing here but leaves this window's log shown. Nothing is
saved.

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
Esc cancels active work, then leaves the form, then leaves the review.
Leaving a form keeps its verdict's draft; reopening it restores that draft, and
GREEN and RED keep separate drafts. Opening the same request from the Pane or from
its chip in a Run's Detail shows the same draft and Tool log. If the state directory is
replaced, for example by restoring a backup, open reviews close, running Human tools and
submissions are cancelled, and unsaved drafts and Tool logs are discarded.

A completed result stays open. Remote publication can fail **after local
settlement**: the form closes and a saved-locally notice explains the failure.
The verdict is already recorded; retry publication with Settings' **Push results**
(or `artifactize remote push`), not by submitting again.

### Leave and quit

`q` quits outside a text field, except while the keyboard is in a Human review that
is editing or busy: leave or cancel that work first. With the keyboard elsewhere,
such as after closing Commands, `q` starts quitting even while that review stays
open. Closing the window also starts quitting. If
this window took Human claims it has not submitted or released itself, choose
**Stay** (Esc), **Release and quit** (`u`) or **Keep and quit** (`k`). Keeping a
claim leaves it held for this reviewer; it is not a verdict. The prompt lists the
claims this window took, as this window last saw them: a claim released or settled
from the CLI or another window can still be listed, and a claim taken before the
window opened is not. Quitting cancels tools that are still producing output and any
review work in progress; programs a launch tool opened keep running, so close them
yourself.

## Settings

Settings shows in the Main area; the **Settings** button again or Ctrl+, again
returns to what the Main area showed, and so does Esc outside a text field or a choice
list (in one, Esc stays with the field). Choosing a branch, chip or review in the Pane
instead shows that choice. It reloads account/store information
when shown. Reopen it or use the palette's **Refresh settings** command to reload it.

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
  token**; a store administrator must [revoke it](../cli/review-store.md#review-store-server).
  See [sharing and push behavior](../cli/review-store.md#remote-review-store-client).

Settings uses the same accounts and store operations as the CLI; credentials do
not belong in `.artf`.
