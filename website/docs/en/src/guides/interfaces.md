# Desktop App

The desktop app (`artifactize-desktop`) uses the same local state as the CLI and MCP server. Runs, review changes, and worktrees update without a reload. The bottom bar stays in place: the brand opens the command palette, **Chat**, **Verify**, and **Review** switch workspaces, and **Settings** opens settings. Switching modes preserves each workspace's selection, scroll position, open panes, and text. **Review** shows how many Human reviews are waiting, regardless of the current filter.

## Opening the app {#open-the-app}

```sh
artifactize-desktop                                       # open Verify with all projects listed
artifactize-desktop monitor --repo PATH                   # open Verify on PATH's branch
artifactize-desktop review [REQUEST_ID] [--reviewer NAME] # open Review on the newest waiting Human review or the given request
artifactize-desktop graph --repo PATH                     # open PATH's Current in Verify
artifactize monitor [PATH | --repo PATH | --all]          # CLI shortcut for Verify
artifactize review [REQUEST_ID] [--repo PATH | --all] [--reviewer NAME]  # CLI shortcut for Review
```

`artifactize-desktop` also accepts `--state-dir PATH`, `--all`, `--repo PATH` (mutually exclusive with `--all`), and `--reviewer NAME`.

Click the brand or press `Ctrl+K` to open the command palette. Use **Show or hide the Pane** (`Ctrl+B`) to toggle the Pane, or **Open a request by ID** to jump directly to a request. **Keyboard shortcuts** lists the available keys. The palette offers Verify commands only while Verify is open. You can drag the divider to resize a Pane and drag the left edge of **Detail** to adjust its width. If you quit while still holding claims this app took, choose **Stay** or **Release and quit**. The app releases only those claims. If you release a claim and take it again from the CLI, quitting the app leaves the new claim in place. Releasing a claim doesn't discard your draft. Quitting also stops a running Chat turn.

## Chat: work with an agent {#chat}

Artifactize owns the Chat session, tools and transcript. To use your Google account in Chat, [connect Antigravity](../getting-started/install.md#antigravity-for-chat-only). A session belongs to the branch you started it on and uses the environment you'd get in a terminal in that worktree. If you check out another branch there, Chat tells you why you can't send messages or use tools. Check out the original branch in that worktree to resume. Every model uses the same tools: read, list, find, search, edit and write files, run commands, and use Verify tools to start, observe, or cancel Runs and inspect declarations and saved results. Chat doesn't ask you to approve each tool call.

1. Click **New chat** and choose a live branch. **Model** shows the model from that branch's last session. If there is no previous session, it shows the first model a connected provider lists once all providers have answered. Until the model is known, the line says **Reading the providers' models…** and **Create** stays disabled. If no provider lists a model after loading, the dialog tells you to connect a provider in **Settings**. Click **Create** once a model is shown. In the Pane, **New chat** sits above the sessions, which are grouped by project and branch, newest first. The first message gives a session its title. The session and its transcript stay available after you restart the app.
2. The selector at the bottom left of the composer shows the current model. Open it to choose a model from the providers listed there. If needed, choose **Connect provider** to open **Settings → Providers**. You can change the session's model for later turns without changing how Chat works.
3. Write a message and click **Send** or press Enter; Shift+Enter starts a new line. The response appears as the turn runs. Each tool call gets one line describing the action and target, or why it failed. **Stop** or Esc in the composer interrupts the turn and its running tool without deleting the conversation or cancelling a Run that Verify has already accepted. Text typed during a turn stays in the composer; use **Send** when the turn ends. A stopped or failed turn also leaves the composer usable.
4. Ask the agent to verify its work. Each accepted Run appears separately in the conversation with its Run ID, current status, and what failed or needs a person. Click **Open in Verify** on the Run you want. In Verify, the first item in the Run header is **Chat · {session title}**. Click it to open the session at that Run card.

## Verify: branches and timelines {#pane}

- **Search**: text matches projects and branches. Use `repo:` or `branch:` to match just one of those fields, and `sort:recent` (the default) or `sort:name` to order the list. The field suggests keys and values as you type.
- **Branches**: Verify lists live branches and non-Git folders for projects with saved Runs, plus the project you open with `--repo`. Click a branch to expand it and show its newest entry. Its chevron expands or folds the timeline without opening an entry. The timeline shows **Current** or the active Run first, followed by saved Runs newest first; **Older runs** loads more. Star a branch to pin it above unstarred branches.

## Verify: Run, Current, and Detail {#current}

- **Run**: see the selected Run's status, evals that failed or are waiting, and the graph of its Artifacts. Click the Run headline for details, including what started it: the CLI, an MCP client, or a Chat session.
- **Current**: see the worktree's current declarations and what would happen on the next verify, much like `artifactize status`. **Refresh** scans again. Current is also where you edit definitions.
- **Graph and Detail**: use **Find** to jump to an Artifact, eval, or starred favorite. Drag the background to pan, scroll to zoom, and use **Fit View** to bring the graph into view. The **Navigator** appears when part of the graph is off-screen. Click a node to open **Detail** on the right. Click an eval to see its summary and any Agent transcript or Runtime log. The Artifact's own tab shows its evals, neighbors, and Human tools. You can run those tools in the current worktree without claiming a review.
- **Human review links**: from a Run's Artifact Detail, use **Claim to review** or **Release claim** for its waiting Human eval, or **Open in Review** for the full review. It's the same claim and draft in both places.

### Edit definitions in Current {#edit-definitions-in-current}

Click **New Artifact**, enter **Name** and **Path**, and click **Create**. In the Artifact's Detail, **Definition** lets you edit **Path** and **Parts**, add a comment, rename the Artifact, or delete it. For a direct `.artf` edit, open **Source** and use **Save** or **Revert**. If the file changed elsewhere after you opened it, Artifactize refuses to overwrite it and refreshes Current.

Drag a node or a whole's title row to place it. Drop a node into another whole to add it to that whole's `parts`, or outside every whole to remove it from its current one. See [composition](../concepts/artifacts.md#composition) for what `parts` mean. **Reset layout** restores automatic placement. Click a node's comment to edit it or drag the comment onto an Artifact without a comment to move it.

## Review: find and complete a Human review {#review}

The default filter in **Review** is `type:human waiting`. Change the plain-text query to look for another request. All terms must match: use `type:human` or `type:agent` for the evaluation kind, `waiting` for requests waiting for a result, `repo:` or `branch:` to match the displayed project or branch name, and free text for the eval, project, or branch. The filter suggests keys and values as you type. Reviews are grouped by project, newest first, with one line per request.

A reference opens the exact request, even if your filter doesn't list it. For example, **Open in Review** can show a review you already claimed without changing the filter. Review tells you when the open request is outside the filter. Click **Verify · {Run} › {Artifact}**, the first item in the review header, to return to that Run and eval. If the request shares another request's sign-off, both its ID and that link stay the same when you claim or reload it.

1. Click **Claim to review**. Read the instruction and use the **Tools** to inspect the Artifacts. Check the **Tool log** for any output or errors. Viewer tools open an [Artifact Viewer](viewer-plugins.md) window.
2. Write **Notes** and choose **GREEN** or **RED**. Fill in the fields the eval asks for and click **Submit**. If a field is invalid, correct it and try again; the draft stays open. Click **Release claim** to give up the claim without submitting.

In a wide window, Review puts the instruction, tools, and tool log on the left and **Evaluation** on the right. The draft belongs to the review and reviewer. It keeps Notes and the values you enter in both verdict forms. No verdict is selected until you choose one. Reopen a claimed review to see the chosen form with its saved values, even after restarting the app. Releasing the claim keeps the draft. Submitting clears it and saves the Notes with the result.

### Review while using a viewer {#review-while-using-a-viewer}

While you hold the active review, a small pet overlay appears at the bottom right when another mode is shown or the main window loses focus. The overlay has the same draft: verdict, **Notes**, form, and **Submit**. Click its header link to reopen that review in the main window. The overlay hides when you return to the evaluation space, release the claim, or submit.

Use [**Point**](viewer-plugins.md#point-into-the-active-review) in a supported viewer to append a location to the active review's **Notes**. Review and the pet overlay highlight the added text until you next edit the Notes. The overlay's **Point** is available when a viewer that supports it is open for the review's Artifact. Point never submits.

## Settings {#settings}

- **Providers** connects built-in or custom providers and lists their available models. Chat uses the selected provider and model; Agent evals declare theirs in their profile.
- **Remote** signs in to the [team review store](team-and-ci.md), checks status, previews a push, or pushes results.
- **Limits** sets the sign-off idle grace for future Runs. The Settings header shows your reviewer name and, when a newer stable release is available, a notice that opens update instructions. Artifactize doesn't download or install it for you; Microsoft Store installations update through the Store.
