# Desktop App and TUI

The desktop app (`artifactize-desktop`) and terminal UI (`artifactize monitor` and `artifactize review`) operate on the same local state as the CLI, updating live as Runs progress or teammates' results arrive.

## Desktop app (`artifactize-desktop`) {#open-the-app}

```sh
artifactize-desktop                         # open with all projects listed
artifactize-desktop monitor --repo PATH     # select and expand PATH's branch on its newest entry
artifactize-desktop review [REQUEST_ID]     # open newest waiting Human review (or REQUEST_ID)
artifactize-desktop graph --repo PATH       # open PATH's Current declaration graph
```

Global shortcuts (on macOS, use `Command` for `Ctrl`):

| Key | Action |
|---|---|
| `Ctrl+K`, `F1` | Searchable **Commands** palette |
| `?` | Context **Help** and graph legend |
| `Ctrl+,` | **Settings** modal (**Providers**, **Remote**, **Limits**) |
| `Ctrl+B` | Show or hide the left **Pane** |
| `/` | Focus branch and waiting-review search in the Pane |
| `Ctrl+F` | **Find** in the graph when a graph is shown and focus is outside the Pane; otherwise focuses Pane search |
| `Ctrl+O` | **Open a request by ID** (waiting Human reviews actionable; others read-only) |
| `Ctrl+R`, `F5` | Reload state and rescan branches/Current (`F5` inside a Human review where `Ctrl+R` selects RED) |
| `q` | Quit when focus is outside editable text fields, Settings is closed, and focus is not inside a Human review that is editing a form or running work (move focus out of a text field, close Settings, or press `Esc` in that review first; quitting from elsewhere still prompts to **Stay** or **Release and quit** (`u`) if this window holds unsubmitted claims) |

### Left Pane: Search, Branches, and Waiting reviews {#pane}

- **Search (`/`)**: free-text, `repo:`, and `branch:` terms filter both branches and waiting reviews; `sort:recent` (default) and `sort:name` order the branch list, while waiting reviews are always listed newest first.
- **Branches**: lists each active Git worktree (`project / branch`) or non-Git project folder with Runs on this machine, plus the project the app was started for with `--repo` or `graph --repo` even before its first Run. Star a branch to pin it to the top. Click a branch name to expand its timeline and open its newest entry: the first timeline row is **Current** (replaced in the timeline by an active Run while one is in progress), followed by saved Runs newest first; click **Current** or any saved Run to show it in the Main area, or click **Older runs** at the bottom of the timeline to load more Run entries.
- **Waiting reviews**: stacked at the bottom of the Pane across all repositories (`Waiting (N)`). Clicking an entry opens the Human review in the Main area.

### Main area: Run, Current, Graphs, and Detail {#current}

- **Run view**: shows the Run's status headline (click it or run **Open the Run's Detail** from the palette for the Run's Detail: **Outcome**, **Waits for**, **What**, **Provenance**, **Technical**; move focus into Detail and press `t` to toggle Technical) and the interactive Artifact Graph.
- **Current view and editing**: scans the worktree like `artifactize status` and draws the graph showing what `verify` would do now (`reused`, `would run`, `waiting`, `blocked`).
  - **Creating and editing definitions**: click **New Artifact** in the tool strip, enter a **Name** and **Path**, and press `Enter` to create `.artifactize/<name>.artf`. In an Artifact's Detail tab, **Definition** edits `Path` (press `Enter` to save), `Parts` (click `+ Add`, type to filter suggestions, use the arrow keys to highlight the intended Artifact and press `Enter`—or click the suggestion directly—since `Enter` accepts the highlighted suggestion; click `✕` to remove), `Comment` (opens the inline comment editor on the node when inline controls are active; on large zoomed-out graphs, zoom in until inline controls appear or edit `[annotations.comment]` in **Source**), `Rename`, and `Delete`, while **Source** edits the full `.artf` file (`Ctrl+S` to Save, **Revert** to reload).
  - **Placing nodes, parts, and comments**: drag a node or a whole's **title bar** to position it in `[annotations.style]` (dragging a whole's inner face or the background pans the graph). Dropping a node on empty space inside its own whole's box repositions it; dropping it on empty space inside a different whole's box (or directly onto another node, even inside the same box) makes that target box or node its new whole in `parts` (removing it from its previous whole); dropping it on empty graph space outside every box removes its part membership. Click a node's comment to edit `[annotations.comment]` (`Enter` saves, `Shift+Enter` adds a line), or drag the comment onto another Artifact that has no comment to move it between their files.
- **Graph navigation**: click or `Tab`+`Enter` a node to open its **Detail** docked on the right; press `m` to **Minimize**/**Expand** a whole's box; use `Ctrl+F` (**Find**) to jump to any Artifact, eval, or starred favorite; use the bottom-right **Navigator** minimap when part of the graph is off-screen. On large graphs, zooming out until names are too small to read renders status-colored blocks that still open **Detail** when clicked; zooming in until names are readable and sufficiently few nodes are in view restores per-node inline controls (`Tab`+`Enter`, **Minimize**/**Expand**, inline comment editing; clicking a node still opens **Detail** whenever inline controls are absent), while node dragging stays disabled on large graphs regardless of zoom (edit positions under `[annotations.style]` in **Source** and change part membership under **Definition → Parts**).
- **Detail tabs**:
  - **Artifact tab**: shows **Evals**, **Neighbours** (inputs on left, consumers on right), declared Human **Tools** (runnable immediately in the worktree with a live **Tool log**), **Path** (or **Definition** and **Source** on Current), and **Debug** (press `t` with focus in Detail to toggle). Click the star beside the title to favorite the Artifact.
  - **Eval tab**: click an eval row or chip (`[icon] {name} / [eval]`) to open that eval's tab: a Human eval with a recorded request in the Run opens its **Human review** directly in the tab, while other evals show the eval summary by default and, for a recorded request in a Run, add a **Summary** switch alongside **Agent log** (showing the live transcript when the local conversation is available) or **Runtime log** (showing saved `stdout`/`stderr`, or why output is unavailable).
- **Human review view**: click **Claim to review** (`c`), run any declared **Tools** and inspect the **Tool log**, then choose **GREEN** (`g` / `Ctrl+G`) or **RED** (`r` / `Ctrl+R`), fill the schema-backed form, and press **Submit** (`Ctrl+S`). To release a claim without submitting, click **Release claim** or press `u` when focus is outside a text field.
- **Settings (`Ctrl+,`)**: manage **Providers** (sign in/out of `codex` or `claude-agent`, save/remove API keys for `openai`/`anthropic`/`gemini`, add custom alias or endpoint providers, and list models), **Remote** (sign in/out of the review store, check status, dry-run or push results), and **Limits** (`signoffIdleGraceSeconds`).

## Terminal UI (`artifactize monitor` and `artifactize review`) {#monitor}

```sh
artifactize monitor [PATH | --repo PATH | --all]   # starts on ALL, or preselects PATH's scope
artifactize review [REQUEST_ID] [--repo PATH]      # opens REQUEST_ID directly, or lists requests
```

- **Monitor**: without a path (or with `--all`), starts on the `ALL` scope; passing `PATH` or `--repo PATH` preselects that repository or worktree. Drills down through **Scope → Runs → Run tree → Detail**, displaying the focused **Full** pane alongside **Compact** (left) and **Preview** (right) panes where those adjacent levels exist (`Scope` has no Compact pane and `Detail` has no Preview pane).
  - Keys: `↑`/`↓` or `j`/`k` selects rows in the navigation panes and scrolls inside the focused read-only Detail pane (an Agent request with a local conversation opens its live/saved **Agent transcript** first and `d` switches between the transcript and saved result details; without a transcript, read-only Detail starts with **Sections** focused, and when an evidence/output pane is present beside or below **Sections**, `Tab` or `Shift+Tab` switches focus between **Sections** and that pane to scroll saved runtime output); `Enter`/`→` steps in (or opens Full Detail from the tree); `Esc`/`←` steps back (in an Agent's saved result details view opened with `d`, `Esc` returns to the Agent transcript while `←` leaves Detail back to the Run tree; while editing a Human verdict form or running a Human tool, `←` and `q` do not leave or quit—use `Esc` first); `Space` (`h`/`l`) folds/unfolds tree rows; `!` jumps to the next `ERROR`, `RED`, or waiting Human eval; `b` cycles through the selected row's upstream dependencies (`Backspace` returns); `t` toggles Technical in read-only details (in an Agent transcript, press `d` first to view result details, then `t`); `F2` toggles mouse capture; `?` opens help; `q` quits outside editing or busy Human work.
- **Review and Shared Human Detail**: `artifactize review` starts on the request list filtered by `kind:human status:waiting_human` (narrowed by `repo:` when `--repo PATH` is given; passing `REQUEST_ID` opens Detail directly). In the list, press `/` to edit the search (`repo:`, `eval:`, `request:`, `reviewer:`, `tag:`, `kind:`, `status:`, `is:mine|claimed|unclaimed`; `Tab` autocompletes, `Enter` keeps the query, `Esc` restores it), `↑`/`↓` or `j`/`k` to select a request, `Enter` or `→` to open its Detail, and `q` to quit (if this session holds unsubmitted claims, `q` prompts with `k` to keep them and quit, `u` to release them and quit, or `Esc` to stay). Inside Human Detail (shared by `review` and `monitor`): press `c` to claim; before editing a form, `Tab` switches between Tools and Fields (`Shift+Tab` returns to Tools from inside an edited verdict form, where `Tab` moves between form fields); with Tools focused, `↑`/`↓` selects a tool and `Enter` runs it; `g` or `r` (`Ctrl+G`/`Ctrl+R`) opens the GREEN/RED form; `Ctrl+S` submits; `u` (`Ctrl+U` while editing) releases the claim; `i`/`t` toggles the instruction or Technical details while not editing a form (while typing in a text field, those keys insert text instead); and `Esc` steps back one stage at a time (cancels running work, exits form editing, and returns from Detail to the request list or tree).
