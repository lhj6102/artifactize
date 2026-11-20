# Artifact Types and Viewer Plugins {#artifact-types-and-viewer-plugins}

Declare `type` on an Artifact to use a plugin's Agent Tools and Human Tools without repeating them. `path` still determines the target, and `select` still determines the files an aspect covers. An Artifact without `type` is a **Custom Artifact**: it declares its own tools in `[views.*]`.

```toml
# .artifactize/guide.artf
path = "guide.md"
type = "markdown"
```

The eval's target and every Artifact it names with `{name}` must have an **Agent Tool** for an Agent eval or a **Human Tool** for a Human eval. A `basis = true` input is no exception. Runtime evals don't need tools. Use `artifactize config check` to find missing tools before running verify; `artifactize config migrate` can add `type = "files"` when that fixes an existing local Artifact without replacing its custom tools.

An Artifact can declare extra tools alongside its type's tools, but it cannot redefine a plugin tool or session with the same name in the same table. The `type` field and tool declarations aren't part of the eval definition hash, for Custom and typed Artifacts alike. See [Tools and Harness Sessions](tools.md) for tool names and scoping.

## Built-in plugins {#built-in-plugins}

You don't have to install the built-ins. In the desktop app, run a viewer tool from the Artifact's **Tools** list or from a Human review that includes the Artifact. This opens a separate **Artifact Viewer** window in the app. Run the same viewer tool again for the same Artifact to bring that window to the front. Custom `launch` and `output` tools still launch their program or print to the **Tool log**.

| Type | Agent Tools | Human Tool |
|---|---|---|
| `files` | `list`, `glob`, `grep`, `read` | **File Explorer**: browse the Artifact's files and read a document. |
| `markdown` | `read` | **Markdown**: read Markdown in a dedicated viewer. |
| `artifactize` | `declarations` | **Artifactize Viewer**: inspect Artifact nodes, their references, and the evals inside each node. |

Use `type = "artifactize"` with a target like `.artifactize/front-design/` to review the declarations in that folder. Agent and Human reviewers see the same scope. The viewer shows what the project **declares**, including an eval's criteria and which Artifacts it reads. It doesn't show the results of a Run. The older `graph` builtin is still project-wide.

## Install a plugin in your project {#install-a-plugin}

```sh
artifactize plugin add PATH
artifactize plugin list
artifactize plugin remove TYPE
```

`PATH` is a folder containing `plugin.toml` and any files the package needs. Adding a plugin copies the folder into `.artifactize/plugins/<type>/`. Commit that folder with your declarations so another checkout can resolve their types. `list` includes built-in and project-installed types. Remove an installed type before adding its replacement. If a declaration still uses a removed type, `config check` reports it until you reinstall the plugin or edit the declaration. You can't remove or replace built-in types.

Only install plugins you trust. A viewer can read the Artifact through the bridge below, and declared Agent and Human command tools can run programs in your environment. The commands are also available over [MCP](../reference/cli-and-mcp.md#mcp-stdio-server).

## Write a plugin manifest {#write-a-plugin-manifest}

The [HTML example](https://github.com/lhj6102/artifactize/tree/main/examples/plugins/html) is an installable package. It reads the page as source for an Agent and opens a self-contained HTML page for a person. Set `type = "html"` on the `.artf` file after installing it.

```toml
# plugin.toml
type = "html"
title = "HTML Viewer"

[viewer]
entry = "viewer.html"

[views.agent_tools]
inspect = { builtin = "read", description = "Inspect the HTML source of {artifactName}." }

[views.human_tools]
view = { kind = "viewer", description = "Open {artifactName} in the HTML Viewer." }
```

`type` is the unique name your Artifact declares. `title` is the plugin name shown in the viewer window. The three tool tables, including `[views.agent_sessions]`, have the same format as [Artifact declarations](../reference/declarations.md#declaration-validation); commands and tool paths are resolved as tools of the Artifact, not of the plugin package. Omit `[viewer]` if you don't supply a viewer Human Tool. Otherwise, include at least one `kind = "viewer"` Human Tool and point `viewer.entry` at a self-contained HTML file inside the package. The desktop app hosts that entry; it doesn't run a web server or open your browser for it.

## Viewer bridge {#viewer-bridge}

The plugin page is sandboxed. It can reach only the `artifactize-viewer/v1` bridge, not the desktop app's commands. Ask for `context` when your page starts. The viewer's strip shows **Loading {plugin title}…** until it receives the first bridge request. If the page finishes loading without sending one, the strip says the plugin did not connect until a request arrives.

Send calls to the host with `parent.postMessage`:

```js
parent.postMessage({
  channel: 'artifactize-viewer/v1',
  id: 1,
  method: 'context',
  args: {},
}, '*');
```

Use a distinct `id` for each outstanding call. The reply from `parent` contains the same `channel` and `id`, and either `result` or an `error` string. Check the message's source and channel before using it. The HTML package shows how to match responses to pending calls.

| `method` | `args` | Result |
|---|---|---|
| `context` | `{}` | `{artifact, type, title, target, targetKind, review}`. `target` is the declared target. `review` is the active claimed review's ID or `null`. |
| `files` | `{path}` | Entries directly under that directory, each `{name, path, kind}` (`file` or `directory`). |
| `readText` | `{path}` | UTF-8 text of a file. For a single-file Artifact, pass `''` to read the target itself. |
| `status` | `{text}` | `null`. Shows one plain-text line in the viewer's strip, replacing the previous line. Closing the window clears it. |
| `pointing` | `{text}` | The review's updated Notes. Refused if there is no active review claimed by this reviewer. |

In bridge calls, `path` is relative to the Artifact, with `/` as the separator and `''` for its root. The bridge can't read siblings of a file Artifact, paths outside an aspect, or files belonging to a separate folder part. It doesn't follow symlinks. Use `status` to say what the page is showing or why it couldn't load. The HTML example waits for the Artifact page to load, then reports its path and document title, for example `checkout/index.html · Checkout`.

The host also sends events:

- `{channel: 'artifactize-viewer/v1', event: 'activeReviewChanged'}`: disarm any Point started under the previous review. Refresh `context` if your page displays review information.
- `{channel: 'artifactize-viewer/v1', event: 'point', armed: true}`: wait for the user to pick a place in your page, then call `pointing` once with that location. The call ends Point. If `armed` is `false`, stop waiting without calling `pointing`. Check the source and channel, just as for replies.

## Point into the active review {#point-into-the-active-review}

With an active Human review you have claimed, click **Point** and choose a place in the Artifact. The viewer's tool strip names the active review's eval and shows **Point** for viewers that support it. The location becomes text: `notes.md L42:C9` in File Explorer, `L42:C9` in Markdown, an Artifact name or eval ID such as `front-design/matches-layout` in the Artifactize Viewer, or a CSS selector such as `#checkout .total` in the HTML example. Click **Point** again or press Esc to cancel without adding anything. Artifactize appends it on a new line in the same **Notes** draft; it doesn't replace existing notes, choose a verdict, or submit the review. The viewer uses the active review, not a review ID picked by the plugin.
