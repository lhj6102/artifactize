# Desktop app: screens, states and transitions

Every screen and state of the app, and every transition between them with a stable edge id
(`E-...`). A journey in UX-FEATURES.md is a path through these edges; the last section lists
each journey's path. Edge ids never change; the end-to-end tests cite them, each by a test
that makes the transition happen and checks where it leads.

## Screens and states

- **Launch**: the window opening. It opens on the Monitor, or on Review when started with
  `review`. The window's title is "Artifactize". The person narrows
  the view inside the app, with the Scope pane or the Review search.
- **Monitor**: **Scope** and **Runs** stacked on the left, the selected Run's **Graph** on the
  right, the divider between them always shown. Scope lists ALL repositories and each repository
  and worktree; Runs lists the selected scope's Runs, newest first; Graph shows the Run's headline
  and what needs attention above the Run graph, and the **notice** of what went wrong as one line
  in its header. With no Run, while loading, on an error or with an empty graph, Graph stays and
  says so. The **Detail** floats over the graph docked toward its right edge, with a gap, and
  shows the Run or an Artifact; its left edge takes a drag that sets its width. An Artifact's
  Detail has tabs, a chip choice that reads as a path, `Artf:{name} / [eval] …`: the Artifact's
  own, listing its evals with their verdicts, then one per eval with its verdict's glyph and colour, showing its Summary (instruction, result and
  request, and with `t` its technical record) or, switched below the tabs, its Agent log or
  Runtime log; for a Human eval the Human review. Hovering only highlights; nothing opens on
  hover.
  States: Scope, Runs or Graph focused, Detail open on a tab (Summary or log), notice in full.
- **Run graph**: the selected Run's Artifacts as nodes and their dependencies as edges,
  coloured by verdict: GREEN, running or waiting for a Human, waiting, RED, ERROR, blocked. It
  takes the pointer; Tab reaches its Artifacts and Enter on one does what a click does.
- **Current graph**: current declarations opened with `artifactize-desktop graph --repo PATH`
  or the Human `graph` tool, without a saved Run or Run verdicts. States: whole graph,
  an Artifact's evals docked over the graph's right side, structure text expanded (all
  evaluation criteria and labeled relations). Refresh, Ctrl+R or F5 reloads declarations.
- **Review**: the saved-request list (at first the waiting Human reviews of every repository)
  with a read-only preview of the selected request, and the search field above it. States:
  Requests focused, Search focused (with or without suggestions), a request open (a Human
  review is actionable, any other request read-only).
- **Human review** (on a Human eval's tab of the Monitor's Detail, or in Review): Unclaimed, Taken (another reviewer
  holds the claim), Claimed (by this reviewer), Editing GREEN
  (the form built from the eval's pass schema), Editing RED (built from its fail schema),
  Submitted; a tool may be running.
- **Settings**: Codex account, API keys, Models, Remote store (URL, share level and token).
- **Overlays**: Palette (a filterable list of commands), Help (the keys of the screen below),
  Leave dialog (quit while this window holds claims).

## Window

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-LAUNCH-MONITOR | Launch | start without arguments | Monitor |
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
| E-REFRESH | any screen | F5, or Ctrl+R outside verdict editing (while editing a verdict Ctrl+R switches to RED, E-HUMAN-SWITCH) | same screen, state reloaded |
| E-QUIT-CLEAN | any screen, no claims held | `q` outside fields, or closing the window from anywhere | window closed |
| E-QUIT-ASK | any screen, claims held | `q` outside fields, or closing the window from anywhere | Leave dialog |
| E-QUIT-STAY | Leave dialog | Esc | the screen below |
| E-QUIT-RELEASE | Leave dialog | `u` | claims released, window closed |

## Monitor

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-MON-SCOPE-MOVE | Scope | `j`, `k`, ↑ or ↓ | Scope, another scope selected (ALL, a repository or a worktree); Runs narrows to its Runs |
| E-MON-SCOPE-RUNS | Scope | Enter or → | Runs |
| E-MON-RUNS-MOVE | Runs | `j`, `k`, ↑ or ↓ | Runs, another Run selected; Graph shows it |
| E-MON-RUNS-GRAPH | Runs | Enter or → | Graph focused |
| E-MON-GRAPH-TO-RUNS | Graph, no Detail open | Esc | Runs |
| E-MON-RUNS-TO-SCOPE | Runs | Esc or ← | Scope |
| E-MON-NOTICE | Graph with a notice | click the notice | the notice shown in full; another click shows it as one line again |
| E-MON-DETAIL-ARTIFACT | Graph | click an Artifact, or Enter on a focused one | Detail of that Artifact opened on its Artifact tab, its evals listed with their verdicts |
| E-MON-DETAIL-TAB | Detail of an Artifact | click an eval's tab, or an eval listed on the Artifact tab | Detail, that eval's tab shown: its instruction, result and request, or its Human review |
| E-MON-DETAIL-SWITCH | Detail | click another Artifact | Detail of that Artifact, on its Artifact tab |
| E-MON-DETAIL-RUN | Graph | click the Run's headline | Detail of the Run opened |
| E-MON-DETAIL-TECHNICAL | Detail on an eval's Summary | `t` | Detail, the technical record shown or hidden |
| E-MON-DETAIL-LOG | Detail on the tab of an eval with a log | click its Agent log or Runtime log, or Summary | Detail, that eval's log shown, or its Summary again |
| E-MON-DETAIL-BACK | Detail | Esc, ←, Close, or a click on the graph's background | the graph, Detail closed |
| E-MON-LIVE | Monitor | another process changes state | same state, updated |
| E-MON-RESIZE | Monitor | drag a divider | same state, pane resized |
| E-MON-DETAIL-WIDTH | Detail | drag its left edge | Detail resized, kept for the next opening and launch |
| E-GRAPH-RECOLOUR | Graph | a result arrives | Graph, nodes and edges recoloured in place |
| E-GRAPH-BEYOND | Graph of a large Run | pan or zoom the camera past the drawn nodes | Graph, the Artifacts brought into view drawn |

## Current graph

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-CURRENT-LAUNCH | Launch | start with `graph --repo PATH`, directly or via the Human `graph` tool | Current graph, current declarations drawn without Run verdicts |
| E-CURRENT-EVALS | Current graph | click an Artifact, or Enter on a focused one | Current graph, its evals docked over the graph's right side |
| E-CURRENT-STRUCTURE | Current graph | expand Structure and evaluation criteria, or click a docked eval | Current graph, complete criteria and labeled relations shown as text |
| E-CURRENT-CLOSE | Current graph, an Artifact's evals docked | Esc, Close, or a click on the graph's background | Current graph, the evals closed |
| E-CURRENT-REFRESH | Current graph | Refresh, Ctrl+R or F5 | Current graph, declarations reloaded and the graph and structure text updated |

## Human review

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-HUMAN-CLAIM | Unclaimed | `c` | Claimed |
| E-HUMAN-RELEASE | Claimed | `u` | Unclaimed |
| E-HUMAN-CLAIMED-ELSEWHERE | Unclaimed | another process claims | Taken, shown with the claimant; claiming here is refused |
| E-HUMAN-RELEASED-ELSEWHERE | Taken | the other process releases | Unclaimed |
| E-HUMAN-TOOL | any | Enter on an output tool | same state, the tool's output shown |
| E-HUMAN-TOOL-LAUNCH | any | Enter on a launch tool | same state, the application opened and said so |
| E-HUMAN-TOOL-FAIL | any | Enter on a tool that fails | same state, the tool's error shown |
| E-HUMAN-GREEN | Claimed | `g` | Editing GREEN, the form built from the pass schema |
| E-HUMAN-RED | Claimed | `r` | Editing RED, the form built from the fail schema |
| E-HUMAN-SWITCH | Editing | Ctrl+G or Ctrl+R | Editing GREEN for Ctrl+G, Editing RED for Ctrl+R (the current verdict stays selected when its own shortcut is used) |
| E-HUMAN-SUBMIT | Editing GREEN | Ctrl+S | Submitted |
| E-HUMAN-INVALID | Editing RED | Ctrl+S with a result failing its schema | Editing RED, failing path shown |
| E-HUMAN-TEXT | Editing | type text, including composed Korean | Editing, no shortcut fires |
| E-HUMAN-ESC-CANCEL | a tool or submission running | Esc | same state, the running work cancelled |
| E-HUMAN-ESC-FORM | Editing | Esc | Claimed |
| E-HUMAN-ESC-DETAIL | Unclaimed, Claimed or Submitted, nothing running or being edited | Esc | the list or graph the Detail was opened from |

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

Each journey of UX-FEATURES.md as walks through the edges above: `→` joins edges one after the
other (each starts where the one before ended), `|` separates alternatives, and a journey with
several walks lists them one per line. Selecting a row first is the movement edge of its pane.

- J-LAUNCH: E-LAUNCH-MONITOR | E-LAUNCH-REVIEW
- J-SWITCH-SCREEN: E-SCREEN-MONITOR | E-SCREEN-REVIEW | E-SCREEN-SETTINGS
- J-PALETTE: E-PALETTE-OPEN → E-PALETTE-FILTER → E-PALETTE-RUN | E-PALETTE-CLOSE
- J-HELP: E-HELP-OPEN → E-HELP-CLOSE
- J-REFRESH: E-REFRESH
- J-QUIT: E-QUIT-CLEAN | E-QUIT-ASK → E-QUIT-STAY | E-QUIT-ASK → E-QUIT-RELEASE
- J-MON-CATALOG: E-LAUNCH-MONITOR → E-MON-SCOPE-MOVE → E-MON-SCOPE-RUNS → E-MON-RUNS-MOVE → E-MON-RUNS-GRAPH
  E-MON-RUNS-MOVE → E-MON-NOTICE
- J-MON-NAVIGATE: E-MON-SCOPE-MOVE → E-MON-SCOPE-RUNS → E-MON-RUNS-MOVE → E-MON-RUNS-GRAPH → E-MON-GRAPH-TO-RUNS → E-MON-RUNS-TO-SCOPE
- J-MON-DETAIL: E-MON-DETAIL-ARTIFACT → E-MON-DETAIL-TAB → E-MON-DETAIL-TECHNICAL → E-MON-DETAIL-LOG → E-MON-DETAIL-SWITCH → E-MON-DETAIL-BACK
  E-MON-DETAIL-RUN → E-MON-DETAIL-BACK
- J-MON-LIVE: E-MON-LIVE
- J-MON-RESIZE: E-MON-RESIZE
  E-MON-DETAIL-ARTIFACT → E-MON-DETAIL-WIDTH → E-MON-DETAIL-BACK → E-MON-DETAIL-ARTIFACT
- J-GRAPH-CURRENT: E-CURRENT-LAUNCH → E-CURRENT-EVALS → E-CURRENT-STRUCTURE → E-CURRENT-CLOSE → E-CURRENT-REFRESH
- J-GRAPH-COLOURS: E-MON-RUNS-GRAPH → E-GRAPH-RECOLOUR
- J-GRAPH-LARGE: E-MON-RUNS-GRAPH → E-GRAPH-BEYOND
- J-HUMAN-CLAIM: E-HUMAN-CLAIM → E-HUMAN-RELEASE
  E-HUMAN-CLAIMED-ELSEWHERE → E-HUMAN-RELEASED-ELSEWHERE → E-HUMAN-CLAIM
- J-HUMAN-TOOL: E-HUMAN-TOOL | E-HUMAN-TOOL-LAUNCH | E-HUMAN-TOOL-FAIL
- J-HUMAN-GREEN: E-HUMAN-CLAIM → E-HUMAN-GREEN → E-HUMAN-SUBMIT
- J-HUMAN-RED: E-HUMAN-CLAIM → E-HUMAN-RED → E-HUMAN-INVALID → E-HUMAN-SWITCH
- J-HUMAN-TEXT: E-HUMAN-CLAIM → E-HUMAN-GREEN → E-HUMAN-TEXT
- J-HUMAN-ESC: E-HUMAN-ESC-CANCEL
  E-HUMAN-CLAIM → E-HUMAN-GREEN → E-HUMAN-ESC-FORM → E-HUMAN-ESC-DETAIL
- J-REVIEW-LIST: E-LAUNCH-REVIEW | E-SCREEN-REVIEW → E-REVIEW-PREVIEW
- J-REVIEW-OPEN: E-REVIEW-OPEN | E-REVIEW-OPEN-TOOLS | E-REVIEW-OPEN-ID | E-REVIEW-READONLY
- J-REVIEW-SEARCH: E-REVIEW-SEARCH-FOCUS → E-REVIEW-SEARCH-TERMS → E-REVIEW-SUGGEST → E-REVIEW-SUGGEST-CLOSE → E-REVIEW-SEARCH-CLEAR → E-REVIEW-SEARCH-LEAVE
- J-REVIEW-PAGE: E-REVIEW-PAGE
- J-SET-KEYS: E-SCREEN-SETTINGS → E-SET-KEYS
- J-SET-CODEX: E-SET-CODEX-LOGIN → E-SET-CODEX-RETURN | E-SET-CODEX-LOGIN → E-SET-CODEX-PASTE | E-SET-CODEX-LOGIN → E-SET-CODEX-CANCEL; E-SET-CODEX-LOGOUT
- J-SET-MODELS: E-SET-MODELS
- J-SET-REMOTE: E-SET-REMOTE-LOGIN → E-SET-REMOTE-STATUS → E-SET-REMOTE-PUSH → E-SET-REMOTE-LOGOUT
