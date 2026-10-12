# Desktop app: screens, states and transitions

Every screen and state of the app, and every transition between them with a stable edge id
(`E-...`). A journey in FEATURES.md is a path through these edges. Edge ids never change; the
end-to-end tests cite them.

## Screens and states

- **Launch**: the window opening.
- **Monitor**: panes Scope, Runs, Tree or Graph, and Detail. States: Tree, Graph, Graph zoomed into an Artifact, Detail open.
- **Review**: Requests list with a preview, the search field, and a request open (Human review actionable, other kinds read-only).
- **Human review** (inside Monitor Detail or Review): Unclaimed, Claimed, Editing GREEN, Editing RED, Submitted.
- **Settings**: Codex, API keys, Models, Remote store.
- **Overlays**: Palette, Help, Leave dialog (quit with claims held).

## Window

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-LAUNCH-MONITOR | Launch | start without arguments | Monitor |
| E-LAUNCH-REVIEW | Launch | start with `review` | Review |
| E-SCREEN-MONITOR | any screen | Ctrl+1 | Monitor |
| E-SCREEN-REVIEW | any screen | Ctrl+2 | Review |
| E-SCREEN-SETTINGS | any screen | Ctrl+, | Settings |
| E-PALETTE-OPEN | any screen | Ctrl+K or the palette button | Palette |
| E-PALETTE-RUN | Palette | Enter on a command | the command's target, Palette closed |
| E-PALETTE-CLOSE | Palette | Esc or a click outside | the screen below |
| E-HELP-OPEN | any screen, outside fields | `?` | Help |
| E-HELP-CLOSE | Help | Esc | the screen below |
| E-REFRESH | any screen | Ctrl+R or F5 | same screen, state reloaded |
| E-QUIT-CLEAN | any screen, no claims held | `q` | window closed |
| E-QUIT-ASK | any screen, claims held | `q` or closing the window | Leave dialog |
| E-QUIT-STAY | Leave dialog | Esc | the screen below |
| E-QUIT-RELEASE | Leave dialog | `u` | claims released, window closed |

## Monitor

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-MON-SCOPE-RUNS | Scope | Enter | Runs |
| E-MON-RUNS-TREE | Runs | Enter | Tree |
| E-MON-TREE-DETAIL | Tree row | Enter, `o` or double-click | Detail |
| E-MON-DETAIL-BACK | Detail | Esc or ← | Tree |
| E-MON-TREE-MOVE | Tree | `j`, `k` or arrows | Tree, other row selected |
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
| E-GRAPH-EVAL | Graph zoomed | ↑ ↓ then Enter | Detail of the selected eval |
| E-GRAPH-ZOOM-OUT | Graph zoomed | Esc | Graph |
| E-GRAPH-BEYOND | Graph of a large Run | move past the drawn nodes | Graph, next nodes drawn, focus follows |

## Human review

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-HUMAN-CLAIM | Unclaimed | `c` | Claimed |
| E-HUMAN-RELEASE | Claimed | `u` | Unclaimed |
| E-HUMAN-CLAIMED-ELSEWHERE | Unclaimed | another process claims | Unclaimed, shown as taken |
| E-HUMAN-TOOL | Claimed or Unclaimed | Enter on a tool | same state, tool output or error shown |
| E-HUMAN-GREEN | Claimed | `g` | Editing GREEN |
| E-HUMAN-RED | Claimed | `r` | Editing RED |
| E-HUMAN-SWITCH | Editing | Ctrl+G or Ctrl+R | Editing the other verdict |
| E-HUMAN-SUBMIT | Editing GREEN | Ctrl+S | Submitted |
| E-HUMAN-INVALID | Editing RED | Ctrl+S with a result failing its schema | Editing RED, failing path shown |
| E-HUMAN-TEXT | Editing | type text, including composed Korean | Editing, no shortcut fires |
| E-HUMAN-ESC-FORM | Editing | Esc | Claimed |
| E-HUMAN-ESC-DETAIL | Claimed | Esc | the list or tree the Detail was opened from |

## Review

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-REVIEW-PREVIEW | Requests | select a request | Requests, read-only preview shown |
| E-REVIEW-OPEN | Requests | Enter or → | request open |
| E-REVIEW-OPEN-TOOLS | Requests | Tab on a Human review | Human review open, Tools focused |
| E-REVIEW-OPEN-ID | Requests | Ctrl+O and an ID | request open |
| E-REVIEW-READONLY | Requests | Enter on a non-Human request | request open, read-only |
| E-REVIEW-SEARCH | Requests | `/` or Ctrl+F, then terms | Requests, filtered |
| E-REVIEW-SUGGEST | Search | ↑ ↓ then Tab or Enter | Search, suggestion taken |
| E-REVIEW-SEARCH-ESC | Search | Esc | suggestions closed, then search cleared, then Requests |
| E-REVIEW-PAGE | Requests | PgDn or PgUp | Requests, older or newer page |

## Settings

| Edge | From | Action | To |
| --- | --- | --- | --- |
| E-SET-KEYS | Settings | open | API keys shown as set or not, never their values |
| E-SET-CODEX-LOGIN | Settings | Sign in with Codex | sign-in in progress |
| E-SET-CODEX-PASTE | sign-in in progress | paste the redirect URL | signed in |
| E-SET-CODEX-CANCEL | sign-in in progress | Cancel sign-in | Settings, signed out |
| E-SET-CODEX-LOGOUT | Settings, signed in | Sign out of Codex | Settings, signed out |
| E-SET-MODELS | Settings | List models | models of the chosen backend shown |
| E-SET-REMOTE-LOGIN | Settings | Sign in to the store | store signed in, status shown |
| E-SET-REMOTE-STATUS | Settings | Check status | status refreshed |
| E-SET-REMOTE-PUSH | Settings, store signed in | Push (dry run) or Push results | push result shown |
| E-SET-REMOTE-LOGOUT | Settings, store signed in | Sign out of the store | store signed out |
