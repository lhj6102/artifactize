# Desktop app: user journeys

What a person can do in the desktop app, from the app's real behaviour. Each journey has an id
(`J-...`) and ends in something visible. It is the artifactize app of this
machine, not of one repository: its Scope pane starts from ALL and narrows to a repository or
a worktree, and its Review search reaches every repository. It shows and changes the same state as the
`artifactize` CLI; a change made by either shows in the other.

Hovering only highlights what is under the pointer: no popup, summary, tooltip or menu opens on
hover, and information opens only on a click. A pane is never wider than its place: its width
is set by the layout or by a drag, never by what it shows, and what it shows wraps or scrolls in
its own box. The Detail's tabs show as rounded chips, the chosen one filled; a choice within a
tab, such as its Summary or its log, shows as plain text, the chosen one dark and bold and the
other light.

## Window

- **J-LAUNCH**: the app opens on the Monitor, or on Review when started with `review`, The window is titled "Artifactize".
- **J-SWITCH-SCREEN**: Ctrl+1 shows the Monitor, Ctrl+2 Review, Ctrl+, Settings. While Human reviews wait, the Review button shows their number after its name, as many as Review's starting search lists, and follows each change of the state; with none waiting it shows its name alone.
- **J-PALETTE**: Ctrl+K or the palette button opens the command palette; typing filters commands, Enter runs one, Esc closes it. It opens again after any use.
- **J-HELP**: `?` lists the keys of the current screen.
- **J-REFRESH**: Ctrl+R or F5 reloads the state from disk.
- **J-QUIT**: `q` outside a field, or closing the window, quits. With Human review claims held by this window it asks first: Esc stays, `u` releases the claims and quits.

## Monitor

- **J-MON-CATALOG**: the left side stacks the Scope and Runs panes and the right side is the graph area, with the divider between them always shown; with no Run selected, while loading, on an error or with an empty graph the graph area stays and says so. The Scope pane lists ALL (this machine) and each repository and worktree; selecting one narrows the Runs pane to its Runs; the selected Run fills the graph area as the graph of its Artifacts with their verdicts, under the Run's headline and what needs attention. What went wrong, such as Runs that cannot be read, shows as one line in the Run pane's header without moving or hiding the graph; a click on it shows it in full.
- **J-MON-NAVIGATE**: arrow keys or `j`/`k` select a scope or a Run, Enter or → goes one level deeper (Scope, Runs, the Run's graph), Esc goes back a level, and so does ← in the Runs pane.
- **J-MON-DETAIL**: a click on an Artifact opens its Detail, floating over the graph docked toward its right edge with a gap, never beside Scope and Runs, the graph staying visible behind it. The Detail's tabs read as a path, `[Artifact icon] {name} / [eval] [eval] …`: the Artifact's own chip, the Artifact icon before its name, read aloud as "Artifact {name}"; only the name is cut short when long (the Detail's title shows it whole), then a `/`, then one chip per eval showing its verdict's glyph and colour; an Artifact without evals has its own chip only, without the `/`. Eval chips that do not fit on one line wrap onto the next, within the Detail; the `/` stays with the Artifact's chip. Close is the Detail's one control of its own. A click on another Artifact shows that Artifact's Detail; a click on the graph's background, Close or Esc closes it. A click on the Run's headline opens the Run's Detail. Everything in the Detail, transcript and log lines, long words and code included, stays within its width.
- **J-MON-NEIGHBOURS**: the Artifact's chip is shown first. It draws the Artifact between its neighbours with the Run graph's nodes and edges, the Artifacts its evals use on its left and those whose evals use it on its right; the drawing does not pan, zoom or drag, and a click on a neighbour opens that neighbour's Detail. Below it, the Artifact's evals with their verdicts (a click on one shows its tab) and its path; every other fact, what it waits for included, folds under Debug.
- **J-MON-SUMMARY**: an eval's tab shows its Summary: its status, beside it in grey what running it took and spent (time, tokens in `k` or `M`, model) or what reuse saved; its instruction; its result, each owner field by name and a list as numbered items; then Execution options, Execution metadata and Fingerprint when present; every other fact, what it waits for included, folds under Debug. An eval with a log switches below the tabs between its Summary, shown first, and its log, a plain text choice a level below the tabs: an Agent eval's Agent log (its transcript, with its own scrollbar) or a runtime eval's Runtime log (its saved output). A Human eval's tab holds its review and sign-off.
- **J-MON-LIVE**: a change made by another process (a claim, a submission, a new Run) appears in the open Monitor without a reload.
- **J-MON-RESIZE**: the dividers between Scope and Runs and between the left side and the graph can be dragged to resize them; the split stays as dragged whether or not a graph is shown. Dragging the Detail's left edge sets its width, between a readable column and leaving some graph visible, never over Scope and Runs; the Detail keeps that width when opened again and in the next launch. Hovering a divider or the edge shows only the resize cursor.

## Run graph

The Monitor shows a Run only as its graph. The graph takes the pointer: Tab reaches its Artifacts and Enter on one does what a click does; it has no other keys of its own.

- **J-GRAPH-CURRENT**: `artifactize-desktop graph --repo PATH` (also the declared Human `graph` tool) opens current declarations in the same graph view, without a saved Run; a click on a node shows its evals docked over the graph's right side, the structure text exposes all criteria and labeled relations, Refresh or Ctrl+R/F5 reloads declarations. There are no Run verdicts on this screen.
- **J-GRAPH-COLOURS**: nodes and edges are coloured by verdict (GREEN, running or waiting for a Human, waiting, RED, ERROR, blocked) and recolour in place when a result arrives.
- **J-GRAPH-LARGE**: a Run too large to draw at once stays navigable: panning or zooming the camera draws the Artifacts it brings into view.

## Human review

- **J-HUMAN-CLAIM**: `c` claims a waiting Human review for this reviewer; `u` releases it. A claim made elsewhere shows as taken.
- **J-HUMAN-LAYOUT**: a Human review reads top to bottom as its instruction, the Tools, their Tool log and the Result.
- **J-HUMAN-TOOL**: the Tools are grouped by the Artifact that declares them, the reviewed one first, each a `▶` button; a click on it runs it at once. A launch tool opens its application and returns at once; a failing tool shows its error.
- **J-HUMAN-LOG**: every run of the review shows in the Tool log as it happens: `▶ HH:MM:SS · operation · Artifact`, then each line the tool prints, stderr in red, then its end line (`■ exit 0`, `■ launched`, `■ done`, `■ error: …`). A launched application's lines keep arriving under its own run until it exits. The log is monospace, wraps, scrolls on its own and follows its newest line unless the reader scrolled up; its text can be selected and copied into the result. A submission clears it.
- **J-HUMAN-GREEN**: after a claim `g` opens the GREEN form built from the eval's pass schema; Ctrl+S submits. The completed result stays open.
- **J-HUMAN-RED**: `r` opens the RED form; a result that fails its schema is not submitted, the failing path is shown and the form stays correctable. Ctrl+G and Ctrl+R switch the verdict while editing.
- **J-HUMAN-FORM**: the form shows text, number, true/false and choice fields, a list of text with `+ Add` and `✕` per item, and repeated groups of fields with `+ Add` for a group and `✕` per group, one group always left to type in. Constants are filled in and not shown; `*` marks a required field and an empty optional one says Optional in grey. There is no JSON editor.
- **J-HUMAN-TEXT**: text fields take any language, including Korean composed by an input method; no key typed during a composition triggers a shortcut.
- **J-HUMAN-ESC**: Esc cancels running work, then leaves the form, then leaves the Detail.

## Review

- **J-REVIEW-LIST**: the Review screen lists saved requests, starting on the waiting Human reviews of every repository on this machine; the selected request previews read-only. A search narrows it to a repository, an eval or a reviewer.
- **J-REVIEW-OPEN**: Enter opens a request (a Human review is actionable, any other request read-only); the context menu or the palette opens a Human review with Tools focused; Ctrl+O opens a request by ID.
- **J-REVIEW-SEARCH**: `/` or Ctrl+F searches: free text and `key:value` terms (repo, eval, request, reviewer, tag, kind, status, is), all of which must match; suggestions complete keys and values; Esc closes suggestions, then clears, then returns to the list.
- **J-REVIEW-PAGE**: PgDn and PgUp page through older and newer requests.

## Settings

- **J-SET-KEYS**: shows whether OPENAI_API_KEY and ANTHROPIC_API_KEY are set in the environment, never their values.
- **J-SET-CODEX**: signs in to or out of the Codex (ChatGPT) account through the browser; when the browser cannot return, the redirect URL can be pasted; a sign-in in progress can be cancelled.
- **J-SET-MODELS**: lists the models a chosen backend offers.
- **J-SET-REMOTE**: signs in to a shared review store with its URL, share level and token, shows its status, pushes results (or a dry run), and signs out.
