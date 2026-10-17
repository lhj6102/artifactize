# TUI

The terminal Monitor and Review browse the same saved Runs and Human requests as
the other interfaces. Install them with the [CLI](../getting-started/install.md);
no separate terminal package or service is needed. Browsing is read-only until
you explicitly claim, run a Human tool or submit a verdict.

## Review

```sh
artifactize review [REQUEST_ID] [--repo PATH | --all] [--state-dir PATH] [--reviewer NAME]
```

A terminal UI that searches saved requests and reviews the waiting Human ones, with the
same lifecycle as `request`: it claims, runs tools, unclaims and submits in-process, and
publishes a submission to the remote review store exactly like `request submit`. It also
runs on its own, for example in a second terminal or tmux pane.

Without an ID it lists the requests its search finds across every repository in the state,
newest Run first, with their eval, request, status, claim, age and repository; Enter opens
one. The search starts as `kind:human status:waiting_human`, the waiting Human sign-offs.
`--repo PATH` adds that repository's `repo:` term to it; `--all` is accepted and is the
default. With an ID it opens that request in any repository; a waiting follower's ID opens
its original.

A search is free text and `key:value` terms; every term must match. A plain word matches,
case-insensitively, part of the repository, eval, request ID or claimant. A key restricts a
term to one field; quote a value with spaces (`repo:"/work/my repo"`), and quote a word to
search for it literally (`"type:style"`):

| Term | Matches |
|---|---|
| `repo:` | part of the repository path |
| `eval:` | part of the eval ID |
| `request:` | part of the request ID |
| `reviewer:` | part of the claimant's name; unclaimed requests never match |
| `tag:` | part of a tag of the eval's Artifact (`tags` in its declaration, as saved with the Run) |
| `kind:` | the eval kind: `runtime`, `agent` or `human` |
| `status:` | the request status, such as `waiting_human`, `green`, `red` or `error` |
| `is:mine` | claimed by you (`$USER` or `--reviewer`) |
| `is:claimed` | claimed by anyone |
| `is:unclaimed` | a waiting Human request no one has claimed |

Claims exist only on waiting Human requests, so `is:` matches nothing else. An unknown key
or value is an error that names the choices, and lists nothing.

`/` edits the search; the list follows as you type. Tab completes the last term: a key, or
after a key a value of the requests the rest of the search finds (repository folders, evals,
requests, claimants, tags), or a kind, status or `is:` value; the footer names the
candidates. Enter keeps the
search, Esc restores the one before editing, and Ctrl-U clears it. Empty search
includes every request kind, not just the initial waiting Humans. A page lists 100 requests;
PgDn shows older ones and PgUp newer.

The list holds one entry per waiting sign-off: followers that later Runs join to a waiting
Human execution are listed once, as the original request, which records the Human tools and
owner schemas and receives every action, as in the monitor. The search matches that entry, so
a follower's own repository does not find it; a follower whose original no longer waits is
listed as itself. A Human request opens the Human Detail below, read-only once settled. Any
other request opens a read-only Detail: the monitor's sections of its eval in its Run, its
saved runtime output, or its Agent session read as the monitor reads it (`d` shows the
sections); it has no claim, tools or forms.

The list and Detail use [Reactive panes](#reactive-panes). With the list focused,
it is Full and the selected request has a read-only Preview; Enter or → opens
Detail beside a Compact list. The linked layout reference gives the widths and
small-terminal behavior.
The breadcrumb is `artifactize review › search › eval`. ↑/↓ or `j`/`k` selects a request;
`r` refreshes the list. Tab opens the selected request with its Tools focused. Esc on the
list stays there, never quits. The standalone UI has no `?` help overlay or `!` attention
navigation; those keys belong to the monitor.

Mouse capture is on by default: click a list row to select it, then click it again
to open it; the wheel scrolls the list. **Human** Detail has the same mouse
interactions as Monitor. Standalone Review's non-Human Detail is keyboard-only:
its transcript groups and content do not respond to clicks or the wheel. F2 turns
capture off for terminal text selection; the header shows `mouse off · F2`.
Bracketed paste works either way.

Standalone Review's Runtime evidence is a fixed lower pane below the sections.
Its output is not independently scrollable: arrows and paging scroll the sections,
and Tab does not switch focus to evidence. Monitor instead has an independently
scrollable Runtime-evidence pane, beside or below its sections depending on width.

### Shared Human Detail

The monitor and standalone `review` use **one Human review component**. Opening
or previewing a request claims nothing. The instruction comes first, below one
line of status, claim and waiting time. `t` shows or hides Technical: request,
Run, repository, full claim details, shared execution, creation time and GREEN/RED
owner schemas. For a follower, the destination request and repository stay visible.

Before claim, the instruction fills the body and tools appear on one line. After
claim, it folds to two rows above Tools/Output and the fields. `i` expands or folds
it while not editing; the expanded instruction takes 70% of the body and ↑/↓
scrolls it. Ctrl-PgUp/PgDn scrolls long criteria from any sub-area; in monitor,
the wheel over the instruction scrolls it too. Tools focus widens Tools/Output
to 60% of the available width;
the selected tool shows its action and description. A command tool shows its
resolved command line, or its declaration and the reason resolution failed.
A built-in shows an action on a logical target instead: for example, opening
`notes.md`, printing a Markdown section, or listing a directory, not a shell
command line.

The way to the tools is always on screen: the footer lists the keys of the
focused area (for example `Tab tools, Enter run`, or `↑↓ tool · Enter run · Tab
fields` with Tools focused), the Tools pane title names its key (`Tools (1) · Tab`,
`Shift-Tab` while a form field takes Tab), and before a claim the one-line tools
summary says `Tab or click: tools`. A click on a tool row focuses the Tools pane
and selects it; a click on the selected tool, or on `[Run tool ⏎]`, runs it.

| Key | Human Detail action |
|---|---|
| `c` | Explicitly claim a waiting request; active only with Detail focused |
| `g` / `r` | Choose GREEN / RED and open its form, after claim |
| `u` | Release your claim without a verdict |
| Ctrl-G / Ctrl-R | Choose or switch verdict while editing |
| Ctrl-S | Submit the chosen form |
| PgUp / PgDn | Scroll the focused fields or completed-result viewport; Tab/arrows still select fields |
| Ctrl-U | Release while editing |
| Tab / Shift-Tab | Switch Tools and Fields; in edited Fields, Tab moves fields or indents JSON and Shift-Tab switches to Tools |
| `i` / `t` | Expand the instruction / toggle Technical, while not editing |
| Enter | With Tools focused, run the selected tool at once (after claim) |
| Esc | Cancel work, then stop editing, then leave Detail |
| Ctrl-E | Standalone only: edit the form in `$EDITOR` |

- **Claim.** Press `c` before running tools or submitting (`$USER` unless
  `--reviewer`). Those actions never claim implicitly. A request claimed by someone
  else, or no longer waiting, is read-only. GREEN/RED, submit and release require
  your claim. `u` or Release relinquishes it; leaving Detail keeps it.
- **Tools.** With Tools focused, ↑/↓ selects and Enter runs the selected tool
  at once; there is no confirmation step. `j`/`k` also selects while not editing.
  The run needs your claim on a request that still waits, checked when it starts:
  after a release or a settlement elsewhere it neither claims nor runs. Each run
  reloads and resolves the declarations again, but refuses the action if the
  recorded Artifact scope or eval declarations (including tools) have changed.
  It does not accept an updated command in place of the recorded review definition.
  An open form stays open while a tool runs. A `launch` tool reports
  "launched" and the UI continues. An `output` tool runs with a spinner; Esc or
  Ctrl-C cancels. Its stdout/stderr fill Output; PgUp/PgDn scroll. A nonzero exit
  is a tool error, not a verdict.
- **Form.** `g`/`r` opens the schema-backed GREEN/RED form directly; there is no
  verdict popup. A flat object with `const`, `boolean`, `string` (`minLength`,
  `maxLength`, `enum`), `integer` or `number` properties gets typed fields. `const`
  fields are prefilled and fixed; `*` marks required fields. Blank optional fields
  are omitted from submission. Optional booleans cycle through true, false and
  unset; required booleans have only true/false. Tab or ↑/↓ moves between fields,
  Space or ←/→ chooses, and typing edits. Other schemas (nested objects, arrays, composition,
  `$ref`) open a JSON template inside the TUI. Both inline and external JSON
  editors take **only owner fields**: omit `verdict`, which is inserted from the
  selected GREEN/RED choice automatically; an explicit `verdict` field is rejected.
  Enter inserts a JSON newline and Tab indents; Enter never submits a flat form. Ctrl-S or Submit submits. A
  validation error keeps the request waiting and shows the failing paths.
  A saved request **without a recorded Human definition** is not an empty-schema
  review: its tools display as unknown and GREEN/RED forms are refused. Browsing
  that old request does not invent missing tool or schema declarations.
- **Editing.** Ordinary letters (`q`, `c`, `i`, `t`, …), ←/→ and Tab belong to the
  fields, not navigation. A bracketed keyboard paste goes into the focused field as
  one edit, in standalone `review` as in the monitor. Ctrl-G/Ctrl-R switches
  verdicts, keeping separate drafts.
  Editing and tool work normally lock focus moves outside Detail. Monitor's Close
  button is an exception: when idle it returns directly to the tree with the form
  retained; while busy it cancels the work first. Esc closes
  one thing at a time: cancel a job, then stop editing with its
  draft kept, then leave Detail. The same verdict reopens the draft. Within the same state identity, refresh and
  leaving/reopening the same request keep drafts; opening a different request in
  standalone `review` starts fresh. A request settled by another command ends
  editing on refresh and releases the focus lock.
- **External editor.** Only standalone `review` offers Ctrl-E in a form. It opens
  `$VISUAL`, else `$EDITOR` (default `vi`, Notepad on Windows) on the owner-fields JSON.
  Ctrl-E converts a typed draft to inline JSON first; that JSON mode remains if
  external editing fails or does not submit. Any nonblank file containing valid
  owner-fields JSON within the 256000-byte Human fields limit attempts immediate
  submission, including `{}` when the schema accepts it. Oversized edited files
  are rejected before parsing or submission. Blank file content submits nothing. Monitor never opens `$EDITOR`;
  nested JSON stays in the TUI.

Outside a form or work, `q` quits from Detail too. In either UI, Ctrl-C
requests cancellation of any active Human lifecycle job: claim, tool, submission
(including publication) or release. It stays in the UI while that job is busy;
once no job is busy, Ctrl-C invokes quitting, including from a form.
Standalone `review` lists claims
this session took without submitting: `k` keeps them and quits, `u` releases them
and quits, and Esc stays. Ctrl-C again at this standalone quit prompt exits while
keeping outstanding claims. The quit prompt sets a form aside without dropping its
draft. Monitor keeps claims on exit; release with `u` first if you want to give
one back. Esc never quits either UI.

After a standalone submission the list returns if the search still lists requests;
otherwise `review` exits. Monitor keeps the completed result open in Detail. Both use the
same in-process lifecycle jobs; monitor does not launch the standalone command.

## Monitor

```sh
artifactize monitor [PATH | --repo PATH | --all] [--state-dir PATH]
```

The monitor catalogs every repository in this state. Plain `monitor` starts on **ALL**,
independent of cwd. `monitor .`, `monitor PATH` or `--repo PATH` selects that repository or
worktree initially; `--all` remains accepted. A positional path conflicts with `--repo`,
and `--all` conflicts with either. This sets only the initial selection: navigation still
reaches ALL and every repository/worktree.
Navigation drills down through **Scope → Runs → Run tree → Detail**.

### Reactive panes

The focused pane is **Full**. **Compact** gives narrow context from the level to
its left. **Preview** shows the next level without moving focus. Hidden levels
stay in the breadcrumb: `artifactize › scope › Run › eval`.

At every terminal width, the focused pane fills the width between its immediate
neighbours: **Compact + Full + Preview**. Both side panes have the same fixed
width, borders included:

| Terminal width | Each side pane |
|---|---|
| Fewer than 100 columns | 16 columns |
| 100–139 columns | 28 columns |
| 140 columns or more | 36 columns |

Full takes the remaining width after the side panes. Scope has no left neighbour,
so it shows Full + Preview; Detail has no right neighbour, so it shows Tree
Compact + Detail Full. No neighbour is hidden at narrower breakpoints. If the
terminal cannot fit even the sides, they are clipped equally rather than dropping
a level. Widen the terminal when the focused pane needs more room.

- **Scope.** Git clones group by their common Git directory. `ALL` selects the
  entire state; each repository row selects all its worktrees, without a separate
  repository ALL row. Worktrees without Runs fold into `+N without Runs (Space)`;
  Space shows or hides them. The selected scope stays visible. Non-Git and deleted
  paths stay listed. Runs record optional `commonDir`, `worktreePath` and `branch`
  alongside their `repoPath` workspace.
- **Runs.** The selected scope's Runs appear newest first, with age, duration and
  request counts (`✓` GREEN, `✗` RED, `!` ERROR, `?` waiting Human, `◐` running).
  Narrow panes drop secondary columns. Scope filtering happens before paging;
  moving past the end loads older Runs.
- **Run tree.** The selected Run's saved definitions, recorded evidence and current
  request states, not a re-evaluation of files. The headline shows status, wall
  time, counts and only the token total for usage. It shows `validation pending`
  while running, then **SATISFIED / NOT SATISFIED at Run end**. Up to three attention
  lines show errors, waiting Humans and running evals, then `+N more`. Enter on the
  **Run** row opens validation and unmet obligations, counts, errors, waiting and
  running work, timing, budgets, executed/reused counts and full spent/saved
  usage in Detail.

Scope badges and the header share `!N ✗N ◐N ?N` attention counters. These cover the
catalog, not just the visible Run page. Followers of one Human execution count as
one waiting sign-off.

### Artifacts and evals

Each eval has **one row**. Artifact rows summarize the eval rows shown now, with
`passed/total` and the most urgent state. `[file]` marks file Artifacts; their target
path and tags are in Detail's Technical section. A basis Artifact is one row:
`◇ engine  basis`. There are no separate relation rows or always-on dependency lists.

Artifact completion also enforces upstream gates, not just its eval rows' colors.
An eval preserves its own reusable GREEN while its Artifact can show **done, but
waits for X** or **done, but blocked by X**, with zero passed (for example `0/1`).
All-GREEN eval rows therefore do not prove that the Artifact is ready for consumers; its
required upstream evidence must also be satisfied.

A partial Run can use saved evidence for an eval that has **no request in this
Run**. Its row says `GREEN · saved result, not in this Run` (or the saved RED).
That evidence contributes to completion counts and gates: saved GREEN can satisfy
an input; saved RED can block a consumer. A dash means neither source is present.

| View state | Meaning |
|---|---|
| `✓` done | GREEN, executed or reused |
| `✗` / `!` failed | RED / ERROR |
| `◐` / `?` / `·` in progress | Running, waiting for a Human (with claimant), awaiting admission, waiting for a backend slot or an explicit `--jobs` cap, or joining an execution |
| `…` waits for X | A running Run waits for X's required evals to be GREEN |
| `⊘` blocked by X | An upstream RED or blocked eval prevents execution |
| `○` / `$` / `-` not run | A finished Run waited for X, has stale evidence or did not review the eval (`○`); its budget ran out (`$`); the eval has neither a request nor saved evidence (`-`) |

Running and Human rows are bold and colored; queued rows keep a colored `·`.
Waiting and ordinary not-run rows are dim. ERROR
upstream asks for a retry rather than blocking as RED does. A finished Run can
still have an open Human request. These view states explain saved request states;
they do not rename the states in JSON.

The Run's [execution modes](../features/run.md#speculation-and-deferred-sign-offs)
also explain its display. With `--speculate`, a downstream eval can be READY or
running while upstream judgment is still pending; a later upstream RED/ERROR
blocks or cancels that speculative work. With `--defer-human`, sign-offs appear
as **DEFERRED** and dependent work can be **PROVISIONAL**, not approved. A finished
**SIGNOFFS_PENDING** Run means only those sign-offs remain. Their later submissions
change current requests, not the historical Run-end verdict; strict Human gates
still remain strict.

Each Eval references its own input Artifacts and waits for their required Evals.
A row groups those gates into at most two
unmet Artifacts, then `+N`, with each Artifact's completion state. When all targets
are themselves waiting, it adds one level of cause, for example
`waits for cli … waiting (cli waits for code-style)`. Cycle peers share `↻ peer`
markers. Actual Eval-cycle peers do not gate one another; an Artifact-level cycle
marker does not make every sibling Eval a cycle peer. External gates still apply.
Upstream comes first where the graph allows; Artifact cycle peers and Evals are
ordered by name.

All-done Artifacts fold automatically. Other Artifacts stay open unless you toggle
them; refresh respects your choices. A new Run selects its first in-progress or
failed eval. `h` folds, `l` unfolds and Space toggles the selected tree row.

The selected eval's upstream Artifact rows get `↑`: bright when unmet, dim when
complete. There is no downstream highlight. Off-screen targets show `↑N above` or
`↑N below`. `b` cycles through those Artifact rows, unmet first; Backspace returns
to the original row. On an Artifact row, `b` uses its evals' combined upstream.

The headline's validation stays **at Run end**. The tree rolls up the requests as
they stand now, including a later Human submission. A dim `*` marks a row whose
derived state changed since the Run ended; it does not mean files changed. Artifact
Detail also keeps an `At Run end` field. Historical requests and validation are
not rewritten by this view.

### Keys and mouse

| Key | Action |
|---|---|
| Enter / → | Next level; on a tree row, open Detail |
| `o` | Open the selected tree row's Detail |
| ← / Esc | Step back; Esc never quits |
| Tab / Shift-Tab | Cycle Scope, Runs and the tree; inside Detail, switch sub-areas |
| ↑/↓ or `j`/`k` | Select a row; inside Detail, scroll |
| `?` | Show help; an ordinary next key dismisses it without acting. Ctrl-C still quits and F2 still toggles mouse capture |
| `!` | Next ERROR, RED or waiting Human eval in this Run, wrapping around |
| `r` | Refresh with Scope, Runs or the tree focused; in an owned idle Human Detail it chooses RED, and it does not refresh read-only Detail |
| `q` / Ctrl-C | `q` quits outside editing/work; Ctrl-C requests cancellation of any busy Human lifecycle job; otherwise it invokes quitting |
| F2 | Toggle mouse capture for terminal text selection |

From Detail, `!` opens the next attention item's Detail. Human actions require
Detail focus; [Shared Human Detail](#shared-human-detail) owns its focus locks,
Esc and Close behavior and retained drafts.

Click focuses and selects. Double-click opens a tree row's Detail; clicking the
peek opens it Full. In a Human Detail, a click on the selected tool runs it. The wheel scrolls the pane under the pointer without moving
focus. In the tree it moves the viewport, not the selection, so the peek stays on
the same eval. Breadcrumb segments are not clickable. Bracketed keyboard paste
works with mouse capture on or off.

### Detail

With the tree focused, Detail is a **peek** that follows `j`/`k`: Outcome and the
Artifacts the eval waits for. A running Agent's peek also shows its last transcript
line. Enter, → or a click on the peek opens **Full** Detail instead of a modal.
Leaving Detail closes the full view and restores the peek.

Read-only sections appear in this order: **Outcome → Waits for → What → Provenance
→ Technical**. Outcome puts the verdict or error first. What includes the instruction
and raw result. Provenance holds work, usage, budgets and timing. Technical holds
identifiers and hashes; it starts folded (`t` toggles it). Waits for lists the same
upstream Artifacts as the tree, with their completion state and reference origin;
`w` shows or hides their pending evals. Empty sections are omitted. Tab switches
between sections and evidence; ↑/↓ and PgUp/PgDn scroll the focused area.
Home jumps to the top of Monitor's focused read-only sections or Runtime evidence,
and to the top of standalone Review's read-only sections.

Detail depends on the eval kind:

- **Agent:** a live conversation, including a running review or the original local
  conversation behind reused evidence. ↑/PgUp pauses following so you can read;
  Home jumps to the start and End returns to the latest messages. Monitor also
  offers Top, Bottom and Details buttons; Bottom resumes following. Space or Enter toggles the visible tool
  group in either UI, showing its names, targets and recorded states. Monitor
  also toggles a group when you click its header. `d` opens the saved result and
  other details and returns to the transcript in either UI. Esc returns from
  saved details to the transcript in Monitor, but leaves Detail for the list in
  standalone Review. Only public reasoning summaries appear,
  when the provider supplies them; private reasoning stays hidden. Missing or
  remote conversations explain why they cannot be shown here. Use `session show
  --json` for the complete saved record. See the
  [transcript reader reference](../features/others/state.md#transcript-reader)
  for storage, streaming and history behavior.
- **Runtime:** saved stdout/stderr, exit code and capture truncation beside the
  sections when Detail has at least 90 inner-content columns (92 including its
  outer borders); in a narrower pane the evidence stacks below. Running,
  timeout/cancellation and remote summary-only results show logs as unavailable;
  there is no live-log recorder.
- **Human:** use the [Shared Human Detail](#shared-human-detail) controls for claims,
  tools, verdict forms and stepping back. Monitor-specific differences: it keeps
  the completed result open and never opens `$EDITOR`. Outside editing or work,
  `q` quits, `?` shows monitor help and `!` opens the next attention item.

Browsing is read-only. If publishing to the team store fails after submission,
Detail shows the completed local result
and the publication failure rather than asking you to submit again. Terminal
settings are restored on exit. See the
[local reader reference](../features/others/state.md#local-readers) for
subscription and recovery behavior.

A refresh failure records a timestamped **read failed at …** notice. Monitor and
the Review list retain last-known data, so visible requests, results and claims
can be stale, not current authority. Standalone non-Human Detail can hide the
notice until you return to the list, and a failed Record reload can clear that
previous Detail instead of retaining it. In Review, return to the request list
and press `r`; in Monitor, request refresh from Scope, Runs or the tree.
A successful read clears the failure notice.

Draft retention applies only within the same state identity. If Monitor detects
that a replacement state database has a different identity, it cancels active
Human work, closes Detail and clears cached reviews and in-memory drafts before
showing the replacement state. Do not expect the old state's draft to carry over.
