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

Click the brand to find commands such as **Keyboard shortcuts**, **Show or hide the Pane**, and **Open a request by ID**. You can drag the divider to resize a Pane and drag the left edge of **Detail** to adjust its width. If you quit while holding Human claims, choose **Stay** or **Release and quit**. Releasing a claim doesn't discard your draft. Quitting also stops a running Chat turn.

## Chat: work with an agent {#chat}

Chat is Artifactize's own agent, not a launcher for another coding-agent harness. A session works in one branch's worktree with the environment you'd get in a terminal there. Every model uses the same tools: read, list, find, search, edit and write files, run commands, and use Verify tools to start, observe, or cancel Runs and inspect declarations and saved results. Chat doesn't ask you to approve each tool call.

1. Click **New chat**, pick a branch, and click **Create**. Chat lists sessions by project, newest first. The first message gives a new session its title; the session and its transcript stay available after restarting the app.
2. Choose a model at the bottom left of the composer. If needed, choose **Connect provider** to open **Settings → Providers**. You can change the session's model for later turns without changing how Chat works.
3. Write a message and click **Send**. Text and tool activity appear as the turn runs, including reasons for failed tools. **Stop** interrupts the turn and its running tool without deleting the conversation or cancelling a Run that Verify has already accepted. You can send another message after a stopped or failed turn.
4. Ask the agent to verify its work. Each accepted Run appears separately in the conversation with its Run ID, current status, and what failed or needs a person. Click **Open in Verify** on the Run you want. In Verify, click **Chat · {session title}** to return to that Run's reference in the conversation.

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

The default filter in **Review** is `type:human waiting`. Change the plain-text query to look for another request. All terms must match: use `type:human` or `type:agent` for the evaluation kind, `waiting` for requests waiting for a result, `repo:` or `branch:` for a specific field, and free text for the eval, project, or branch. Reviews are grouped by project, newest first.

A reference opens the exact request, even if your filter doesn't list it. For example, **Open in Review** can show a review you already claimed without changing the filter. Review tells you when the open request is outside the filter. Click **Verify · {Run} › {Artifact}** in the review header to return to its Run and eval in Verify.

1. Click **Claim to review**. Read the instruction and use the **Tools** to inspect the Artifacts. Check the **Tool log** for any output or errors. Viewer tools open an [Artifact Viewer](viewer-plugins.md) window.
2. Write **Notes** and choose **GREEN** or **RED**. Fill in the fields the eval asks for and click **Submit**. If a field is invalid, correct it and try again; the draft stays open. Click **Release claim** to give up the claim without submitting.

The draft belongs to the review and reviewer. It keeps Notes, the chosen verdict, and the fields for both verdict forms. It stays available across modes, windows, restarts, and release. Submitting clears the draft and keeps the Notes with the result.

### Review while using a viewer {#review-while-using-a-viewer}

When you leave the active claimed review's evaluation space, a small pet overlay appears at the bottom right. It stays with that review while you use a viewer, another mode, or another application. Write in the same **Notes**, choose a verdict, or submit from the overlay without switching back. Its review reference takes you back to Review. The overlay hides when you return to the evaluation space, release the claim, or submit.

Use [**Point**](viewer-plugins.md#point-into-the-active-review) in a supported viewer to append a location to the active review's Notes. From the overlay, **Point** uses the open viewer for the review's Artifact; it's unavailable if the needed viewer isn't open. Point never submits.

## Settings {#settings}

- **Providers** connects built-in or custom providers and lists their available models. Chat uses the selected provider and model; Agent evals declare theirs in their profile.
- **Remote** signs in to the [team review store](team-and-ci.md), checks status, previews a push, or pushes results.
- **Limits** sets the sign-off idle grace for future Runs. A newer stable release, when available, appears in the Settings header with update instructions. Artifactize doesn't download or install it for you; Microsoft Store installations update through the Store.
