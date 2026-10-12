# Desktop app: screens, states and transitions

Every screen and state of the app, and every transition between them with a stable edge id
(`E-...`). A journey in FEATURES.md is a path through these edges; the last section lists
each journey's path. Edge ids never change; the end-to-end tests cite them, each by a test
that makes the transition happen and checks where it leads.

## Screens and states

- **Launch**: the window opening. It opens on the Monitor, or on Review when started with
  `review`, and shows the repository it was started for (the Monitor's breadcrumb, the Review
  search).
- **Monitor**: four panes. **Scope** lists ALL repositories and each repository and worktree;
  **Runs** lists the selected scope's Runs, newest first; **Tree** shows the selected Run's
  Artifacts and their evals, each with its verdict; **Detail** shows the selected row.
  States: Scope, Runs, Tree or Graph focused, Graph zoomed into an Artifact, Detail open.
- **Run graph**: the selected Run's Artifacts as nodes and their dependencies as edges,
  coloured by verdict: GREEN, running or waiting for a Human, waiting, RED, ERROR, blocked.
- **Review**: the saved-request list (at first the waiting Human reviews of every repository)
  with a read-only preview of the selected request, and the search field above it. States:
  Requests focused, Search focused (with or without suggestions), a request open (a Human
  review is actionable, any other request read-only).
- **Human review** (in the Monitor's Detail or in Review): Unclaimed, Claimed, Editing GREEN
  (the form built from the eval's pass schema), Editing RED (built from its fail schema),
  Submitted; a tool may be running.
- **Settings**: Codex account, API keys, Models, Remote store (URL, share level and token).
- **Overlays**: Palette (a filterable list of commands), Help (the keys of the screen below),
  Leave dialog (quit while this window holds claims).

## Window

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-LAUNCH-MONITOR | Launch | start without arguments | Monitor, showing the repository in its breadcrumb |
| E-LAUNCH-REVIEW | Launch | start with `review` | Review, listing the waiting Human reviews of every repository |
| E-SCREEN-MONITOR | any screen | Ctrl+1 | Monitor |
| E-SCREEN-REVIEW | any screen | Ctrl+2 | Review |
| E-SCREEN-SETTINGS | any screen | Ctrl+, | Settings |
| E-PALETTE-OPEN | any screen | Ctrl+K or the palette button | Palette, all enabled commands listed |
| E-PALETTE-FILTER | Palette | type text | Palette, the commands filtered by the text |
| E-PALETTE-RUN | Palette | Enter on a command | the command's target, Palette closed |
| E-PALETTE-CLOSE | Palette | Esc or a click outside | the screen below |
| E-HELP-OPEN | any screen, outside fields | `?` | Help, listing the keys of the current screen |
| E-HELP-CLOSE | Help | Esc | the screen below |
| E-REFRESH | any screen | Ctrl+R or F5 | same screen, state reloaded |
| E-QUIT-CLEAN | any screen, no claims held, outside fields | `q`, or closing the window | window closed |
| E-QUIT-ASK | any screen, claims held, outside fields | `q`, or closing the window | Leave dialog |
| E-QUIT-STAY | Leave dialog | Esc | the screen below |
| E-QUIT-RELEASE | Leave dialog | `u` | claims released, window closed |

## Monitor

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-MON-SCOPE-MOVE | Scope | `j`, `k` or arrows | Scope, another scope selected; Runs lists its Runs |
| E-MON-SCOPE-RUNS | Scope | Enter or → | Runs |
| E-MON-RUNS-MOVE | Runs | `j`, `k` or arrows | Runs, another Run selected; Tree shows it |
| E-MON-RUNS-TREE | Runs | Enter or → | Tree |
| E-MON-TREE-MOVE | Tree | `j`, `k` or arrows | Tree, other row selected; Detail previews it |
| E-MON-TREE-DETAIL | Tree row | Enter, `o` or double-click | Detail opened |
| E-MON-DETAIL-TECHNICAL | Detail of an eval | `t` | Detail, the technical record shown or hidden |
| E-MON-DETAIL-BACK | Detail | Esc or ← | Tree |
| E-MON-TREE-TO-RUNS | Tree | Esc or ← | Runs |
| E-MON-RUNS-TO-SCOPE | Runs | Esc or ← | Scope |
| E-MON-TREE-FOLD | Tree row | Space, `h` or `l` | Tree, row folded or unfolded |
| E-MON-ATTENTION | Tree | `!` | Tree, next ERROR, RED or waiting Human eval selected |
| E-MON-BLOCKER | Tree, blocked eval | `b` | Tree, upstream blocker selected |
| E-MON-LIVE | Monitor | another process changes state | same state, updated |
| E-MON-RESIZE | Monitor | drag a divider | same state, pane resized |
| E-GRAPH-OPEN | Tree | `v` | Graph |
| E-GRAPH-CLOSE | Graph | `v` | Tree |
| E-GRAPH-RECOLOUR | Graph | a result arrives | Graph, nodes and edges recoloured in place |
| E-GRAPH-MOVE | Graph | arrows | Graph, neighbouring Artifact selected |
| E-GRAPH-ZOOM-IN | Graph | Enter or click on an Artifact | Graph zoomed into its evals |
| E-GRAPH-EVAL-SELECT | Graph zoomed | ↑ ↓ | Graph zoomed, another eval selected; Detail follows |
| E-GRAPH-EVAL-OPEN | Graph zoomed | Enter on an eval | Detail opened |
| E-GRAPH-ZOOM-OUT | Graph zoomed | Esc | Graph |
| E-GRAPH-BEYOND | Graph of a large Run | move past the drawn nodes | Graph, next nodes drawn, focus follows |

## Human review

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-HUMAN-CLAIM | Unclaimed | `c` | Claimed |
| E-HUMAN-RELEASE | Claimed | `u` | Unclaimed |
| E-HUMAN-CLAIMED-ELSEWHERE | Unclaimed | another process claims | Unclaimed, shown as taken |
| E-HUMAN-TOOL | any | Enter on an output tool | same state, the tool's output shown |
| E-HUMAN-TOOL-LAUNCH | any | Enter on a launch tool | same state, the application opened and said so |
| E-HUMAN-TOOL-FAIL | any | Enter on a tool that fails | same state, the tool's error shown |
| E-HUMAN-GREEN | Claimed | `g` | Editing GREEN, the form built from the pass schema |
| E-HUMAN-RED | Claimed | `r` | Editing RED, the form built from the fail schema |
| E-HUMAN-SWITCH | Editing | Ctrl+G or Ctrl+R | Editing the other verdict |
| E-HUMAN-SUBMIT | Editing GREEN | Ctrl+S | Submitted |
| E-HUMAN-INVALID | Editing RED | Ctrl+S with a result failing its schema | Editing RED, failing path shown |
| E-HUMAN-TEXT | Editing | type text, including composed Korean | Editing, no shortcut fires |
| E-HUMAN-ESC-CANCEL | a tool or submission running | Esc | same state, the running work cancelled |
| E-HUMAN-ESC-FORM | Editing | Esc | Claimed |
| E-HUMAN-ESC-DETAIL | Claimed | Esc | the list or tree the Detail was opened from |

## Review

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-REVIEW-PREVIEW | Requests | select a request | Requests, read-only preview shown |
| E-REVIEW-OPEN | Requests | Enter or → on a Human review | Human review open |
| E-REVIEW-OPEN-TOOLS | Requests | the context menu or the palette command "Open the selected request with Tools focused" | Human review open, Tools focused |
| E-REVIEW-OPEN-ID | Requests | Ctrl+O and an ID | request open |
| E-REVIEW-READONLY | Requests | Enter on a non-Human request | request open, read-only |
| E-REVIEW-SEARCH-FOCUS | Requests | `/` or Ctrl+F | Search focused |
| E-REVIEW-SEARCH-TERMS | Search | type free text and `key:value` terms (repo, eval, request, reviewer, tag, kind, status, is), all of which must match | Search, Requests filtered, suggestions for the last term |
| E-REVIEW-SUGGEST | Search with suggestions | ↑ ↓ then Tab or Enter | Search, the suggestion taken |
| E-REVIEW-SUGGEST-CLOSE | Search with suggestions | Esc | Search, suggestions closed, query kept |
| E-REVIEW-SEARCH-CLEAR | Search, no suggestions, query not empty | Esc | Search, query cleared, every request listed |
| E-REVIEW-SEARCH-LEAVE | Search | Enter after a finished term, or Esc on an empty query | Requests |
| E-REVIEW-PAGE | Requests | PgDn or PgUp | Requests, older or newer page |

## Settings

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-SET-KEYS | Settings | open | API keys shown as set or not, never their values |
| E-SET-CODEX-LOGIN | Settings | Sign in with Codex | sign-in in progress; the system browser opened at the sign-in address, which is also shown |
| E-SET-CODEX-RETURN | sign-in in progress | the browser returns to the app's loopback address | signed in |
| E-SET-CODEX-PASTE | sign-in in progress | paste the redirect URL | signed in |
| E-SET-CODEX-CANCEL | sign-in in progress | Cancel sign-in | Settings, signed out |
| E-SET-CODEX-LOGOUT | Settings, signed in | Sign out of Codex | Settings, signed out |
| E-SET-MODELS | Settings | List models | models of the chosen backend shown |
| E-SET-REMOTE-LOGIN | Settings | enter the store URL, share level and token, then Sign in to the store | store signed in, status shown, token cleared from the form |
| E-SET-REMOTE-STATUS | Settings | Check status | status refreshed |
| E-SET-REMOTE-PUSH | Settings, store signed in | Push (dry run) or Push results | push result shown |
| E-SET-REMOTE-LOGOUT | Settings, store signed in | Sign out of the store | store signed out |

## Journeys as paths

Each journey of FEATURES.md as the edges it takes, in order.

- J-LAUNCH: E-LAUNCH-MONITOR or E-LAUNCH-REVIEW
- J-SWITCH-SCREEN: E-SCREEN-MONITOR, E-SCREEN-REVIEW, E-SCREEN-SETTINGS
- J-PALETTE: E-PALETTE-OPEN, E-PALETTE-FILTER, E-PALETTE-RUN or E-PALETTE-CLOSE
- J-HELP: E-HELP-OPEN, E-HELP-CLOSE
- J-REFRESH: E-REFRESH
- J-QUIT: E-QUIT-CLEAN; with claims E-QUIT-ASK, then E-QUIT-STAY or E-QUIT-RELEASE
- J-MON-CATALOG: E-LAUNCH-MONITOR, E-MON-SCOPE-MOVE, E-MON-RUNS-MOVE
- J-MON-NAVIGATE: E-MON-SCOPE-MOVE, E-MON-SCOPE-RUNS, E-MON-RUNS-MOVE, E-MON-RUNS-TREE, E-MON-TREE-MOVE, E-MON-TREE-FOLD, E-MON-DETAIL-BACK, E-MON-TREE-TO-RUNS, E-MON-RUNS-TO-SCOPE
- J-MON-DETAIL: E-MON-TREE-DETAIL, E-MON-DETAIL-TECHNICAL, E-MON-DETAIL-BACK
- J-MON-ATTENTION: E-MON-ATTENTION, E-MON-BLOCKER
- J-MON-LIVE: E-MON-LIVE
- J-MON-RESIZE: E-MON-RESIZE
- J-GRAPH-SWITCH: E-GRAPH-OPEN, E-GRAPH-CLOSE
- J-GRAPH-COLOURS: E-GRAPH-OPEN, E-GRAPH-RECOLOUR
- J-GRAPH-MOVE: E-GRAPH-MOVE
- J-GRAPH-ZOOM: E-GRAPH-ZOOM-IN, E-GRAPH-EVAL-SELECT, E-GRAPH-EVAL-OPEN, E-GRAPH-ZOOM-OUT
- J-GRAPH-LARGE: E-GRAPH-BEYOND
- J-HUMAN-CLAIM: E-HUMAN-CLAIM, E-HUMAN-RELEASE, E-HUMAN-CLAIMED-ELSEWHERE
- J-HUMAN-TOOL: E-HUMAN-TOOL, E-HUMAN-TOOL-LAUNCH, E-HUMAN-TOOL-FAIL
- J-HUMAN-GREEN: E-HUMAN-CLAIM, E-HUMAN-GREEN, E-HUMAN-SUBMIT
- J-HUMAN-RED: E-HUMAN-CLAIM, E-HUMAN-RED, E-HUMAN-INVALID, E-HUMAN-SWITCH
- J-HUMAN-TEXT: E-HUMAN-GREEN, E-HUMAN-TEXT
- J-HUMAN-ESC: E-HUMAN-ESC-CANCEL, E-HUMAN-ESC-FORM, E-HUMAN-ESC-DETAIL
- J-REVIEW-LIST: E-LAUNCH-REVIEW or E-SCREEN-REVIEW, E-REVIEW-PREVIEW
- J-REVIEW-OPEN: E-REVIEW-OPEN, E-REVIEW-OPEN-TOOLS, E-REVIEW-OPEN-ID, E-REVIEW-READONLY
- J-REVIEW-SEARCH: E-REVIEW-SEARCH-FOCUS, E-REVIEW-SEARCH-TERMS, E-REVIEW-SUGGEST, E-REVIEW-SUGGEST-CLOSE, E-REVIEW-SEARCH-CLEAR, E-REVIEW-SEARCH-LEAVE
- J-REVIEW-PAGE: E-REVIEW-PAGE
- J-SET-KEYS: E-SET-KEYS
- J-SET-CODEX: E-SET-CODEX-LOGIN, then E-SET-CODEX-RETURN or E-SET-CODEX-PASTE or E-SET-CODEX-CANCEL; E-SET-CODEX-LOGOUT
- J-SET-MODELS: E-SET-MODELS
- J-SET-REMOTE: E-SET-REMOTE-LOGIN, E-SET-REMOTE-STATUS, E-SET-REMOTE-PUSH, E-SET-REMOTE-LOGOUT
