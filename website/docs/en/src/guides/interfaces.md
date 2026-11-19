# Desktop App

The desktop app (`artifactize-desktop`) operates on the same local state as the CLI, updating live as Runs progress, worktrees appear or leave, or teammates' results arrive. `artifactize monitor` and `artifactize review` are CLI shortcuts that open the desktop app directly.

## Opening the app {#open-the-app}

```sh
artifactize-desktop                                       # open with all projects listed
artifactize-desktop monitor --repo PATH                   # select and expand PATH's branch on its newest entry
artifactize-desktop review [REQUEST_ID] [--reviewer NAME] # open newest waiting Human review (or REQUEST_ID)
artifactize-desktop graph --repo PATH                     # open PATH's Current declaration graph
artifactize monitor [PATH | --repo PATH | --all]          # CLI shortcut to open the desktop app
artifactize review [REQUEST_ID] [--repo PATH | --all] [--reviewer NAME]  # CLI shortcut for Review
```

`artifactize-desktop` also accepts `--state-dir PATH`, `--all`, `--repo PATH` (mutually exclusive with `--all`), and `--reviewer NAME`.

The header gives access to the app's global controls:

- **Pane** switch: shows or hides the left pane while keeping the Main area in place.
- **Commands** button: opens a searchable command palette listing every command available in the current view (**Keyboard shortcuts**, **Show Settings**, **Show or hide the Pane**, **Search the branches**, **Open a request by ID**, **Refresh**, **Quit**, plus commands for the active graph, Human review, or Settings).
- **Settings** button: opens the **Settings** modal (**Providers**, **Remote**, **Limits**).
- **Update notice** (`{version} available`): appears before **Settings** when a newer stable release is out (prereleases are ignored, and Microsoft Store installs do not show it). Clicking it opens update instructions for how the app was installed; the app never downloads or installs updates itself.

Keyboard shortcuts are optional; any shortcut the app offers is shown next to its command in **Commands** and in **Help** (**Commands → Keyboard shortcuts**, which also shows the graph legend). Closing the window or choosing **Quit** while this window holds unsubmitted Human claims prompts to **Stay** or **Release and quit**.

## Left Pane: Search, Branches, and Waiting reviews {#pane}

- **Search**: click the search field at the top of the Pane (or run **Search the branches** from **Commands**). Free-text, `repo:`, and `branch:` terms filter both branches and waiting reviews; `sort:recent` (default) and `sort:name` order the branch list, while waiting reviews are always listed newest first. Typing shows clickable completions for keys and values.
- **Branches**: lists each active Git worktree (`project / branch`) or non-Git project folder with Runs on this machine, plus the project the app was started for with `--repo` or `graph --repo` even before its first Run. Click the star beside a branch to pin it to the top. Click a branch name to expand its timeline and open its newest entry, or click its chevron to expand or fold without changing the Main area: the first timeline row is **Current** (replaced by an active Run while one is in progress), followed by saved Runs newest first. Click **Current** or any saved Run to show it in the Main area, or click **Older runs** at the bottom of the timeline to load more entries.
- **Waiting reviews**: stacked at the bottom of the Pane across all repositories (`(0)` when empty). Clicking an entry opens the Human review in the Main area.
- **Resizing**: drag the divider between the Pane and the Main area, or drag the Detail panel's left edge, to resize them; both widths persist across views and restarts.

## Main area: Run, Current, Graphs, and Detail {#current}

- **Run view**: shows the Run's ID and status headline (click the headline or run **Open the Run's Detail** from **Commands** for the Run's Detail: **Outcome**, **Waits for**, **What**, **Provenance**, and **Technical**) above the graph tool strip and interactive Artifact Graph.
- **Current view and editing**: scans the worktree like `artifactize status` and colors each Artifact as its scan finishes (`reused`, `would run`, `waiting`, `blocked`), with a **Refresh** button at the end of the header.
  - **Creating and editing definitions**: click **New Artifact** in the tool strip, enter a **Name** and **Path**, and click **Create** to write `.artifactize/<name>.artf`. In an Artifact's Detail tab, **Definition** edits `Path` (press `Enter` to write), `Parts` (click `+ Add`, pick a suggestion or press `Enter` to add; click `✕` to remove), `Comment` (opens the inline comment field under the node), **Rename**, and **Delete**; **Source** edits the full `.artf` file with **Save** and **Revert** buttons. If a file changed on disk since Current scanned it, the edit is refused without overwriting and Current rescans.
  - **Placing nodes, parts, and comments**: drag a node, a minimized whole, or a whole's **title row** to position it in `[annotations.style]` (dragging a node never auto-pans the camera; dragging a whole's inner face or the background pans the graph). Dropping a node on empty space inside its own whole's box repositions it; dropping it inside a different whole's box or onto another node makes that target its new whole in `parts` (removing it from its previous whole); dropping it outside every box removes its part membership. Use **Reset layout** in a box's title row or the tool strip to return to automatic layout. Click a node's comment to edit `[annotations.comment]` (`Enter` saves, `Shift+Enter` adds a line), or drag the comment onto another Artifact without a comment to move it between files.
- **Graph navigation**: use the tool strip above the graph for **Find** (jumps to any Artifact, eval, or starred favorite), **Zoom out**, **Zoom in**, and **Fit View**; drag the background to pan and scroll to zoom. Click a node to open its **Detail** docked on the right; click a whole's **Minimize**/**Expand** button (or **Minimize or expand the whole** in **Commands**) to collapse or reopen its box; use the bottom-right **Navigator** minimap when part of the graph is off-screen. On large graphs, zooming out until names are too small to read draws status-colored blocks that still open **Detail** when clicked.
- **Detail tabs**:
  - **Artifact tab**: shows collapsible sections for **Evals**, **Neighbours** (inputs on left, consumers on right), declared Human **Tools** (runnable immediately in the worktree with a live **Tool log**), **Path** (or **Definition** and **Source** on Current), and **Debug**. Click the star beside the title to favorite the Artifact across all branches of the project.
  - **Eval tab**: click an eval row or chip (`[icon] {name} / [eval]`) to open that eval's tab. A Human eval with a recorded request in the Run opens its **Human review** directly in the tab; other evals show **Summary** first and, when a log exists, a plain-text toggle to **Agent log** (live or saved transcript) or **Runtime log** (saved `stdout`/`stderr`).
- **Human review view**: click **Claim to review**, run any declared **Tools** and inspect the live **Tool log**, then click **GREEN** or **RED**, fill the schema-backed form, and click **Submit**. Click **Release claim** to release a claim without submitting.
- **Settings**: click **Settings** in the header to manage **Providers** (sign in/out of `codex` or `claude-agent`, save/remove API keys for `openai`/`anthropic`/`gemini`, add custom alias or endpoint providers, and click **List models**), **Remote** (sign in/out of the review store, **Check status**, dry-run or push results), and **Limits** (`signoffIdleGraceSeconds`).
