# Desktop

Open the app to see, on one screen, every live branch of every project with Runs on
this machine and every Human review waiting for a person. Browse Runs, see what each
branch would run now, inspect evidence and handle Human sign-offs. The app works on the
same state as the CLI and TUI, so a change made in one shows in the others. Every release ships it beside the
commands; see [Desktop app](../getting-started/install-options.md#desktop-app) to install it.

For first-launch command setup, PATH details, updates and cleanup, see the
[installation guide](../getting-started/install-options.md#desktop-app).

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
newest waiting Human review, `review REQUEST_ID` that request; it selects and expands no
branch, even with `--repo`. `graph` requires
`--repo` and shows that branch's [Current](#current). `--state-dir PATH` chooses a
state directory; otherwise the app uses the shared state home. `--reviewer NAME`
sets the cooperative claim name (normally the user name). `-h` or `--help`, also
after a mode (`artifactize-desktop monitor --help`), and `artifactize-desktop help [MODE]` print
help on stderr and exit 2, as invalid arguments do. The app
exits 0 when you quit and 1 if it cannot start.

Ctrl+K, F1 or the **Commands** button opens a searchable command palette of every command
you can run now, with its key; Enter runs one and Esc closes it. `?` opens Help: the keys of
what has focus and, on a graph, what each mark means. The window's keys (macOS uses Command
for Ctrl):

| Key | Does |
|---|---|
| Ctrl+K, F1 | Command palette |
| `?` | Help |
| Ctrl+, | [Settings](#settings), and back |
| Ctrl+B | Hide or show the [Pane](#pane) |
| `/` | [Search](#search) the branches |
| Ctrl+O | [Open a request by ID](#requests-by-id) |
| Ctrl+R, F5 | Reload the state and scan the branches (and a shown Current) again; in a Human review, Ctrl+R is RED, so use F5 |
| `q` | [Quit](#leave-and-quit) |

The app is always dark, whatever the system's appearance.

When a [newer release](../getting-started/update.md#newer-releases) is out, the header
shows `VERSION available` before **Settings**. A click on it opens a dialog with this
app's version and how to update the way it was installed: the command to run, as text
you can select and copy, or the file to download and install by hand, or, when the way
is unknown, the release page. A link opens in your browser; Esc or **Close** closes the
dialog. The app looks the release up when it starts and once a day while it runs, and
never updates itself. A Microsoft Store install never shows it: the Store updates the app.

Hovering only highlights; information opens on a click. The one exception is a box's **Minimize** or **Expand** button, whose tooltip names its key, `m`.

## Pane

The window is one screen: the **Pane** on the left, the **Main area** beside it.
The Pane is one column: a search field at the top, the branches below it, and the
reviews waiting for a person at the bottom. Drag the divider between the Pane and
the Main area to resize them; the split stays as dragged whatever the Main area shows.

Ctrl+B or the **Pane** button in the header hides the Pane, giving the Main area the
window's width, and shows it again at the width it had. With the Pane hidden, the keyboard
is in the Main area, and `/` or Esc back to the Pane shows it first.

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
with its status and age and, below, the [comment](../cli/index.md#submission-and-watching)
its verify was requested with, cut short to one line, or else its Run ID. The first entry is **Current**, the worktree as it is
now; while a Run is in progress, that Run takes Current's place, and Current returns when
the Run ends. **Older runs** at the end brings
older Runs. A click on an entry shows it in the Main area, in a grey pill. Any number
of branches can be expanded; the one `monitor` or `graph` was started for starts expanded.

The star at the end of a branch's name stars it, filled while it is on: starred branches
are listed first, each group in the search's order, and stay starred the next time you open
the app. Click the filled star again to take the star away.

↑/↓ or `j`/`k` move through the branches, the timelines of expanded ones and then
the waiting reviews, showing each branch's newest entry or the entry reached. →
expands the selected branch or enters its timeline; ← steps back out or folds it. ↓
past a timeline's oldest entry brings older Runs. Enter moves into the Main area, and
Esc there, with no Detail open, returns to the Pane, showing it first when it is hidden.

### Search

`/` focuses the search, showing the Pane first when it is hidden, and so does Ctrl+F in the
Pane or while no graph is shown; over a
graph, Ctrl+F [finds in the graph](#find-in-the-graph) instead. The starting query is
`sort:recent`. Free text
and `key:value` terms must all match, and they narrow the branches and the waiting
reviews alike. A plain word matches part of a project, a branch or a waiting review's
eval, ignoring case; `repo:` and `branch:` match part of that one field. Quote a value
that has spaces, as in `repo:"my project"`: spaces inside the quotes belong to the value
instead of starting another term.
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
its project and branch, how long it waits and, once claimed, who claimed it. With none waiting it
is a thin bar reading **Waiting (0)**; each waiting review adds a row and the area
grows upward like a stack, up to half of the Pane's height, beyond which the list
scrolls. A click or Enter on one opens the [review](#human-reviews) in the Main area.
Followers of one Human execution wait as one review: opening a follower opens the
original review. A review of a removed branch stays listed while its repository folder
exists; once the folder is deleted, its row goes, unless another worktree still waits on
the same review, which then lists it under that worktree.

New Runs, results, claims and submissions made elsewhere, and worktrees added or
removed, appear without a manual reload.

## Main area

The Main area shows what was chosen in the Pane: a Run, a branch's Current, a
review, or a request opened by ID, under one header row that names it and says its
headline. With nothing chosen, while loading, on an error or with an empty graph, one
line at the top says so and the graph's place stays empty. A problem
reading this machine's records, such as other Runs that cannot be read, shows as a few
red words at the header row's end without moving or hiding the graph; a click on them
shows the problem in full, and another click folds it again. After such a failed read, what was last read stays on screen and can be
stale, so refresh (Ctrl+R) once its cause is fixed. The chosen Run itself turning
unreadable, or its graph failing to load, is different: a message takes the graph's
place, and you choose another. Damaged records are
[marked unreadable](../features/others/state.md#unreadable-records), not treated
as successful evidence.

### Run

A Run chosen in the Pane fills the Main area with the graph of its Artifacts. The header
row holds the Run's ID and its headline: its verdict's mark and word in its colour, then
each eval that needs attention, every RED or ERROR eval and every sign-off that waits, by
its mark and name, cut at the row's end. Click the headline, or use the palette command
**Open the Run's Detail**, for the [Run's Detail](#detail): its outcome (its status, the
evals that failed, how many evals ended in each status, how long it took and its comment in
full) and what it waits for, then **What**, **Provenance** and **Technical**, folded until
you unfold them; `t` shows or hides **Technical**. Run-end validation stays historical: a
later Human submission can change the current request display without rewriting the old
verdict.

### Current

A branch's **Current** scans its worktree as it is now, as
[`artifactize status`](../features/run.md#status-and-static-graph) does, and draws
the graph of its current declarations without needing a Run. Each Artifact first shows as
checking, then shows what verify would do with it now: reused (every eval's result would be
reused without evaluation), would run, waiting or blocked, and how many of its evals would be
reused of how many. Read aloud, an Artifact says which, with how many of its evals would run.
While the scan runs, the header row shows the checking mark and how many Artifacts are left
to check (`3 left`); once it ends, the time it scanned at; if it fails, why. **Refresh** sits
at that row's end.
When the Current scans:

- Choosing it again, or a Run of the branch ending: it scans again; during a scan, that
  scan goes on.
- **Refresh** at the row's end: it scans again; disabled during a scan.
- The palette's **Refresh** or Ctrl+R/F5: it scans again, after a running scan finishes.
- A Run ending while your search hides the branch: press **Refresh**.

A Human [`graph` tool](../features/tools/builtin.md#current-project-graph)
and `artifactize-desktop graph --repo PATH` open the app on this view; opening it
approves nothing.

### Editing the Current

Only a branch's Current can be edited; a Run's graph is a record. **New Artifact** in the
graph's tool strip asks for Name and Path, writes `.artifactize/<name>.artf`, and opens
its Detail. Targets may be defined before they exist. Invalid names or paths say why
without writing anything.

On an Artifact's tab, **Definition** replaces a Run's **Path**. It shows the definition
file, an editable Path, Parts, Comment, Rename and Delete. Enter saves a changed Path;
Esc puts it back. **+ Add** under Parts suggests Artifacts by name; Enter adds one,
Esc closes it, and **✕** removes a part. Cycles are refused. **Source**, folded at first,
contains the complete file: edit evals, tools and fingerprints there. Typing shows
**Save** and **Revert**; Save or Ctrl+S writes it, and Revert reloads the file.

Rename changes the filename in place and updates structural references and saved part
positions together. Delete names the file it would remove; while other Artifacts name
it, it refuses and lists them. Every edit validates the project first. An invalid edit
writes nothing and leaves the text for correction, with a file, line and column for a
Source error. There is no app undo; these are worktree files, and Git keeps their history.

#### Placing nodes and parts

Drag a node, a minimized whole or a whole's title row to place it. A whole carries its
parts and comments. A click without movement still opens Detail, background dragging
pans, and Esc during a drag puts it back without saving. On a Run, dragging only pans.
Placement and part-membership dragging require at most 500 drawn nodes, counting each
copy of a shared part. Above that limit, zooming in does not enable these gestures;
use Definition or Source to change membership instead.

A box and the top level are automatic until something in them is moved. The first
top-level move saves every top-level node's shown position in one edit. A whole keeps
its parts' positions in its own definition; each top-level node keeps its own. Saved
places survive reopening and also draw this branch's Runs. New nodes appear in free
space. **Reset layout** in a whole's title row or the Current's tool strip returns that
box or the top level to automatic; inner boxes keep their own positions.

Drop onto a node or into a whole's box to make it a part; a valid target highlights.
Moving between boxes removes it from the old whole and adds it to the new one in one
edit. Dropping outside every box removes that membership. For a shared part, only the
dragged copy's whole changes; add another whole through Parts in Detail. A cycle is
refused and the node returns. References are words in evals, not draggable edges:
change them in Source.

#### Node comments

A node's plain-text comment appears below it, or below a whole's title inside its box,
and below every copy of a shared part. The graph shows its first three lines, then an
ellipsis; minimized parts and nodes drawn as blocks do not show comments. This note is
separate from a Run's submission comment and never changes verification or reuse.

On a Current, **Comment** in Definition or a click on the note opens its full text in
place. Enter saves; Shift+Enter adds a line; saving empty text removes it. Esc or a
click outside closes without saving. Drag the note onto another Artifact without a
comment to move it between their files in one edit. A background, its own node or an
occupied target leaves it where it was; Esc cancels. On a Run, clicking a comment opens
its node's Detail and never edits it.

#### Conflicts with file edits

If a terminal or agent changed a file since the Current read it, an edit writes nothing,
names that file and rescans. Rescanning the same branch keeps typed Source text until
you Save or Revert. Save before switching branches or renaming that Artifact: those
actions discard its unsaved Source. A graph move, drop, Reset layout or comment that would write a
file with unsaved Source is also refused until you save or revert that Source. Untouched
text and unknown annotation values are retained. See [positions and comments](../features/declarations.md#positions-and-comments)
for their file form.

### Graphs

Each node shows its status as a mark and a coloured strip, then how many of its evals are
GREEN (`2/3`; on a Current, how many would be reused). **Help** (`?`) lists what each mark
means. An edge runs from an input to its consumer and is grey unless it leaves a RED, ERROR
or blocked Artifact. Nodes and edges update in place as results arrive.

Both graphs, a Run's and the Current, work the same way:

| Control | Does |
|---|---|
| Click, or Tab to an Artifact and Enter | Opens its Detail |
| **Find**, Ctrl+F | [Finds an Artifact or an eval, or lists your favorites](#find-in-the-graph) |
| `m` | Minimizes or expands a box (below) |
| **Zoom out**, **Zoom in**, wheel, pinch, background double-click | Zoom, up to 200% |
| **Fit** | Shows the whole graph, never above 100% |
| Drag the background | Pans |

A graph shows whole when it first appears. After that, only you move the camera: new
results, scans, layouts and the Detail leave it where it is.

Whenever part of a graph lies outside the view, a **Navigator** at the graph's bottom right,
left of the Detail when one is open, shows the whole graph in small, each Artifact a block of
its status's colour, with the part in view framed. A click or a drag in it moves the camera
there, the zoom kept. It goes when the whole graph is in view.

Every graph places inputs on the left. In a graph of **more than 500 drawn nodes**
(shared copies each count), zooming out until names are too small to read replaces
nodes with blocks in their status's colour and whole boxes with outlines. At a readable
zoom, nodes keep their names, icons, tallies and comments even when more than 500 are in
view, but individual node controls are unavailable: Tab and Enter, mouse Minimize/Expand,
and in-graph comment editors. Clicking a node or block, including a pictured Minimize/Expand
control, opens Detail instead. Zoom or pan until names are readable and at most 500 nodes
are in view to restore these controls. Alternatively, open a whole's Detail and press `m`
to minimize or expand it, or edit a comment in Source. Detail shows at most 16 neighbours
on each side and counts the rest.

An Artifact made of [parts](../features/artifact.md#composition) is drawn as a box around
its parts, under a title row with its mark, its name and its count, closed by the strip in
its status's colour; a part with parts of its own is a box inside it.
The box is that Artifact: a click on the box outside its parts, or Tab and Enter, opens its
Detail, and a click on a part opens the part's. A part of several Artifacts is drawn inside
each of them. Opening one of its copies highlights them all, and its Detail says how many
times it is drawn and inside which Artifacts.

A box has a **Minimize** button at its top right, reached with Tab after the box; its
tooltip names its key, `m`. It draws that Artifact as a node of the usual size, with its
mark, its name, how many parts it has and an **Expand** button, and hides its parts; their edges meet the node instead, and edges among them are not
drawn. The node opens its Detail like any other, and Find still lists what it hides.
**Expand** draws the box again. `m` does the same for the box you reached with Tab, or else
for the box whose Detail is open; it does nothing while you type or the keyboard is in the
Pane. An Artifact stays minimized on every graph of its branch until you expand it or quit
the app.

### Find in the graph

With a Run's graph or a Current shown, the tool strip's **Find**, or Ctrl+F with the
keyboard outside the Pane (on the graph or in the Detail), opens a find field over the
graph's top left. It lists the
Artifacts and evals whose names contain what you type, ignoring case, in the graph's
order: an Artifact by its whole name and an eval as `Artifact / eval`, each with its
verdict's mark and colour (on a Current, what verify would do). It lists the first 50 and
says how many more match, or that none does.

With nothing typed, it lists your favorites: the graph's [starred Artifacts](#detail), in
the graph's order and the same way, as if you had found them. While none of the graph's
Artifacts is starred, it says how to star one.

↑/↓ choose an entry. Enter (on the first entry when none is chosen) or a click opens its
Detail, on the eval's tab for an eval, and centres the Artifact in the graph left of the
Detail, zoomed in until its name reads. Esc closes the field and gives the keyboard back; a
click outside it closes it too and acts as usual. Find reaches every Artifact, however large
the graph. A part drawn inside several Artifacts is listed once,
and opening it centres its first copy. A part inside a minimized Artifact stays hidden:
opening it opens its Detail and centres and highlights the minimized Artifact instead;
expand that one to see the part on the graph.

### Detail

A click on an Artifact opens its **Detail** at the graph's right edge. Clicking another
Artifact switches the Detail to it; clicking the graph's background, **Close**, Esc or ←
closes it. On a Human review's Detail, ← leaves only
when you are not editing or running something, and the others first step back as
[Esc does in a review](#human-reviews). Drag the Detail's left edge to resize it, between a readable column
and leaving some graph visible; it keeps that width, also in the next launch.

A click on a section's header row (or Enter or Space on it) folds or unfolds the section,
and every Detail remembers that, also in the next launch; a Human review's Tools, Tool log
and Result always stay open. Unfolded at first: **Evals**,
**Tools**, **Path**, **Definition**, **Result**, **Outcome** and **Waits for**. Folded at first:
**Neighbours**, **Source**, **Execution options**, **Execution metadata**, **Fingerprint**, **What**,
**Provenance**, **Debug** and **Technical**.

An Artifact's Detail opens on its own tab, titled with its name. **Evals** lists its evals,
each with its verdict's mark, its status in its colour and, on the right, its time in a Run
or its declared title on a Current; a click on
one opens that eval's tab. **Neighbours**, once unfolded, draws the Artifact between its
neighbours with the graph's nodes and edges: the Artifacts its evals use on the left, the
Artifacts whose evals use it on the right. The drawing holds still; a click on a neighbour
opens that neighbour's Detail. Then come its **Tools**, their **Tool log** once one has run,
its **Path** (or **Definition** and **Source** on a Current), and **Debug**, where every other fact, what it waits for included, folds.

On an eval's tab, chips under the title read as a path, `[icon] {name} / [eval] …`: the
Artifact's own chip, which leads back to the Artifact's tab, then one chip per eval with its
verdict's mark and colour, on a Current whether it would run, would be reused or is still
being checked.

The star beside the title of an Artifact's Detail, on any of its tabs, stars the Artifact:
it becomes one of your **favorites**, which Ctrl+F lists [with nothing typed](#find-in-the-graph).
An Artifact stays starred on every branch of its project, in Runs and Currents alike, and the
next time you open the app. Click the filled star again to take the star away.

The **Tools** are the [Human tools](../features/tools/human.md) the Artifact
declares, each a button with its operation and description, like a review's
(below). A click runs one at once in the branch's worktree as it is now, whichever
Run is shown, with no claim, and records nothing as a review's evidence; its run shows in the
Artifact's **Tool log**, which appears with the first run and unfolds if you had folded it.
What ended well is said by the log's end line; a failure is said under the tools. While one
runs, the Artifact's other tool buttons wait and **Cancel** beside the running mark
stops it; closing the Detail does not stop it, and its output keeps arriving in the
Artifact's Tool log, shown when the Detail opens again. Its runs stay in that Tool
log while the app runs. The graph's nodes carry no tool buttons. Tools, the star, and a
Current's Definition and Source editors show only while the Pane's search still lists the
branch. If the search hides it, clear or widen the search to bring them back.

An eval's tab opens on **Summary**: read its status, instruction and result, or on a
Current, what verify would do and the result it would reuse. The status line includes
duration, tokens abbreviated as `k`/`M`, model and the Runtime exit code. Reused duration
and tokens prefixed with `saved` are work avoided, not work spent by this Run.

Logs are available only for evals with recorded requests in Runs or requests opened by
ID. Below the chips choose **Agent log** for the conversation or **Runtime log** for
saved stdout/stderr. Current eval tabs have no logs, even when they show a reusable
result. An unavailable conversation says why it cannot be read on this machine. Return to
Summary to use `t` for **Debug**, also on Artifact tabs and in Human reviews.

A Run's Human eval opens its review and sign-off, described below. An eval outside that
Run's selection has no request: its sections are read-only, without a result Summary,
Human controls or Tool log.

Agent logs update live. In a focused Agent log, ↑/↓ or `j`/`k` moves by row,
PgUp/PgDn by page, Home/End or **Top/Bottom** goes to either end, and the mouse wheel
scrolls. Moving up pauses following; return to Bottom to follow new messages.
Space or Enter toggles the visible grouped content; clicking a group header also
expands or folds it. Only public reasoning summaries are shown, not hidden reasoning.

### Requests by ID

Ctrl+O, or the palette's **Open a request by ID**, opens a saved request of any
repository in the Main area: a waiting Human review actionable, as from the Pane, and
any other request read-only, its title marked read-only in the header row. A finished Human
request opens read-only in the Human review view, where `t` shows or hides **Debug**. Another
read-only request with a log switches between its Summary and its Agent or Runtime log, as
an eval's tab does, and `t` shows or hides its **Debug** in its Summary; **Close** returns to the Pane. It follows its request's changes
made elsewhere, and Ctrl+R or F5 reads it again, without opening it again.

## Human reviews

A waiting review opened from the Pane, or by ID, fills the Main area as a Human
review, its eval and title in the header row above it. Choosing a branch or a timeline entry leaves it without
closing it: an open form keeps its draft and running review work goes on.
Esc steps back one stage at a time: it cancels running work, then leaves the form, then
leaves the review, back to the Pane (showing it first when it is hidden) or, on a Detail,
closing the Detail.

A Human review opens at **CLAIM**, showing another claimant's name when there is one,
or **REVIEW (yours)** after you claim it. Read the instruction, use **Tools**, inspect
**Tool log**, then enter **Result**. Scroll to read long criteria. In wide windows the
log sits beside the form; in narrow windows and Detail tabs it sits above Result.
**Debug** starts folded; `t` or its header shows the request ID, mode, claimant and
waiting time.

Opening a review claims nothing. `c` or **Claim to review** claims it; `u` outside a text
field, or **Release claim**, releases it, also while you edit its form, whose draft is kept.
Another person's claim makes the review read-only until it is released. Claims are
cooperative reviewer locks, not authenticated accounts; only the claimant can run tools or
submit. See [Human](../features/human.md) for shared lifecycle and deferred sign-offs.

Tools are grouped under the Artifact that declares them, the reviewed one first, a group
named by its Artifact only when more than one declares tools. Each is a button led by the run
mark, with its operation, its description, and the reason when it cannot run; before a claim
they are disabled. **A click, or Enter or Space on the focused button, runs it immediately**; Tab
moves between them. A launch tool opens its application and returns at once; a
failing tool says so in the Result's row of actions, without a verdict. The saved definition must still match before
an action runs: when the declarations changed, opening the review, running a tool or
submitting ends the request ERROR (INPUT_CHANGED), and the next verify asks again.

The [Tool log](../features/tools/human.md#tool-log) shows every run of this review
as it happens, its start times in local time. The log wraps,
scrolls on its own and follows the newest line; scroll up to read and it stays,
scroll back to the bottom and it follows again. Its text is ordinary selectable
text, so copy what a tool printed into the result. It lasts while the app runs and
is cleared when you submit in this window; a submission made with the CLI or in
another window ends the editing here but leaves this window's log shown. Nothing is
saved.

After claiming, `g` or **GREEN** opens the GREEN form and `r` or **RED** the RED one, each
built from the eval's schema ([the shapes it may use](../features/declarations.md#human-result-schemas)).
`*` marks a required field, an empty optional one says **Optional**, and **Add** and the
remove mark add and remove list items and groups. Tab moves between controls, and
Ctrl+G/Ctrl+R or the verdict buttons switch verdicts. The Result ends in one row: **Release
claim** on the left and **Submit GREEN** or **Submit RED** on the right, with what running
work or a failure has to say between them. Ctrl+S or **Submit** submits; Enter in a field
does not. A schema error names the failing field and
leaves the form open to correct. Each verdict keeps its own draft until you submit, also
when you leave the form or open the request from elsewhere in the same window. Typing through an input
method, such as Korean, never fires shortcuts. If the state directory is replaced, for
example from a backup, open reviews close and their drafts and Tool logs are discarded.

A completed result stays open. Remote publication can fail **after local
settlement**: the form closes and a saved-locally notice explains the failure.
The verdict is already recorded; retry publication with Settings' **Push results**
(or `artifactize remote push`), not by submitting again.

### Leave and quit

Press `q` outside a text field, choose **Quit** in Commands, or close the window. `q`
does not quit while the keyboard is in a Human review that is editing or busy, so finish
or cancel that work first; it also does nothing while Settings is open.

If this window took Human claims it has not submitted or released, it names them and asks
first:

| Choice | Key | Claims |
|---|---|---|
| **Stay** | Esc | Nothing changes; the app stays open. |
| **Release and quit** | `u` | Released, so another reviewer can take them. |

Quitting cancels tools that are still producing output and review work in progress, and
discards unsubmitted drafts.
Programs a launch tool opened keep running, so close them yourself.

## Settings

Settings opens as a modal over the screen; Esc, its **Close** or the **Settings** button
again closes it, and the screen beneath stays as it was. It reloads account, key and store
information each time it opens; the palette's **Refresh settings** command reloads it too.
It has three tabs, **Providers**, **Remote** and **Limits**; while the tab list has focus, ← and →
cycle through them.

**Providers** lists the built-in providers on the left (Codex, Claude Agent, OpenAI, Anthropic,
Gemini),
then your custom providers by name, then **Custom provider**. Choosing one shows its setup
on the right, with **List models** for the models it offers.

- **Codex:** its account as a fact: not signed in, artifactize's stored token and expiry, a
  read-only `ARTIFACTIZE_CODEX_AUTH_FILE`, or why storage is refused. An expired stored access
  token refreshes on next use; an expired external file needs a new Codex sign-in.
  External-file and refused-storage modes disable sign-in here; sign-out is
  available only for artifactize's own stored account. **Sign in with Codex** signs in through
  the browser; while it is in progress the account reads so and **Cancel sign-in** stops it.
  When the browser cannot return, unfold **Finish by hand**: it shows the sign-in's address
  as text you can copy, a field for the redirect URL, and **Finish sign-in**. If another
  process holds port 1455, it says the browser cannot return here, so paste the full
  `http://localhost:1455/auth/callback?code=…` URL from the browser's address bar after the
  failed load. **Sign out of Codex** removes local tokens and warns when server revocation
  cannot be confirmed. While signed in, Commands offers **Sign in to Codex again** to sign in
  anew without signing out first.
- **Claude Agent:** shows Claude Code's sign-in as Claude Code itself reports it
  (`claude auth status`): `claude` not found on `PATH`, not signed in, signed in with its
  login method and plan (for example `claude.ai · max`), or the error it failed with.
  **Check again** asks again, for example after signing in from a terminal. **Sign in with
  Claude Code** runs `claude auth login`, as `artifactize login claude-agent` does: Claude
  Code opens the browser. Finish in the browser; if it did not open or cannot return, unfold
  **Finish by hand** for the URL Claude Code printed and a field for the code its page gives,
  then choose **Finish sign-in**. **Cancel sign-in** stops Claude Code. **Sign out of Claude Code** runs `claude auth logout`. The
  status shows again when either ends. Claude Code keeps its login; artifactize never reads,
  copies or stores it. **List models** shows the models Claude Code offers to it
  ([Claude Agent](../features/agent.md#claude-agent)).
- **OpenAI, Anthropic, Gemini:** shows whether the key is set from its environment
  variable (naming it), saved here, or not set, never its value. Paste a key and choose **Save key** to save
  it, as `artifactize login` does; **Remove saved key** forgets it. A set environment
  variable is used first: a key saved beside it shows as kept but not used, and can still be
  removed.
- **Custom providers:** **Custom provider** opens an empty form. Give it a name, then
  choose its kind: an alias of a built-in provider (used with that provider's sign-in or
  key) or an endpoint (its API, base URL and optional key), and choose **Save**. Add as many as
  you need; choosing one shows its form to change it or **Remove provider**. An endpoint's
  saved key is never shown: leave the key empty to keep it, or check **No key**. Evals use
  a custom provider by its name in `backend`; see
  [Custom providers](../features/agent.md#custom-providers).

**Remote** is the review store. Signed out, it shows the store URL, share level and token
fields and **Sign in to the store**. Signed in, it shows the store's URL and share level as
facts and its status in one line (signed in, the reviewer and how many results this machine pushed today, or what a push or check said),
with **Check status**, **Push (dry run)**, **Push results** and **Sign out of the store**; the
fields come back once you sign out. When a status check finds that the store refuses
the saved token or cannot be reached, the sign-in fields come back with the store's URL and what went wrong, and only
**Check status** besides **Sign in to the store**: there is no **Push results** until you
sign in again with a valid token, or restore the connection and choose **Check status**.
A failed push may leave the previous signed-in controls visible with an error. Choose
**Check status** or reopen Settings to refresh the connection-dependent controls.
**Summary** omits invocation details,
captured output and tool-call history; **Full** also sends the saved execution.
Summary still shares review metadata, owner results, declared Runtime commands
and arguments, and Agent execution-path metadata. Those declarations can contain
paths or sensitive literals: Summary is not secret redaction. Choose according
to the team's privacy needs. Signing out removes local configuration, **not the server
token**; a store administrator must [revoke it](../cli/review-store.md#review-store-server).
See [sharing and push behavior](../cli/review-store.md#remote-review-store-client).

**Limits** is the sign-off idle grace: how long a Run with only unclaimed sign-offs left
stays open before it ends YELLOW. Its seconds field holds the grace in force, 120 unless
changed; enter a new value (0–86400) and choose **Save** for the Runs started afterwards. It is the same machine-wide
[setting](../features/others/state.md#sign-off-idle-grace) as `artifactize limits`.

Settings uses the same accounts and store operations as the CLI; credentials do
not belong in `.artf`.
