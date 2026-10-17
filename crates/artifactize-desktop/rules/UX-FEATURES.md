# Desktop app: user journeys

What a person can do in the desktop app, from the app's real behaviour. Each journey has an id
(`J-...`) and ends in something visible. It is the artifactize app of this
machine, not of one repository: its Scope pane starts from ALL and narrows to a repository or
a worktree, and its Review search reaches every repository. It shows and changes the same state as the
`artifactize` CLI; a change made by either shows in the other.

## Window

- **J-LAUNCH**: the app opens on the Monitor, or on Review when started with `review`, The window is titled "Artifactize".
- **J-SWITCH-SCREEN**: Ctrl+1 shows the Monitor, Ctrl+2 Review, Ctrl+, Settings.
- **J-PALETTE**: Ctrl+K or the palette button opens the command palette; typing filters commands, Enter runs one, Esc closes it. It opens again after any use.
- **J-HELP**: `?` lists the keys of the current screen.
- **J-REFRESH**: Ctrl+R or F5 reloads the state from disk.
- **J-QUIT**: `q` outside a field, or closing the window, quits. With Human review claims held by this window it asks first: Esc stays, `u` releases the claims and quits.

## Monitor

- **J-MON-CATALOG**: the left side stacks the Scope and Runs panes; the Scope pane lists ALL (this machine) and each repository and worktree; selecting one narrows the Runs pane to its Runs; the selected Run fills the right side as the graph of its Artifacts with their verdicts, under the Run's headline and what needs attention.
- **J-MON-NAVIGATE**: arrow keys or `j`/`k` select a scope or a Run, Enter or → goes one level deeper (Scope, Runs, the Run's graph), Esc goes back a level, and so does ← in the Runs pane.
- **J-MON-DETAIL**: hovering an Artifact or an eval, or selecting it with the keys, shows its Detail summary next to it. Enter, `o` or a click on an eval in an Artifact's eval popup opens its full Detail as an overlay on the right of the graph: instruction, result, request, and with `t` the technical record; `o` on an Artifact opens the Artifact's Detail, and a click on the Run's headline the Run's. Esc closes the overlay.
- **J-MON-ATTENTION**: `!` opens the eval popup of the Artifact of the next ERROR, RED or waiting Human eval, with that eval selected; `b` follows a blocked Artifact or eval to its upstream blocker.
- **J-MON-LIVE**: a change made by another process (a claim, a submission, a new Run) appears in the open Monitor without a reload.
- **J-MON-RESIZE**: the dividers between Scope and Runs and between the left side and the graph can be dragged to resize them.

## Run graph

The Monitor shows a Run only as its graph.

- **J-GRAPH-CURRENT**: `artifactize-desktop graph --repo PATH` (also the declared Human `graph` tool) opens current declarations in the same graph view, without a saved Run; a click or Enter on a node pops up its evals around it, the structure text exposes all criteria and labeled relations, Refresh or Ctrl+R/F5 reloads declarations. There are no Run verdicts on this screen.
- **J-GRAPH-COLOURS**: nodes and edges are coloured by verdict (GREEN, running or waiting for a Human, waiting, RED, ERROR, blocked) and recolour in place when a result arrives.
- **J-GRAPH-MOVE**: ← → move to the upstream or downstream Artifact; ↑ ↓ to the neighbour in the same column; the selected Artifact's summary shows next to it.
- **J-GRAPH-EVALS**: Enter or a click on an Artifact pops up its evals with their verdicts around the node, ↑ ↓ select one (its summary shows next to it), Esc closes the popup.
- **J-GRAPH-LARGE**: a Run too large to draw at once stays navigable: moving beyond the drawn nodes brings the next ones into view and focus follows.

## Human review

- **J-HUMAN-CLAIM**: `c` claims a waiting Human review for this reviewer; `u` releases it. A claim made elsewhere shows as taken.
- **J-HUMAN-TOOL**: the Tools list runs a Human tool (`Enter`): an output tool shows its output, a launch tool opens the application, a failing tool shows its error.
- **J-HUMAN-GREEN**: after a claim `g` opens the GREEN form built from the eval's pass schema; Ctrl+S submits. The completed result stays open.
- **J-HUMAN-RED**: `r` opens the RED form; a result that fails its schema is not submitted, the failing path is shown and the form stays correctable. Ctrl+G and Ctrl+R switch the verdict while editing.
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
