# Desktop App

The desktop app (`artifactize-desktop`) uses the same local state as the CLI and MCP server. Runs, review changes, and worktrees update without a reload. The bottom bar stays in place: the brand opens the command palette, **Chat**, **Verify**, and **Review** switch workspaces, and **Settings** opens settings. Switching modes preserves each workspace's selection, scroll position, open panes, and text. **Review** shows how many unclaimed Human reviews are waiting, regardless of the current filter.

## Opening the app {#open-the-app}

```sh
artifactize-desktop                                       # open Verify with all projects listed
artifactize-desktop monitor --repo PATH                   # open Verify on PATH's branch
artifactize-desktop review [REQUEST_ID] [--reviewer NAME] # open Review on the newest waiting Human review or the given request
artifactize-desktop graph --repo PATH                     # open PATH's Current in Chat's Graph view
artifactize monitor [PATH | --repo PATH | --all]          # CLI shortcut for Verify
artifactize review [REQUEST_ID] [--repo PATH | --all] [--reviewer NAME]  # CLI shortcut for Review
```

`artifactize-desktop` also accepts `--state-dir PATH`, `--all`, `--repo PATH` (mutually exclusive with `--all`), and `--reviewer NAME`.

`graph --repo PATH` opens Chat on the newest session of PATH's branch that can still be worked on, in its **Graph** view. If none can, it opens the newest saved session, which says why. If the branch has no session yet, it opens **New chat** on that branch.

Click the brand or press `Ctrl+K` to open the command palette. Use **Show or hide the Pane** (`Ctrl+B`) to toggle the Pane, or **Open a request by ID** to jump directly to a request. **Keyboard shortcuts** lists the available keys. The palette offers Verify commands only while Verify is open. You can drag the divider to resize a Pane and drag the left edge of **Detail** to adjust its width. If you quit while still holding claims this app took, choose **Stay** or **Release and quit**. The app releases only those claims. If you release a claim and take it again from the CLI, quitting the app leaves the new claim in place. Releasing a claim doesn't discard your draft. Quitting also stops a running Chat turn.

## Chat: work with an agent {#chat}

Artifactize owns the Chat session, tools and transcript. To use your Google account in Chat, [connect Antigravity](../getting-started/install.md#antigravity-for-chat-only). A session belongs to the branch you started it on and uses the environment you'd get in a terminal in that worktree. If you check out another branch there, Chat tells you why you can't send messages or use tools. Check out the original branch in that worktree to resume. Every model uses the same tools: read, list, find, search, edit and write files, run commands, and use Verify tools to start, observe, or cancel Runs and inspect declarations and saved results. Chat doesn't ask you to approve each tool call.

1. Click **New chat** and choose a live branch. **Model** preselects the last model used on that branch if it is still available, or the first model a connected provider lists. You can click **Create** without a model and open the **Graph** view to work with Current. To send a message, first choose a model in the composer. In the Pane, **New chat** sits above the sessions, which are grouped by project and branch, newest first. The first message gives a session its title. The session and its transcript stay available after you restart the app.
2. The selector at the bottom left of the composer shows the current model. Open it to choose a model from the providers listed there. If needed, choose **Connect provider** to open **Settings → Providers**. You can change the session's model for later turns without changing how Chat works.
3. Once a model is selected, write a message and click **Send** or press Enter; Shift+Enter starts a new line. The response appears as the turn runs. Each tool call gets one line describing the action and target, or why it failed. **Stop** or Esc in the composer interrupts the turn and its running tool without deleting the conversation or cancelling a Run that Verify has already accepted. Text typed during a turn stays in the composer; use **Send** when the turn ends. A stopped or failed turn also leaves the composer usable.
4. Switch the session to **Graph** to see its branch's Current: the worktree's current declarations and what would happen on the next verify, much like `artifactize status`. **Refresh** scans again, and Current is where you [read definitions](#read-definitions-in-current). The composer floats over the graph, so you can write to the agent while you look at it.
5. Ask the agent to verify its work. Each accepted Run appears separately in the conversation with its Run ID, current status, and what failed or needs a person. Click **Open in Verify** on the Run you want. In Verify, the first item in the Run header is **Chat · {session title}**. Click it to open the session at that Run card.

### Read definitions in Current {#read-definitions-in-current}

Current draws the project as its [`.artf` code](../reference/declarations.md) declares it, laid out automatically. It reads the code and never changes it: change Artifacts, parts and inputs by editing the files, in Chat or anywhere else, and **Refresh** to see the result.

In an Artifact's Detail, **Definition** shows where it is bound (its file and line) and, for an Artifact a macro made, the macro it calls; its **Path** (a missing target is said in red beside it); its **Parts**, each opening that part's Detail; and its **Tools**, the toolsets it composes. Unfold **Source** to read the code that binds it, with its line numbers. For an Artifact a macro made, **Authored** shows the call as written and **Expanded** the declaration it expands to, every parameter filled in, as `artifactize config expand` prints it.

## Verify: branches and timelines {#pane}

- **Search**: text matches projects and branches. Use `repo:` or `branch:` to match just one of those fields, and `sort:recent` (the default) or `sort:name` to order the list. The field suggests keys and values as you type.
- **Branches**: Verify lists live branches and non-Git folders for projects with saved Runs, plus the project you open with `--repo`. Click a branch to expand it and show its newest Run. Its chevron expands or folds the timeline without opening a Run. The timeline lists Runs only: the active Run first, followed by saved Runs newest first; **Older runs** loads more. A branch without Runs says so and offers **Open in Chat**, which opens the branch's latest Chat session in **Graph** view to show Current—or opens **New chat** on that branch if it has no session yet (create a session and switch to **Graph** to see Current). Star a branch to pin it above unstarred branches.

## Verify: Run and Detail {#current}

- **Run**: see the selected Run's graph of its Artifacts and their verdicts. **Run details** shows its outcome, the evals that failed or are waiting, and what started it: the CLI, an MCP client, or a Chat session. **Open in Chat** opens the branch's latest Chat session in **Graph** view to show Current, or opens **New chat** on that branch if it has no session yet.
- **Graph and Detail**: use **Find** to jump to an Artifact, eval, or starred favorite. Drag the background to pan, scroll to zoom, and use **Fit View** to bring the graph into view. The **Navigator** appears when part of the graph is off-screen. Click a node to open **Detail** on the right. Click an eval to see its summary and any Agent transcript or Runtime log. The Artifact's own tab shows its evals, neighbors, and Human tools. You can run those tools in the current worktree without claiming a review.
- **Human review links**: from a Run's Artifact Detail, use **Claim to review** or **Release claim** for its waiting Human eval, or **Open in Review** for the full review. It's the same claim and draft in both places.

## Review: find and complete a Human review {#review}

The default filter in **Review** is `type:human waiting`. Change the plain-text query to look for another request. All terms must match: use `type:human`, `type:agent`, or `type:runtime` for the evaluation kind, `waiting` for Human reviews waiting for a person, claimed or not (`is:unclaimed`, `is:claimed` and `is:mine` narrow them by claim), `repo:` or `branch:` to match the displayed project or branch name, and free text for the eval, project, or branch. A claimed review stays in the waiting count and in `waiting`, naming its claimant, until it is submitted. The filter suggests keys and values as you type. Reviews are grouped by project, newest first, with one line per request.

A reference opens the exact request without changing the filter, even if your filter doesn't list it—for example, **Open in Review** can show a review you already claimed. When you open a review from Verify via **Open in Review**, the review header starts with **Verify · {Run} › {Artifact}** to return to that Run and eval. Reviews opened from the list, launch, or by ID don't show that back link, though every review header has **Open in Verify** to jump to its Run and eval. If the request shares another request's sign-off, both its ID and its Verify link stay the same when you claim or reload it.

1. Click **Claim to review**. Read the instruction, then use the **Tools** directly under it to inspect the Artifacts. Each tool execution appears as its own row below the request; click a row to expand or collapse that execution's output. Viewer tools open an [Artifact Viewer](viewer-plugins.md) window, where **Point** adds a place to the review's **Points**.
2. Write each thing the Artifact must change as an issue: type it in the issues box at the bottom and press Enter (Shift+Enter starts a new line) or click **Add issue**; **Insert** on a point inserts its text into the issue field at the caret. Added issues appear above the box, where you can edit or remove them. In the decision bar below the box, choose **GREEN** or **RED**: **GREEN** approves the Artifact as it stands and takes no issues, and **RED** requires at least one. Click **Submit** (any text still typed in the issues box must be added first); Submit says why it waits. If the submission is rejected, correct the draft and try again; the draft stays open. Click **Release claim** to give up the claim without submitting.

A review is one column: the instruction, its **Tools**, and a folded **Debug**, followed by tool runs, **Points**, and your added issues, with the issues box and decision bar anchored below. The draft belongs to the review and reviewer. It keeps your points, added issues and chosen verdict. No verdict is selected until you choose one. Reopen a claimed review to see your saved draft, even after restarting the app. Releasing the claim keeps the draft. Submitting saves the chosen verdict with its issues, then clears the draft, points included.

### Review while using a viewer {#review-while-using-a-viewer}

While you hold the active review, a small pet overlay appears at the bottom right when another mode is shown or the main window loses focus. The overlay shares the same draft: it shows **Points** (with **Insert**), your added issues, a compact issue field with **Add** and **Point**, the **GREEN** / **RED** choice, and **Submit**. Click its header link to reopen that review in the main window. The overlay hides when you return to the evaluation space, release the claim, or submit.

Use [**Point**](viewer-plugins.md#point-into-the-active-review) in a supported viewer to add a location to the active review's **Points**, with where and when you pointed. Review and the pet overlay show the same points; remove one in Review if you no longer need it. The overlay's **Point** is available when a viewer that supports it is open for the review's Artifact. Point never submits.

## Settings {#settings}

- **Providers** connects built-in or custom providers and lists their available models. Chat uses the selected provider and model; Agent evals declare theirs in their profile.
- **Remote** signs in to the [team review store](team-and-ci.md), checks status, previews a push, or pushes results.
- **Limits** sets the sign-off idle grace for future Runs. The Settings header shows your reviewer name and, except on Microsoft Store installations (which update through the Store), a notice when a newer stable release is available that opens update instructions. Artifactize doesn't download or install updates for you.
