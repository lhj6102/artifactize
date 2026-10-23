# Desktop app: screens, states and transitions

Every screen and state of the app, and every transition between them with a stable edge id
(`E-...`). A journey in UX-FEATURES.md is a path through these edges; the last section lists
each journey's path. Edge ids never change; the end-to-end tests cite them, each by a test
that makes the transition happen and checks where it leads.

## Screens and states

- **Launch**: the window opening, titled "Artifactize". It opens on its one screen, the
  **Pane** beside the **Main area**, the divider between them always shown. Started with
  `--repo PATH` it selects the branch of PATH; with `review ID` it opens that review, with
  `review` alone the newest waiting one; with `graph --repo PATH` (also the declared Human
  `graph` tool) it shows the Current of the branch of PATH. The Branches list the projects with
  Runs on this machine and the one the app was started for with `--repo` or `graph --repo`,
  even before its first Run (J-BRANCHES). There is no screen switch.
- **Pane**: one column: the **Search** field at the top, the **Branches** below it and the
  **Waiting** stack at the bottom. There is no Scope heading and no Runs list.
  - **Search**: its starting query is `sort:recent`. Free text and `key:value` terms (`repo:`,
    `branch:`, `sort:recent`, `sort:name`) must all match; they narrow the Branches and the
    Waiting stack alike. States: not focused, focused, focused with suggestions.
  - **Branches**: one entry per live branch of every project with Runs on this machine, read
    as `project / branch`, the branch in bold and the project quieter: a Git worktree that
    exists now with the branch it has checked out (a detached one by its folder's name), or a
    folder outside Git that exists now and holds its declarations. A removed worktree or
    folder, or one Git no longer lists, has no entry. Who ran a Run makes no entry: a branch's
    Runs are its project's Runs on that branch, from whichever folder they ran in. Each entry
    has a **chevron** before its name and its **star** at its line's end. Folded, the name
    sits over the branch's newest Run, its status dot and word on the left and its age on the
    right (a running one its time so far), or `No Runs yet`. Expanded, its **timeline** sits
    under the name instead: one entry per Run, newest first, each a dot in its status colour,
    its status word, its age on the right and its Run ID below, joined by a line through the
    dots, starting with the Run in progress on the branch, or else with the **Current**, a
    ring rather than a dot, reading `Working tree` below. The entry shown in the Main area
    sits in a grey pill. **Older runs** at a timeline's end, while older Runs exist, brings
    them in below the oldest. Any number of branches may be expanded at once, for the
    window's life; the branch the app was started for starts expanded. Starred branches come
    first; within each group `sort:recent` puts the branch with the most recent Run first and
    branches without Runs last, and `sort:name` orders by project and branch. States: a
    branch folded or expanded, selected, starred or not, one of its entries shown (in its
    pill) and selected.
  - **Waiting**: the waiting Human reviews of every repository on this machine, those of
    removed branches included, newest at the top, each with its eval, its project and branch,
    how long it waits and who claimed it. With none it is a thin bar at the bottom of the pane
    reading `(0)`; each waiting review adds a row, the area growing upward like a stack up to
    half of the pane's height, beyond which it scrolls. States: a waiting review selected.
  - The keys move one selection through the Branches and then the Waiting stack. Focus is in
    the Pane (its entries or its Search) or in the Main area.
- **Main area**: shows one view at a time, with what went wrong (such as Runs that cannot be
  read) as the **notice**, one line in its header that neither moves nor hides what is below.
  - **Nothing chosen**: no branch or review chosen yet; the graph's place says so.
  - **Run**: the shown Run's headline and what needs attention above the **Run graph**; while
    loading, on an error or with an empty graph the graph's place stays and says so.
  - **Current**: the branch's worktree as it is now, scanned as `artifactize status` does: a
    header saying when it was scanned, a Refresh button, the **Current graph** and, below it,
    the expandable **Structure and evaluation criteria** text (every criterion and labeled
    relation). It scans again when chosen again, on Refresh and when a Run of the branch ends.
  - **Review**: a waiting Human review opened from the Pane, its eval and Artifact above the
    **Human review**. Choosing a branch or a timeline entry leaves it without closing it: its claim, draft
    and Tool log stay for when it is opened again.
  - **Request**: a request opened by ID that is not a waiting Human review, read-only.
- **Settings**: a modal over the screen with two tabs. **Providers** lists the built-in providers
  (Codex, OpenAI, Anthropic, Gemini), the custom providers and **+ Custom provider** on the left, and
  the chosen one's setup on the right: the Codex account, an API key, or a custom provider's form,
  each with its Models. **Remote** is the shared review store (URL, share level and token).
- **Run graph**: the shown Run's Artifacts as nodes and their dependencies as edges, coloured
  by verdict: GREEN, running or waiting for a Human, waiting, RED, ERROR, blocked.
- **Current graph**: the current declarations' Artifacts and relations, each Artifact coloured
  by what verify would do now: reused (every eval's result would be reused without
  evaluation), would run, waiting, blocked. A node shows only its name, as on a Run graph;
  read aloud it says which, would run with how many of its evals would run.
- Both graphs take the pointer: Tab reaches their Artifacts and Enter on one does what a click
  does; they have no other keys of their own. Hovering only highlights.
- **Detail**: floats over a graph docked toward its right edge, with a gap, and shows a Run or
  an Artifact; it never sits beside the Pane. Its left edge takes a drag that sets its width,
  between a readable column and a width that leaves some of the graph visible on its left.
  An Artifact's Detail has tabs, a chip choice that reads as a path, `[Artifact icon] {name} /
  [eval] …`: the Artifact's own chip (the Artifact icon before its name, read aloud as
  "Artifact {name}"; only the name is cut short when long, the Detail's title showing it
  whole), then a `/` and one chip per eval with its verdict's glyph and colour, on a Current
  whether it would run or be reused; an Artifact without evals has its own chip only, without
  the `/`. Eval chips that do not fit on one line wrap onto further lines within the Detail,
  the `/` staying with the Artifact's chip. The Artifact's tab draws it between its
  neighbours with its graph's nodes and edges (the Artifacts its evals use on its left, those
  whose evals use it on its right), a drawing that does not pan, zoom or drag, then lists its
  evals with their verdicts, then its **Tools** and their **Tool log**, then its path, every
  other fact (what it waits for included) under Debug (unfolded with `t` or a click). Its
  Tools are the Human tools the Artifact declares in the branch's worktree as it is now,
  whichever Run is shown, each a `▶` button; the graph's nodes carry no tool buttons. An
  eval's tab shows its Summary (its status, beside it in grey what running it took and spent:
  time, tokens in `k` or `M` and model, or what reuse saved; its instruction; its result, each
  owner field by name and a list as numbered items; execution options, metadata and
  fingerprint, every other fact under Debug), on a Current whether it would run or be reused
  and why and the result it would reuse, or, switched below the tabs, its Agent log or Runtime
  log; in a Run a Human eval's tab holds its Human review. Everything in the Detail,
  transcript and log lines, long words and code included, wraps or scrolls within its width.
  States: open on a tab (the Artifact's, an eval's Summary, Debug unfolded, or a log), its
  chips on one line or wrapped; an Artifact's tool running.
- **Human review** (on a Human eval's tab of a Run's Detail, or the Review view): its
  instruction, the Tools (a `▶` button each, grouped by the Artifact that declares them, the
  reviewed one first), the Tool log of every run in this review, and the Result. States:
  Unclaimed, Taken (another reviewer holds the claim), Claimed (by this reviewer), Editing
  GREEN (the form built from the eval's pass schema), Editing RED (built from its fail schema),
  Submitted (the completed result shown); a tool or a submission may be running, and a
  launched application may still be logging. The form has text, number, true/false and choice
  fields, lists of text (one text per item) and lists of groups of fields; a list always keeps
  one item to type in. Constants are filled in and not shown; `*` marks a required field and
  an empty optional one says Optional in grey. There is no JSON editor.
- **Tool log** (of a review, or of an Artifact while the app runs): each run as `▶ HH:MM:SS ·
  operation · Artifact`, then each line the tool prints (stderr in red), then its end line
  (`■ exit 0`, `■ launched`, `■ done`, `■ error: …`); a launched application's lines keep
  arriving under its own run until it exits. It is monospace, wraps within its width and
  scrolls in its own box. Its states: Following (it shows its newest line as lines arrive)
  and Scrolled up (the reader scrolled away from the bottom: it stays where the reader left
  it while lines arrive below). Its text selects and copies like any page text. A review's
  submission clears its log; an Artifact's tool runs record nothing as a review's evidence.
- **Overlays**: Palette (a filterable list of commands), Help (the keys of what has focus),
  Open by ID (a field for a request ID), Leave dialog (quit while this window holds claims).

## Window

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-LAUNCH-MONITOR | Launch | start without arguments | Pane and Main area, the Main area with Nothing chosen |
| E-LAUNCH-REPO | Launch | start with `--repo PATH` | the branch of PATH selected and expanded in the Branches, its newest entry shown in the Main area |
| E-LAUNCH-REVIEW | Launch | start with `review` | the newest waiting Human review open in the Main area as Review |
| E-LAUNCH-REVIEW-ID | Launch | start with `review ID` | the review ID open in the Main area as Review |
| E-CURRENT-LAUNCH | Launch | start with `graph --repo PATH`, directly or via the Human `graph` tool | the branch of PATH selected, its Current shown |
| E-SCREEN-SETTINGS | any view | Ctrl+, or the Settings button | Settings open over the view, on the Providers tab |
| E-SETTINGS-BACK | Settings | Esc, Close, or the Settings button again | Settings closed, the view beneath as it was |
| E-PALETTE-OPEN | any view | Ctrl+K or the palette button | Palette, all enabled commands listed |
| E-PALETTE-FILTER | Palette | type text | Palette, the commands filtered by the text |
| E-PALETTE-RUN | Palette | Enter on a command | the command's target, Palette closed |
| E-PALETTE-CLOSE | Palette | Esc or a click outside | the view below |
| E-HELP-OPEN | any view, outside fields | `?` | Help, listing the keys of what has focus (Pane, Main area or Settings) |
| E-HELP-CLOSE | Help | Esc | the view below |
| E-REFRESH | any view | F5, or Ctrl+R outside verdict editing (while editing a verdict Ctrl+R switches to RED, E-HUMAN-SWITCH) | same view, the state reloaded and the Branches scanned again; a shown Current scanned again (E-CURRENT-REFRESH) |
| E-REVIEW-OPEN-ID | any view | Ctrl+O, or the palette's "Open a request by ID", then a waiting Human review's ID | Review of that request in the Main area, actionable |
| E-REVIEW-READONLY | any view | Ctrl+O, or the palette's "Open a request by ID", then the ID of any other request | Request: that request in the Main area, read-only |
| E-QUIT-CLEAN | any view, no claims held | `q` outside fields, or closing the window from anywhere | window closed |
| E-QUIT-ASK | any view, claims held | `q` outside fields, or closing the window from anywhere | Leave dialog |
| E-QUIT-STAY | Leave dialog | Esc | the view below |
| E-QUIT-RELEASE | Leave dialog | `u` | claims released, window closed |

## Pane

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-PANE-SELECT | Pane | click a branch's name | that branch selected and expanded, its newest entry (the running Run, else Current) shown in the Main area in its pill |
| E-PANE-STAR | Pane | click a branch's star | the branch starred, or its star taken away; starred branches listed first, each group in the search's order, kept for the next monitor |
| E-PANE-CHIP | Pane, a branch expanded | click an entry of its timeline | that entry selected and shown in the Main area (a Run as Run, the Current as Current), in its pill |
| E-PANE-FOLD | Pane | click a branch's chevron | the branch expanded to its timeline, or folded back to its newest Run's status; nothing chosen |
| E-PANE-OLDER | Pane, a branch expanded with older Runs | click Older runs | older Runs listed below the oldest; nothing chosen |
| E-MON-SCOPE-MOVE | Pane entries focused | `j`, `k`, ↑ or ↓ | the next or previous entry selected, through the Branches, the timelines of expanded branches and then the Waiting stack; a branch shows its newest entry in the Main area, a timeline entry shows itself, a waiting review is only selected; the list scrolls to show the selected entry |
| E-MON-EXPAND | Pane entries focused, a branch or a timeline entry selected | → or ← | → expands a folded branch, or enters an expanded one's timeline on its newest entry; ← steps from a timeline entry back to its branch, or folds an expanded branch; what is shown stays unless an entry is entered |
| E-MON-RUNS-MOVE | Pane entries focused, a timeline entry selected | ↓ or ↑ | the next or previous entry shown in the Main area and in its pill, the list scrolling to show it |
| E-MON-RUNS-OLDER | Pane entries focused, the oldest listed entry of a timeline selected, older Runs existing | ↓ | older Runs listed below it, the selection staying; ↓ again moves into them |
| E-MON-RUNS-GRAPH | Pane entries focused, a branch or a timeline entry selected | Enter | Main area focused, on the shown view |
| E-MON-GRAPH-TO-RUNS | Main area focused, no Detail open | Esc | Pane entries focused, the same entry selected |
| E-REVIEW-OPEN | Pane, a waiting review in the Waiting stack | click it, or Enter on it selected | Review of that request in the Main area |
| E-PANE-WORKTREES | Pane | a worktree of a listed project is added or removed (or its folder deleted), in another process | within seconds and without a reload, its branch appears in the Branches or goes from them |
| E-BRANCH-RUNNING | Pane | a Run starts on a listed branch, in another process | its timeline starts with that Run, `Running` and its time so far, the Current gone; folded, its status line says the same |
| E-BRANCH-RUN-END | Pane, a branch's timeline starting with a running Run | that Run ends | its entry shows its final status and age; the timeline starts with the Current |
| E-WAITING-CHANGE | Pane | a Human review starts or stops waiting, in this window or another process | the Waiting stack adds or drops its row, the bar's count following: `(0)` with none |
| E-MAIN-REVIEW-LEAVE | Review in the Main area | choose a branch or an entry of its timeline | that view shown, the review left without being closed; opening it again shows it as it was left |
| E-MON-NOTICE | Main area with a notice, focus anywhere | click the notice in the Main area's header | the notice shown in full, wrapped within the header, what is below neither moved nor hidden; another click shows it as one line again |
| E-MON-LIVE | any view | another process changes the state (a new Run, a result, a claim, a submission) | same view, updated without a reload |
| E-MON-RESIZE | Pane and Main area | drag the divider between them (hovering it shows only the resize cursor) | same view, the split resized and kept whatever the Main area shows |

## Search

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-REVIEW-SEARCH-FOCUS | Pane or Main area, outside fields | `/` or Ctrl+F | Search focused |
| E-REVIEW-SEARCH-TERMS | Search | type free text, `repo:` or `branch:` terms, all of which must match | Search, the Branches and the Waiting stack narrowed alike (free text: part of a project, a branch or a waiting review's eval; `repo:` part of the project; `branch:` part of the branch), suggestions for the last term |
| E-PANE-SORT | Search | replace `sort:recent` with `sort:name`, or back | Branches ordered by project and branch, or most recent Run first with branches without Runs last |
| E-REVIEW-SUGGEST | Search with suggestions | ↑ ↓ then Tab or Enter | Search, the suggestion (a key, a project, a branch or a sort order) taken |
| E-REVIEW-SUGGEST-CLOSE | Search with suggestions | Esc | Search, suggestions closed, query kept |
| E-REVIEW-SEARCH-CLEAR | Search, no suggestions, query other than `sort:recent` | Esc | Search, `sort:recent` put back, every branch and waiting review listed |
| E-REVIEW-SEARCH-LEAVE | Search | Enter after a finished term, or Esc on `sort:recent` | Pane entries focused |

## Main area

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-CURRENT-CHIP | Pane | choose a branch's Current (a click on it, ↓ or ↑ onto it, or a branch selected without a Run in progress), also when it is already shown | Current: the worktree scanned again, its header saying when; the Current graph with each Artifact coloured reused, would run, waiting or blocked, its name read aloud with which (would run with its count of evals) |
| E-CURRENT-RUN-END | Current of a branch shown | a Run of that branch ends | Current scanned again, its header the new time, the graph recoloured in place |
| E-CURRENT-REFRESH | Current | Refresh, Ctrl+R or F5 | Current scanned again and its graph and structure text updated |
| E-CURRENT-STRUCTURE | Current | expand Structure and evaluation criteria | Current, every criterion and labeled relation shown as text |
| E-GRAPH-RECOLOUR | Run graph or Current graph | a result arrives, or a scan ends | the graph, nodes and edges recoloured in place |
| E-GRAPH-TAB | Run graph or Current graph, the Main area focused | Tab | the graph, the next Artifact focused, nothing opened |
| E-GRAPH-BEYOND | Run graph or Current graph too large to draw at once | pan or zoom the camera past the drawn nodes | the graph, the Artifacts brought into view drawn |
| E-GRAPH-FIRST | Pane | choose an entry whose graph is not shown yet | its Run graph or Current graph drawn whole |
| E-GRAPH-CAMERA | Run graph or Current graph | pan, zoom, or Fit View | the graph, its camera where the person put it, or whole again after Fit View |
| E-GRAPH-CAMERA-KEPT | Run graph or Current graph | open, switch, resize or close the Detail; a result or scan arrives; a new layout | the graph, its camera neither zoomed nor panned |

## Detail

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-MON-DETAIL-ARTIFACT | Run graph | click an Artifact, or Enter on a focused one | Detail of that Artifact opened on its Artifact tab: its chips as a path (eval chips wrapping onto further lines with the `/` kept with the Artifact's chip; no `/` without evals), the Artifact between its neighbours, its evals listed with their verdicts, its Tools and Tool log, then its path, every other fact (what it waits for included) folded under Debug |
| E-CURRENT-EVALS | Current graph | click an Artifact, or Enter on a focused one | Detail of that Artifact over the Current: its eval chips and listed evals saying whether each would run or be reused, the Artifact between its neighbours, its Tools and Tool log, its path, the rest under Debug |
| E-MON-DETAIL-TAB | Detail of an Artifact of a Run | click an eval's tab, or an eval listed on the Artifact tab | Detail, that eval's tab shown: its Summary (status, instruction, result), or its Human review, long words and code wrapping within the Detail's width |
| E-CURRENT-TAB | Detail of an Artifact of a Current | click an eval's tab, or an eval listed on the Artifact tab | Detail, that eval's Summary: whether it would run or be reused and why, and the result it would reuse |
| E-MON-DETAIL-NEIGHBOUR | Detail of an Artifact, on its Artifact tab | click a neighbour drawn beside it | Detail of that Artifact, on its Artifact tab, between its own neighbours |
| E-MON-DETAIL-SWITCH | Detail | click another Artifact of the graph | Detail of that Artifact, on its Artifact tab |
| E-MON-DETAIL-RUN | Run graph | click the Run's headline | Detail of the Run opened |
| E-MON-DETAIL-TECHNICAL | Detail on an eval's Summary or on an Artifact's tab | `t`, or a click on Debug | Detail, its Debug facts (an eval's request, source, raw result and the rest; an Artifact's other facts) shown or hidden |
| E-MON-DETAIL-LOG | Detail on the tab of an eval with a log | click its Agent log or Runtime log, or Summary | Detail, that eval's log shown, its lines wrapping within the Detail's width and scrolling in its own box, or its Summary again |
| E-DETAIL-TOOL | Detail on an Artifact's tab, no tool of it running | click an output tool's `▶` | same Detail, the tool running at once in the branch's worktree with no claim until it ends, its run in the Artifact's Tool log: `▶ HH:MM:SS · operation · Artifact`, each printed line, `■ exit 0`; no review's evidence recorded |
| E-DETAIL-TOOL-LAUNCH | Detail on an Artifact's tab, no tool of it running | click a launch tool's `▶` | same Detail at once, the application opened and said so; its run open in the Artifact's Tool log until it exits |
| E-DETAIL-TOOL-FAIL | Detail on an Artifact's tab, no tool of it running | click a tool that fails | same Detail, the failure said and its run ended red in the Artifact's Tool log |
| E-MON-DETAIL-BACK | Detail, no Human review in it running work or editing its form | Esc, ←, Close, or a click on the graph's background | the graph, Detail closed |
| E-CURRENT-CLOSE | Detail over a Current | Esc, Close, or a click on the graph's background | the Current graph, Detail closed |
| E-MON-DETAIL-WIDTH | Detail | drag its left edge (hovering it shows only the resize cursor) | Detail resized, never narrower than a readable column nor so wide that no graph shows on its left, never over the Pane; the width kept for the next opening and launch |

## Human review

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-HUMAN-CLAIM | Unclaimed | `c` | Claimed |
| E-HUMAN-RELEASE | Claimed | `u` | Unclaimed |
| E-HUMAN-CLAIMED-ELSEWHERE | Unclaimed | another process claims | Taken, shown with the claimant; claiming here is refused |
| E-HUMAN-RELEASED-ELSEWHERE | Taken | the other process releases | Unclaimed |
| E-HUMAN-TOOL | Claimed or Editing | click an output tool's `▶` button | same state, the tool running until it ends, its run in the Tool log: `▶ HH:MM:SS · operation · Artifact`, each printed line, `■ exit 0` |
| E-HUMAN-TOOL-LAUNCH | Claimed or Editing | click a launch tool's `▶` button | same state at once, the application opened and said so; its run open in the Tool log |
| E-HUMAN-LOG-LIVE | a launched application logging | it prints, then exits | same state, its lines (stderr in red) and then its end added under its own run |
| E-HUMAN-TOOL-FAIL | Claimed or Editing | click a tool that fails | same state, the failure said and its run ended red in the Tool log |
| E-HUMAN-GREEN | Claimed | `g` | Editing GREEN, the form built from the pass schema |
| E-HUMAN-RED | Claimed | `r` | Editing RED, the form built from the fail schema |
| E-HUMAN-SWITCH | Editing | Ctrl+G or Ctrl+R | Editing GREEN for Ctrl+G, Editing RED for Ctrl+R (the current verdict stays selected when its own shortcut is used) |
| E-HUMAN-SUBMIT | Editing | Ctrl+S with a result its schema accepts | Submitted: the form gone, the completed result open, the Tool log cleared |
| E-HUMAN-LIST-ADD | Editing a form with a list of text | `+ Add` under the list | Editing, another empty item to type in |
| E-HUMAN-LIST-REMOVE | Editing a form with a list of text | `✕` on an item | Editing, that item removed; one empty item stays when it was the last |
| E-HUMAN-GROUP-ADD | Editing a form with a list of groups | `+ Add` | Editing, another empty group of the list's fields |
| E-HUMAN-GROUP-REMOVE | Editing a form with a list of groups | `✕` on a group | Editing, that group removed; one empty group stays when it was the last |
| E-HUMAN-INVALID | Editing RED or Editing GREEN | Ctrl+S with a result failing its schema | the same form, its entries kept and correctable, failing path shown |
| E-HUMAN-TEXT | Editing | type text, including composed Korean | Editing, no shortcut fires |
| E-HUMAN-LOG-UP | Tool log Following, longer than its box | scroll it up | Tool log Scrolled up; new lines arrive below without moving it, and the rest of the review does not scroll |
| E-HUMAN-LOG-FOLLOW | Tool log Scrolled up | scroll it back to its bottom | Tool log Following, showing each newest line again |
| E-HUMAN-LOG-COPY | Editing, the Tool log holding lines | select text in the Tool log, copy it and paste it into a Result field | Editing, the field holding the log's text as shown |
| E-HUMAN-ESC-CANCEL | a tool or submission running | Esc in the review or on the Detail's tabs, the review's Close, or Esc on the graph or a click on its background | same state, the running work cancelled, the review still open |
| E-HUMAN-ESC-FORM | Editing, nothing running | Esc, Close or a click as for E-HUMAN-ESC-CANCEL | Claimed, the form left with its draft kept, the review still open |
| E-HUMAN-ESC-DETAIL | Unclaimed, Taken, Claimed or Submitted, nothing running or being edited | Esc | in a Run's Detail the graph, Detail closed; for a review opened from the Pane, Pane entries focused |

## Settings

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-SET-TAB | Settings | choose the Providers or Remote tab | that tab shown |
| E-SET-PROVIDER | Providers | choose a provider in the list | its setup shown on the right |
| E-SET-KEY | an API-key provider (OpenAI, Anthropic, Gemini) | open | its key shown as set from the environment, saved or not set, never its value |
| E-SET-KEY-SAVE | an API-key provider | paste a key, then Save key | the key saved and the field cleared; shown as saved, or still set from the environment when its variable is set, since that is used first |
| E-SET-KEY-REMOVE | an API-key provider, key saved | Remove saved key | the key shown as not set, or set from the environment |
| E-SET-CUSTOM-NEW | Providers | + Custom provider | an empty custom provider form |
| E-SET-CUSTOM-SAVE | custom provider form | name it, choose an alias of a built-in provider or an endpoint (API, URL, optional key), then Save | the custom provider listed by name and chosen, its key never shown |
| E-SET-CUSTOM-REMOVE | a custom provider | Remove provider | the custom provider gone from the list |
| E-SET-CODEX-LOGIN | Codex | Sign in with Codex | sign-in in progress; the system browser opened at the sign-in address, which is also shown |
| E-SET-CODEX-RETURN | sign-in in progress | the browser returns to the app's loopback address | signed in |
| E-SET-CODEX-PASTE | sign-in in progress | paste the redirect URL | signed in |
| E-SET-CODEX-CANCEL | sign-in in progress | Cancel sign-in | Codex, signed out |
| E-SET-CODEX-LOGOUT | Codex, signed in | Sign out of Codex | Codex, signed out |
| E-SET-MODELS | a provider's setup | List models | models of the chosen provider shown |
| E-SET-REMOTE-LOGIN | Remote | enter the store URL, share level and token, then Sign in to the store | store signed in, status shown, token cleared from the form |
| E-SET-REMOTE-STATUS | Remote | Check status | status refreshed |
| E-SET-REMOTE-PUSH | Remote, store signed in | Push (dry run) or Push results | push result shown |
| E-SET-REMOTE-LOGOUT | Remote, store signed in | Sign out of the store | store signed out |

## Journeys as paths

Each journey of UX-FEATURES.md as walks through the edges above: `→` joins edges one after the
other (each starts where the one before ended), `|` separates alternatives, and a journey with
several walks lists them one per line. Selecting an entry first is E-MON-SCOPE-MOVE or
E-PANE-SELECT.

- J-LAUNCH: E-LAUNCH-MONITOR | E-LAUNCH-REPO | E-LAUNCH-REVIEW | E-LAUNCH-REVIEW-ID | E-CURRENT-LAUNCH
- J-SETTINGS: E-SCREEN-SETTINGS → E-SETTINGS-BACK
- J-PALETTE: E-PALETTE-OPEN → E-PALETTE-FILTER → E-PALETTE-RUN | E-PALETTE-CLOSE
- J-HELP: E-HELP-OPEN → E-HELP-CLOSE
- J-REFRESH: E-REFRESH
  E-CURRENT-CHIP → E-REFRESH
- J-QUIT: E-QUIT-CLEAN | E-QUIT-ASK → E-QUIT-STAY | E-QUIT-ASK → E-QUIT-RELEASE
- J-OPEN-ID: E-REVIEW-OPEN-ID | E-REVIEW-READONLY
- J-PANE: E-LAUNCH-MONITOR → E-MON-RESIZE
- J-BRANCHES: E-LAUNCH-MONITOR → E-PANE-SELECT
  E-PANE-WORKTREES
- J-BRANCH-RUNS: E-PANE-FOLD → E-PANE-SELECT → E-PANE-OLDER → E-PANE-CHIP
- J-BRANCH-STAR: E-PANE-STAR → E-PANE-STAR
  E-BRANCH-RUNNING → E-BRANCH-RUN-END
- J-PANE-SEARCH: E-REVIEW-SEARCH-FOCUS → E-REVIEW-SEARCH-TERMS → E-REVIEW-SUGGEST → E-REVIEW-SUGGEST-CLOSE → E-REVIEW-SEARCH-CLEAR → E-REVIEW-SEARCH-LEAVE
  E-REVIEW-SEARCH-FOCUS → E-PANE-SORT
- J-PANE-NAVIGATE: E-MON-SCOPE-MOVE → E-MON-EXPAND → E-MON-RUNS-MOVE → E-MON-RUNS-GRAPH → E-MON-GRAPH-TO-RUNS
  E-MON-RUNS-MOVE → E-MON-RUNS-OLDER
- J-WAITING: E-WAITING-CHANGE → E-REVIEW-OPEN
- J-LIVE: E-MON-LIVE | E-PANE-WORKTREES | E-BRANCH-RUNNING | E-WAITING-CHANGE
- J-RESIZE: E-MON-RESIZE
  E-MON-DETAIL-ARTIFACT → E-MON-DETAIL-WIDTH → E-MON-DETAIL-BACK → E-MON-DETAIL-ARTIFACT
- J-MAIN-RUN: E-PANE-CHIP → E-MON-NOTICE
- J-MAIN-CURRENT: E-CURRENT-CHIP → E-CURRENT-STRUCTURE → E-CURRENT-REFRESH
  E-CURRENT-CHIP → E-CURRENT-RUN-END
- J-MAIN-REVIEW: E-REVIEW-OPEN → E-MAIN-REVIEW-LEAVE → E-REVIEW-OPEN
- J-DETAIL: E-MON-DETAIL-ARTIFACT → E-MON-DETAIL-TAB → E-MON-DETAIL-LOG → E-MON-DETAIL-SWITCH → E-MON-DETAIL-BACK
  E-MON-DETAIL-RUN → E-MON-DETAIL-BACK
  E-CURRENT-CHIP → E-CURRENT-EVALS → E-CURRENT-CLOSE
  E-MON-RUNS-GRAPH → E-GRAPH-TAB → E-MON-DETAIL-ARTIFACT → E-MON-DETAIL-BACK
  E-MON-RUNS-GRAPH → E-GRAPH-TAB → E-CURRENT-EVALS → E-CURRENT-CLOSE
- J-DETAIL-NEIGHBOURS: E-MON-DETAIL-ARTIFACT → E-MON-DETAIL-TECHNICAL → E-MON-DETAIL-NEIGHBOUR → E-MON-DETAIL-NEIGHBOUR → E-MON-DETAIL-TAB
- J-DETAIL-TOOLS: E-MON-DETAIL-ARTIFACT → E-DETAIL-TOOL | E-DETAIL-TOOL-LAUNCH | E-DETAIL-TOOL-FAIL
  E-CURRENT-EVALS → E-DETAIL-TOOL
- J-DETAIL-SUMMARY: E-MON-DETAIL-ARTIFACT → E-MON-DETAIL-TAB → E-MON-DETAIL-TECHNICAL → E-MON-DETAIL-LOG
  E-CURRENT-EVALS → E-CURRENT-TAB
- J-GRAPH-COLOURS: E-PANE-CHIP → E-GRAPH-RECOLOUR
  E-CURRENT-CHIP → E-CURRENT-RUN-END
- J-GRAPH-LARGE: E-PANE-CHIP → E-GRAPH-BEYOND
- J-GRAPH-CAMERA: E-GRAPH-FIRST → E-GRAPH-CAMERA-KEPT → E-GRAPH-CAMERA → E-GRAPH-CAMERA-KEPT
- J-HUMAN-CLAIM: E-HUMAN-CLAIM → E-HUMAN-RELEASE
  E-HUMAN-CLAIMED-ELSEWHERE → E-HUMAN-RELEASED-ELSEWHERE → E-HUMAN-CLAIM
- J-HUMAN-LAYOUT: E-MON-DETAIL-TAB | E-REVIEW-OPEN
- J-HUMAN-TOOL: E-HUMAN-CLAIM → E-HUMAN-TOOL | E-HUMAN-TOOL-LAUNCH | E-HUMAN-TOOL-FAIL
- J-HUMAN-LOG: E-HUMAN-TOOL → E-HUMAN-TOOL-FAIL
  E-HUMAN-TOOL-LAUNCH → E-HUMAN-GREEN → E-HUMAN-LOG-LIVE
  E-HUMAN-TOOL → E-HUMAN-LOG-UP → E-HUMAN-TOOL → E-HUMAN-LOG-FOLLOW → E-HUMAN-TOOL
  E-HUMAN-TOOL → E-HUMAN-GREEN → E-HUMAN-LOG-COPY → E-HUMAN-SUBMIT
- J-HUMAN-GREEN: E-HUMAN-CLAIM → E-HUMAN-GREEN → E-HUMAN-SUBMIT
- J-HUMAN-RED: E-HUMAN-CLAIM → E-HUMAN-RED → E-HUMAN-INVALID → E-HUMAN-SWITCH
- J-HUMAN-FORM: E-HUMAN-CLAIM → E-HUMAN-RED → E-HUMAN-LIST-ADD → E-HUMAN-LIST-REMOVE → E-HUMAN-GROUP-ADD → E-HUMAN-GROUP-REMOVE → E-HUMAN-SUBMIT
- J-HUMAN-TEXT: E-HUMAN-CLAIM → E-HUMAN-GREEN → E-HUMAN-TEXT
- J-HUMAN-ESC: E-HUMAN-CLAIM → E-HUMAN-GREEN → E-HUMAN-TOOL → E-HUMAN-ESC-CANCEL → E-HUMAN-ESC-FORM → E-HUMAN-ESC-DETAIL
  E-REVIEW-OPEN → E-HUMAN-CLAIM → E-HUMAN-GREEN → E-HUMAN-ESC-FORM → E-HUMAN-ESC-DETAIL
- J-SET-PROVIDERS: E-SCREEN-SETTINGS → E-SET-PROVIDER
- J-SET-CODEX: E-SET-PROVIDER → E-SET-CODEX-LOGIN → E-SET-CODEX-RETURN | E-SET-CODEX-LOGIN → E-SET-CODEX-PASTE | E-SET-CODEX-LOGIN → E-SET-CODEX-CANCEL; E-SET-CODEX-LOGOUT
- J-SET-KEY: E-SET-PROVIDER → E-SET-KEY → E-SET-KEY-SAVE → E-SET-KEY-REMOVE
- J-SET-CUSTOM: E-SET-CUSTOM-NEW → E-SET-CUSTOM-SAVE → E-SET-CUSTOM-REMOVE
- J-SET-MODELS: E-SET-PROVIDER → E-SET-MODELS
- J-SET-REMOTE: E-SET-TAB → E-SET-REMOTE-LOGIN → E-SET-REMOTE-STATUS → E-SET-REMOTE-PUSH → E-SET-REMOTE-LOGOUT
