# Desktop app: screens, states and transitions

Every screen and state of the app, and every transition between them with a stable edge id
(`E-...`). A journey in UX-FEATURES.md is a path through these edges; the last section lists
each journey's path. Edge ids never change; the end-to-end tests cite them, each by a test
that makes the transition happen and checks where it leads.

## Screens and states

- **Launch**: the window opening. It opens on the Monitor, or on Review when started with
  `review`. The window's title is "Artifactize". The person narrows
  the view inside the app, with the Scope pane or the Review search.
- **Monitor**: **Scope** and **Runs** stacked on the left, and on the right the **Run pane**
  (the graph area), the divider between them always shown. Scope lists ALL repositories and each
  repository and worktree; Runs lists the selected scope's Runs, newest first. The Run pane is
  titled with the selected Run and shows its headline and what needs attention above its
  **Graph**; the **notice** of what went wrong (such as Runs that cannot be read) is one line in
  the Run pane's header, whichever pane has focus, and neither moves nor hides the Graph. With no
  Run, while loading, on an error or with an empty graph, the Graph's place stays and says so.
  The **Detail** floats over the Graph docked toward its right edge, with a gap, and shows the
  Run or an Artifact. Its left edge takes a drag that sets its width, between a readable column
  and a width that leaves some of the Graph visible on its left; it stays within the graph area,
  never over Scope or Runs. An Artifact's Detail has tabs, a chip choice that reads as a path,
  `[Artifact icon] {name} / [eval] …`: the Artifact's own chip (the Artifact icon before its name,
  read aloud as "Artifact {name}"; only the name is cut short when long, the Detail's title
  showing it whole), then a `/` and one chip per eval
  with its verdict's glyph and colour; an Artifact without evals has its own chip only, without
  the `/`. Eval chips that do not fit on one line wrap onto further lines within the Detail, the
  `/` staying with the Artifact's chip. The Artifact's tab draws it between its neighbours with the
  Run graph's nodes and edges (the Artifacts its evals use on its left, those whose evals use it
  on its right), a drawing that does not pan, zoom or drag, then lists its evals with their verdicts, then
  shows its path, every other fact (what it waits for included) under Debug (unfolded with `t`
  or a click). An eval's tab shows its Summary (its status, beside it in grey what running it took
  and spent: time, tokens in `k` or `M` and model, or what reuse saved; its instruction; its
  result, each owner field by name and a list as numbered items; execution options, metadata
  and fingerprint, every other fact under Debug, unfolded
  with `t` or a click) or, switched below the tabs, its Agent log or Runtime log; for a Human
  eval the Human review. Everything in the Detail, transcript and log lines, long words and code
  included, wraps or scrolls within its width. Hovering only highlights; nothing opens on hover.
  States: Scope, Runs or Graph focused; Detail open on a tab (the Artifact's, an eval's Summary,
  Debug unfolded, or a log), its chips on one line or wrapped; notice as one line or in full.
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
- **Human review** (on a Human eval's tab of the Monitor's Detail, or in Review): its
  instruction, the Tools (a `▶` button each, grouped by the Artifact that declares them, the reviewed one first), the
  Tool log of every run in this review, and the Result. States: Unclaimed, Taken (another
  reviewer holds the claim), Claimed (by this reviewer), Editing GREEN (the form built from the
  eval's pass schema), Editing RED (built from its fail schema), Submitted (the completed result
  shown); a tool or a submission may be running, and a launched application may still be
  logging. The form has text, number, true/false and choice fields, lists of text (one text per
  item) and lists of groups of fields; a list always keeps one item to type in. Constants are
  filled in and not shown; `*` marks a required field and an empty optional one says Optional in
  grey. There is no JSON editor.
  The **Tool log** shows each run as `▶ HH:MM:SS · operation · Artifact`, then each line the tool
  prints (stderr in red), then its end line: `■ exit 0` (or another exit), `■ launched`, `■ done`
  or `■ error: …`. It is monospace, wraps within the review's width and scrolls in its own box,
  apart from the rest of the review. Its states: Following (it shows its newest line as lines
  arrive) and Scrolled up (the reader scrolled away from the bottom: it stays where the reader
  left it while lines arrive below). Its text selects and copies like any page text.
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
| E-SCREEN-REVIEW-COUNT | any screen | a Human review starts or stops waiting, in this window or another process | same screen, Review showing after its name how many wait as Review's starting search lists them, or its name alone with none |
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
| E-MON-NOTICE | Monitor with a notice, any pane focused | click the notice in the Run pane's header | the notice shown in full, wrapped within the Run pane's header, the Graph neither moved nor hidden; another click shows it as one line again |
| E-MON-DETAIL-ARTIFACT | Graph | click an Artifact, or Enter on a focused one | Detail of that Artifact opened on its Artifact tab: its chips as a path (eval chips wrapping onto further lines with the `/` kept with the Artifact's chip; no `/` without evals), the Artifact between its neighbours, its evals listed with their verdicts, then its path, every other fact (what it waits for included) folded under Debug |
| E-MON-DETAIL-TAB | Detail of an Artifact | click an eval's tab, or an eval listed on the Artifact tab | Detail, that eval's tab shown: its Summary (status, instruction, result), or its Human review, long words and code wrapping within the Detail's width |
| E-MON-DETAIL-NEIGHBOUR | Detail of an Artifact, on its Artifact tab | click a neighbour drawn beside it | Detail of that Artifact, on its Artifact tab, between its own neighbours |
| E-MON-DETAIL-SWITCH | Detail | click another Artifact | Detail of that Artifact, on its Artifact tab |
| E-MON-DETAIL-RUN | Graph | click the Run's headline | Detail of the Run opened |
| E-MON-DETAIL-TECHNICAL | Detail on an eval's Summary or on an Artifact's tab | `t`, or a click on Debug | Detail, its Debug facts (an eval's request, source, raw result and the rest; an Artifact's other facts) shown or hidden |
| E-MON-DETAIL-LOG | Detail on the tab of an eval with a log | click its Agent log or Runtime log, or Summary | Detail, that eval's log shown, its lines wrapping within the Detail's width and scrolling in its own box, or its Summary again |
| E-MON-DETAIL-BACK | Detail, no Human review in it running work or editing its form | Esc, ←, Close, or a click on the graph's background | the graph, Detail closed |
| E-MON-LIVE | Monitor | another process changes state | same state, updated |
| E-MON-RESIZE | Monitor | drag a divider | same state, pane resized |
| E-MON-DETAIL-WIDTH | Detail | drag its left edge | Detail resized, never narrower than a readable column nor so wide that no Graph shows on its left, within the graph area and never over Scope or Runs; the width kept for the next opening and launch |
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
| E-HUMAN-TOOL | Claimed or Editing | click an output tool's `▶` button | same state, the tool running until it ends, its run in the Tool log: `▶ HH:MM:SS · operation · Artifact`, each printed line (stderr in red), then `■ exit 0`, or `■ done` for a builtin tool |
| E-HUMAN-TOOL-LAUNCH | Claimed or Editing | click a launch tool's `▶` button | same state at once, the application opened and said so; its run in the Tool log: `▶ HH:MM:SS · operation · Artifact`, then `■ launched` |
| E-HUMAN-LOG-LIVE | a launched application logging | it prints, then exits | same state, its lines (stderr in red) and then its end added under its own run |
| E-HUMAN-TOOL-FAIL | Claimed or Editing | click a tool that fails | same state, the failure said and its run ended in red with `■ error: …` in the Tool log |
| E-HUMAN-GREEN | Claimed | `g` | Editing GREEN, the form built from the pass schema |
| E-HUMAN-RED | Claimed | `r` | Editing RED, the form built from the fail schema |
| E-HUMAN-SWITCH | Editing | Ctrl+G or Ctrl+R | Editing GREEN for Ctrl+G, Editing RED for Ctrl+R (the current verdict stays selected when its own shortcut is used) |
| E-HUMAN-SUBMIT | Editing | Ctrl+S with a result its schema accepts | Submitted: the form gone, the completed result open, the Tool log cleared |
| E-HUMAN-LIST-ADD | Editing a form with a list of text | `+ Add` under the list | Editing, another empty item to type in |
| E-HUMAN-LIST-REMOVE | Editing a form with a list of text | `✕` on an item | Editing, that item removed; one empty item stays when it was the last |
| E-HUMAN-GROUP-ADD | Editing a form with a list of groups | `+ Add` | Editing, another empty group of the list's fields |
| E-HUMAN-GROUP-REMOVE | Editing a form with a list of groups | `✕` on a group | Editing, that group removed; one empty group stays when it was the last |
| E-HUMAN-INVALID | Editing RED | Ctrl+S with a result failing its schema | Editing RED, failing path shown |
| E-HUMAN-TEXT | Editing | type text, including composed Korean | Editing, no shortcut fires |
| E-HUMAN-LOG-UP | Tool log Following, longer than its box | scroll it up | Tool log Scrolled up; new lines arrive below without moving it, and the rest of the review does not scroll |
| E-HUMAN-LOG-FOLLOW | Tool log Scrolled up | scroll it back to its bottom | Tool log Following, showing each newest line again |
| E-HUMAN-LOG-COPY | Editing, the Tool log holding lines | select text in the Tool log, copy it and paste it into a Result field | Editing, the field holding the log's text as shown |
| E-HUMAN-ESC-CANCEL | a tool or submission running | Esc in the review or on the Detail's tabs, the review's Close, or in the Monitor Esc on the Graph or a click on its background | same state, the running work cancelled, the Detail still open |
| E-HUMAN-ESC-FORM | Editing, nothing running | Esc, Close or a click as for E-HUMAN-ESC-CANCEL | Claimed, the form left with its draft kept, the Detail still open |
| E-HUMAN-ESC-DETAIL | Unclaimed, Taken, Claimed or Submitted, nothing running or being edited | Esc | the list or graph the Detail was opened from |

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
- J-SWITCH-SCREEN: E-SCREEN-MONITOR | E-SCREEN-REVIEW | E-SCREEN-SETTINGS | E-SCREEN-REVIEW-COUNT
- J-PALETTE: E-PALETTE-OPEN → E-PALETTE-FILTER → E-PALETTE-RUN | E-PALETTE-CLOSE
- J-HELP: E-HELP-OPEN → E-HELP-CLOSE
- J-REFRESH: E-REFRESH
- J-QUIT: E-QUIT-CLEAN | E-QUIT-ASK → E-QUIT-STAY | E-QUIT-ASK → E-QUIT-RELEASE
- J-MON-CATALOG: E-LAUNCH-MONITOR → E-MON-SCOPE-MOVE → E-MON-SCOPE-RUNS → E-MON-RUNS-MOVE → E-MON-RUNS-GRAPH
  E-MON-RUNS-MOVE → E-MON-NOTICE
- J-MON-NAVIGATE: E-MON-SCOPE-MOVE → E-MON-SCOPE-RUNS → E-MON-RUNS-MOVE → E-MON-RUNS-GRAPH → E-MON-GRAPH-TO-RUNS → E-MON-RUNS-TO-SCOPE
- J-MON-DETAIL: E-MON-DETAIL-ARTIFACT → E-MON-DETAIL-TAB → E-MON-DETAIL-LOG → E-MON-DETAIL-SWITCH → E-MON-DETAIL-BACK
  E-MON-DETAIL-RUN → E-MON-DETAIL-BACK
- J-MON-NEIGHBOURS: E-MON-DETAIL-ARTIFACT → E-MON-DETAIL-TECHNICAL → E-MON-DETAIL-NEIGHBOUR → E-MON-DETAIL-NEIGHBOUR → E-MON-DETAIL-TAB
- J-MON-SUMMARY: E-MON-DETAIL-ARTIFACT → E-MON-DETAIL-TAB → E-MON-DETAIL-TECHNICAL → E-MON-DETAIL-LOG
- J-MON-LIVE: E-MON-LIVE
- J-MON-RESIZE: E-MON-RESIZE
  E-MON-DETAIL-ARTIFACT → E-MON-DETAIL-WIDTH → E-MON-DETAIL-BACK → E-MON-DETAIL-ARTIFACT
- J-GRAPH-CURRENT: E-CURRENT-LAUNCH → E-CURRENT-EVALS → E-CURRENT-STRUCTURE → E-CURRENT-CLOSE → E-CURRENT-REFRESH
- J-GRAPH-COLOURS: E-MON-RUNS-GRAPH → E-GRAPH-RECOLOUR
- J-GRAPH-LARGE: E-MON-RUNS-GRAPH → E-GRAPH-BEYOND
- J-HUMAN-CLAIM: E-HUMAN-CLAIM → E-HUMAN-RELEASE
  E-HUMAN-CLAIMED-ELSEWHERE → E-HUMAN-RELEASED-ELSEWHERE → E-HUMAN-CLAIM
- J-HUMAN-LAYOUT: E-MON-DETAIL-TAB | E-REVIEW-OPEN
- J-HUMAN-TOOL: E-HUMAN-TOOL | E-HUMAN-TOOL-LAUNCH | E-HUMAN-TOOL-FAIL
- J-HUMAN-LOG: E-HUMAN-TOOL → E-HUMAN-TOOL-FAIL
  E-HUMAN-TOOL-LAUNCH → E-HUMAN-GREEN → E-HUMAN-LOG-LIVE
  E-HUMAN-TOOL → E-HUMAN-LOG-UP → E-HUMAN-TOOL → E-HUMAN-LOG-FOLLOW → E-HUMAN-TOOL
  E-HUMAN-TOOL → E-HUMAN-GREEN → E-HUMAN-LOG-COPY → E-HUMAN-SUBMIT
- J-HUMAN-GREEN: E-HUMAN-CLAIM → E-HUMAN-GREEN → E-HUMAN-SUBMIT
- J-HUMAN-RED: E-HUMAN-CLAIM → E-HUMAN-RED → E-HUMAN-INVALID → E-HUMAN-SWITCH
- J-HUMAN-FORM: E-HUMAN-CLAIM → E-HUMAN-RED → E-HUMAN-LIST-ADD → E-HUMAN-LIST-REMOVE → E-HUMAN-GROUP-ADD → E-HUMAN-GROUP-REMOVE → E-HUMAN-SUBMIT
- J-HUMAN-TEXT: E-HUMAN-CLAIM → E-HUMAN-GREEN → E-HUMAN-TEXT
- J-HUMAN-ESC: E-HUMAN-CLAIM → E-HUMAN-GREEN → E-HUMAN-TOOL → E-HUMAN-ESC-CANCEL → E-HUMAN-ESC-FORM → E-HUMAN-ESC-DETAIL
  E-HUMAN-CLAIM → E-HUMAN-GREEN → E-HUMAN-ESC-FORM → E-HUMAN-ESC-DETAIL
- J-REVIEW-LIST: E-LAUNCH-REVIEW | E-SCREEN-REVIEW → E-REVIEW-PREVIEW
- J-REVIEW-OPEN: E-REVIEW-OPEN | E-REVIEW-OPEN-TOOLS | E-REVIEW-OPEN-ID | E-REVIEW-READONLY
- J-REVIEW-SEARCH: E-REVIEW-SEARCH-FOCUS → E-REVIEW-SEARCH-TERMS → E-REVIEW-SUGGEST → E-REVIEW-SUGGEST-CLOSE → E-REVIEW-SEARCH-CLEAR → E-REVIEW-SEARCH-LEAVE
- J-REVIEW-PAGE: E-REVIEW-PAGE
- J-SET-KEYS: E-SCREEN-SETTINGS → E-SET-KEYS
- J-SET-CODEX: E-SET-CODEX-LOGIN → E-SET-CODEX-RETURN | E-SET-CODEX-LOGIN → E-SET-CODEX-PASTE | E-SET-CODEX-LOGIN → E-SET-CODEX-CANCEL; E-SET-CODEX-LOGOUT
- J-SET-MODELS: E-SET-MODELS
- J-SET-REMOTE: E-SET-REMOTE-LOGIN → E-SET-REMOTE-STATUS → E-SET-REMOTE-PUSH → E-SET-REMOTE-LOGOUT
