# Human reviews

A Human eval asks a person for a GREEN or RED sign-off. The reviewer claims the
waiting request, runs the Human tools you declared and submits a verdict, either
with `artifactize request` commands or in the `artifactize review` terminal UI.

READY Human evals persist WAITING_HUMAN and release their job slot. They consume
no `maxExecutions` budget, so even a zero budget admits a Human review. `verify`
waits for Human results as it waits for runtime and Agent evals, and never
fabricates a verdict. When its wait times out it exits and lists the waiting
requests, and no worker stays alive. Waiting executions with a reuse key retain their exclusive
claim on the key after the verifier exits. Cross-repository followers refer to that
same execution and forward Human actions to its original request and repository.

A submission that arrives while `verify` waits runs the Human eval's dependents in
the same Run. Human sign-offs reuse by default: artifactsum supplies the target and
dependency fingerprints. A later `verify` reuses the submitted result and runs its
dependents. **`fingerprint = false` means no reuse** when declared by the target or any
dependency: submission settles only that Run, and a later `verify` asks for a new
Human review. Those dependents continue only in the Run that waits for it.
Dependency evals wait with WAIT_DEPENDENCY while required Human evidence waits, then
derive GREEN or BLOCKED from the submission. Later submissions never rewrite a
historical derived request; a new `status` or `verify` derives current evidence.

```sh
artifactize verify --all --timeout-ms 600000
# In another terminal, using the same state directory:
artifactize request list [--run RUN_ID] [--json]
artifactize request show REQUEST_ID
artifactize request claim REQUEST_ID [--reviewer NAME]
artifactize request unclaim REQUEST_ID [--reviewer NAME]   # release without a verdict
artifactize request tool REQUEST_ID inspect_child [--reviewer NAME]
artifactize request submit REQUEST_ID --verdict GREEN --fields '{"approved":true}'
# Alternatively: --fields-file /path/to/fields.json
# Or claim, run tools and submit in a terminal UI (see Review):
artifactize review [REQUEST_ID]
```

Claim, unclaim, tool and submit default the reviewer to `$USER`; `--reviewer NAME`
can select the same explicit reviewer for each action. Reviewer names are local
cooperative locks, not authenticated accounts. Only the claimant can run tools,
submit or unclaim. `request claim` prints the claim and `request unclaim` the
released claim; unclaiming a request someone else holds, or one that no longer
waits, exits 2. Tool names are `<operation>_<artifactId>` and take no free arguments.
Text output prints captured text or a launch notice; `--json` prints the tool
result. A tool failure exits 2 and does not invent a verdict.

Submission requires `--verdict GREEN|RED`. `--fields` and `--fields-file` are
mutually exclusive JSON objects of owner fields (default `{}`), not verdict
wrappers. Files must be regular files; fields and the complete result are bounded
at 256000 bytes. Invalid JSON, schemas, reviewer names or verdicts exit 2 and
leave the request correctable. A successful submission exits 0, even for RED;
it does not start a separate verifier.

While Human requests wait, local state-change notifications wake `verify`, which
reconciles saved states and resumes newly READY dependents in the **same Run**,
retaining its execution budget and earlier results. While waiting the Run remains
RUNNING. `--timeout-ms` defaults to 600000, and accepts 1–2147483647. The deadline
begins when scheduling starts and is checked when foreground execution is idle with
pending Human work; it never interrupts running evals or cancels Human requests. On
timeout the Run ends INCOMPLETE with `waitTimedOut: true` and exit 3. Claims and submissions
remain available, but no background worker continues dependents. With reuse
disabled, use a new verify and submit its new request to complete those dependents.
Ctrl-C/SIGTERM exits 2, cleans owned processes, and ends the Run as cancelled;
previously created Human requests remain available. Missing non-Human obligations
without any pending Human request return INCOMPLETE (4) immediately. `verify --reuse-only human`
never records a Human request or waits: a Human eval reuses a sign-off or is
[not executed](../reference/cli.md#reuse-only), as CI wants.

The [reference](../reference/human-tools.md#human-reviews) has the library API, the
`request list` and `request show` fields and Run summaries.

## Human tools

`views.human_tools` is a separate safe-name map of predefined commands. Each entry requires
exactly `description`, `kind = "launch"` or `kind = "output"`, `command` and `args`, with
optional `timeout_ms` (1–2147483647). No `input_schema`, free call arguments, `protocol`,
`execution_paths`, `metadata` or `script` wrappers are accepted. Descriptions follow
the Agent description rules, including `{artifactName}`.

```toml
[views.human_tools]
open = { description = "Open {artifactName} for review.", kind = "launch", command = "code", args = ["{artifactPath}"] }
show = { description = "Show the review notes for {artifactName}.", kind = "output", command = "cat", args = ["{artifactPath}/notes.txt"], timeout_ms = 120000 }
```

Placeholders, executable resolution and the `launch` and `output` kinds are in the
[reference](../reference/human-tools.md#human-tools).

## Review

```sh
artifactize review [REQUEST_ID] [--repo PATH | --all] [--state-dir PATH] [--reviewer NAME]
```

A terminal UI for waiting Human reviews with the same lifecycle as `request`: it
claims, runs tools, unclaims and submits in-process, and
publishes a submission to the remote review store exactly like `request submit`.
Without an ID it lists the WAITING_HUMAN requests of the canonical `--repo`
(default: the current directory) or, with `--all`, of every repository (newest
Run first: eval, request, claim, waiting time, repository); Enter opens one. With
an ID it opens that request in any repository. It also runs on its own, for example
in a second terminal or tmux pane.

The request screen shows the request, Run, repository, status, claim, instruction,
GREEN and RED owner schemas and the declared Human tools with their commands and
args. Opening a request claims nothing.

- **Claim on first action.** Running a tool or submitting claims the request for
  the reviewer (`$USER` unless `--reviewer`) if it is unclaimed. A request claimed
  by someone else, or no longer waiting, is read-only. `u` releases your claim. On
  quit, claims this session took without submitting are listed: `k` keeps them,
  `u` releases them.
- **Tools.** `j`/`k` select a tool and Enter runs it. Before the first run of a
  command line in a session, a confirmation shows the resolved command, its
  directory and the repository it comes from (with `--all` it may be another
  repository); `y` runs it. A `launch` tool reports "launched" and the UI
  continues. An `output` tool runs with a spinner (Esc cancels) and its stdout and
  stderr fill the output pane (PgUp/PgDn scroll). A nonzero exit is shown as a tool
  error, not a verdict.
- **Submit.** `s`, then `g` (GREEN) or `r` (RED). A form opens when the verdict's
  owner schema is a flat object whose properties are `const`, `boolean`, `string`
  (with `minLength`, `maxLength` or `enum`), `integer` or `number`; `const` fields
  are prefilled and fixed. Tab or ↑/↓ move, Space or ←/→ choose, typing edits,
  Enter submits. Any other schema (nested objects, arrays, composition, `$ref`),
  or Ctrl-E in the form, opens `$EDITOR` (default `vi`) on a JSON template of the
  owner fields; saving submits it, and an empty file submits nothing. A validation
  error keeps the request waiting and returns to the form with the failing paths.
- After a submission the list returns if more requests wait; otherwise `review`
  exits. Esc goes back and `q` quits. The monitor embeds the same review controller
  and lifecycle jobs rather than launching this standalone command.

## Monitor

```sh
artifactize monitor [--repo PATH | --all] [--state-dir PATH]
```

The monitor catalogs every repository in this state. The current directory
(including a Git subdirectory), `--repo`, or `--all` only sets the first selection.
Navigation drills down through **Scope → Runs → Run tree → Detail**.

### Reactive panes

The focused pane is **Full**. **Compact** gives narrow context from the level to
its left. **Preview** shows the next level without moving focus. Hidden levels
stay in the breadcrumb: `artifactize › scope › Run › eval`.

| Terminal width | Visible panes |
|---|---|
| 140 columns or more | Up to Compact + Full + Preview |
| 100–139 columns | Full + Preview, not Compact + Full |
| Fewer than 100 columns | Full only, with the breadcrumb |

With Detail focused, the layout shows Tree Compact + Detail Full at 100 columns
or more, and Detail alone below 100. Compact widths are 20 for Scope, 18 for Runs
and 30 for the tree. Full grows to its natural width; spare width goes to Preview.

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
  request states, not a re-evaluation of files. The headline shows status, elapsed
  time, counts and only the token total for usage. It shows `validation pending`
  while running, then **SATISFIED / NOT SATISFIED at Run end**. Up to three attention
  lines show errors, waiting Humans and running evals, then `+N more`. Enter on the
  **Run** row opens validation and unmet obligations, counts, errors, waiting and
  running work, timing, budgets, executed/reused/derived counts and full spent/saved
  usage in Detail.

Scope badges and the header share `!N ✗N ◐N ?N` attention counters. These cover the
catalog, not just the visible Run page. Followers of one Human execution count as
one waiting sign-off.

### Artifacts and evals

Each eval has **one row**. Artifact rows summarize the eval rows shown now, with
`passed/total` and the most urgent state. `[file]` marks file Artifacts; their target
path and tags are in Detail's Technical section. A basis Artifact is one row:
`◇ engine  basis`. There are no separate relation rows or always-on dependency lists.

| View state | Meaning |
|---|---|
| `✓` done | GREEN, executed, reused or derived |
| `✗` / `!` failed | RED / ERROR |
| `◐` / `?` / `·` in progress | Running, waiting for a Human (with claimant), queued for a job/backend slot, or joining an execution |
| `…` waits for X | A running Run waits for X's required evals to be GREEN |
| `⊘` blocked by X | An upstream RED or blocked eval prevents execution |
| `○` / `$` / `-` not run | A finished Run waited for X, has stale evidence or did not review the eval (`○`); its budget ran out (`$`); the eval has no request (`-`) |

Running and Human rows are bold and colored; queued rows keep a colored `·`.
Waiting and ordinary not-run rows are dim. ERROR
upstream asks for a retry rather than blocking as RED does. A finished Run can
still have an open Human request. These view states explain saved request states;
they do not rename the states in JSON.

Dependencies target **Artifacts**, not individual evals. A row shows at most two
unmet Artifacts, then `+N`, with each Artifact's completion state. When all targets
are themselves waiting, it adds one level of cause, for example
`waits for cli … waiting (cli waits for code-style)`. Cycle peers share `↻ peer`
markers and do not wait for each other, unless a dependency eval's `depends_on`
explicitly names a peer. Upstream comes first where the graph allows; cycle peers
are ordered by name. Dependency evals carry `[dep]` and follow ordinary evals.

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
| `?` | Show help; the next key closes it without another action |
| `!` | Next ERROR, RED or waiting Human eval in this Run, wrapping around |
| `r` | Refresh outside a Human form |
| `q` / Ctrl-C | Quit outside editing; Ctrl-C first cancels a running Human tool |
| F2 | Toggle mouse capture for terminal text selection |

From Detail, `!` opens the next attention item's Detail. Human actions work only
with Detail focused; ordinary letters are field input while editing. Review work,
confirmation and editing lock focus moves to other panes. Esc cancels a tool or
confirmation first, otherwise leaves Detail with the draft kept.

Click focuses and selects. Double-click opens a tree row's Detail; clicking the
peek opens it Full. The wheel scrolls the pane under the pointer without moving
focus. In the tree it moves the viewport, not the selection, so the peek stays on
the same eval. Breadcrumb segments are not clickable. Bracketed keyboard paste
works with mouse capture on or off.

### Detail

With the tree focused, Detail is a **peek** that follows `j`/`k`: Outcome and the
Artifacts the eval waits for. A running Agent's peek also shows its last transcript
line. Enter, → or a click on the peek opens **Full** Detail instead of a modal.
Leaving Detail closes the full view and restores the peek.

Sections appear in this order: **Outcome → Waits for → What → Provenance →
Technical**. Outcome puts the verdict or error first. What includes the instruction
and raw result. Provenance holds work, usage, budgets and timing. Technical holds
identifiers and hashes; it starts folded (`t` toggles it). Waits for lists the same
upstream Artifacts as the tree, with their completion state and reference origin;
`w` shows or hides their pending evals. Empty sections are omitted. Tab switches
between sections and evidence; ↑/↓ and PgUp/PgDn scroll the focused area.

Detail depends on the eval kind:

- **Agent:** a live transcript in Detail, including RUNNING reviews and the original
  local session behind reused evidence. Its title keeps one line of outcome:
  state, elapsed time, profile and dependencies. Summary/result sections are opt-in
  with `d` or Details; `d`/Esc returns to the transcript. Provider-normalized text and public
  reasoning summaries update the active block before the turn finishes, coalesced
  about every 100 ms. Public summaries appear under a subdued Thinking label only
  when the provider/model supplies them; their absence is not simulated. Encrypted,
  redacted and raw reasoning payloads never enter this view. OpenAI/Codex public
  summary SSE frames are observed without changing bytes delivered to rig, since
  rig's generic reasoning delta does not distinguish public summaries from raw
  reasoning. Typed completed summaries are the safe fallback. Display-only delivery
  events are not fed back into model history; final content replaces provisional
  content instead of duplicating it, and interrupted partial output is labeled.
  The initial view follows the bottom. ↑/PgUp or the wheel pauses at the
  reading position; ↓/PgDn back to the bottom resumes following. Home/End and the
  Top/Bottom buttons jump through the full history. The bottom border shows the row
  position, bottom/history and following/paused state. Resize preserves the logical
  reading anchor while paused and stays at the bottom while following. F2 disables
  mouse capture without changing keyboard scrolling or paste. The transcript shows
  actual message text with readable Markdown headings/code indentation, not serialized
  message envelopes. Consecutive tools share a collapsed activity line; click it or
  press Enter/Space for the first visible group to expand only names, targets and
  recorded states. Prose separates groups. Running targets and short failure reasons
  remain visible; explicitly expanded groups stay open across updates and resize.
  Successful tool output, detailed arguments, private reasoning and media payloads stay out
  of this view. Final verdicts and meaningful errors appear as text. `session show
  --json` remains the explicit lossless view of the saved record.
  Missing recorded local sessions explain session GC; never-saved and remote
  conversations are distinguished, and remote references never open arbitrary local
  paths. Replacement, truncation and deletion reset the selected document. Complete
  malformed events show an error; partial newline/UTF-8 tails wait for the next append.
  History is indexed incrementally with private ephemeral formatted-text/row files
  and visible-window page-in, not a permanent first-2-MiB cutoff. One complete typed
  event temporarily needs memory proportional to that event when decoded once;
  large history/layout jobs otherwise yield between bounded chunks. Temporary files
  are removed when the selected reader closes or resets. Later `session send` events
  remain visible after the review's end event.
- **Runtime:** saved stdout/stderr, exit code and capture truncation beside the
  sections, or below them in a Detail narrower than 90 columns. Running,
  timeout/cancellation and remote summary-only results show logs as unavailable;
  there is no live-log recorder.
- **Dependency:** the saved derived state and `blockedBy` requirements, marked
  `derived (no execution)`. It has no claim, tools or verdict form.
- **Human:** completed result, or an explicit **CLAIM → REVIEW** flow. Claim (`c`)
  enables tools on the left and GREEN/RED fields on the right. Request instructions
  stay visible above them in CLAIM and REVIEW; the wheel there or Ctrl-PgUp/PgDn
  scrolls long criteria without scrolling the form. GREEN/RED (`g`/`r`
  before editing, Ctrl-G/Ctrl-R while editing) selects the existing schema form.
  Flat schemas retain typed fields; nested/array schemas use a JSON template edited
  inside the TUI, never `$EDITOR`. Enter inserts a JSON newline; Ctrl-S or Submit
  submits. Shift-Tab switches between tools and fields; Tab moves flat fields or
  indents JSON. Run tool/Enter in the tool pane preserves first-command
  confirmation and cancellation. Release (`u`, or Ctrl-U while editing) unclaims.
  Refresh, leaving/reopening Detail and verdict switches retain drafts. Ordinary
  letters in fields are input, not global shortcuts.

Browsing uses read-only state queries and cached Git discovery, not configuration
rediscovery or fingerprints on each refresh. Local monitor/review readers share a
temporary hub hosted inside the first subscriber process (no permanent daemon),
using an owner-only Unix socket or Windows named pipe. Registration is acknowledged
before the initial snapshot. Readers reconnect and resync if the hub exits or
bounded hint queues overflow; writers never elect a hub. Unsafe or unavailable IPC
falls back to five-second authoritative reconciliation, without an insecure
transport fallback. A persistent read-only SQLite `data_version` connection and
file-identity probe detect missed commits and database replacement. Clocks and
busy spinners redraw cached data instead of reloading the database on each tick.
`verify` separately reconciles dead execution owners/backend slots and optional
remote results, retaining its existing absolute Human deadline. Session invalidation
hints wake only the selected session reader, without a full database query; a
five-second file probe also detects missed appends and removal. Rendering performs
no file I/O, and terminal input takes priority over append/index bursts. This does
not add remote team-store push events. Only explicit Human actions write or
run owner tools, through the same atomic claim, fingerprint recheck, schema
validation and `submit_and_publish` APIs as `review`. Followers resolve to their
original request. If remote publishing fails after local settlement, Detail
shows the completed local result and the publication failure instead of offering
another submission. Leaving Human Detail keeps its claim; Release relinquishes
it. Terminal modes, cursor, mouse capture and paste are restored on exit.
