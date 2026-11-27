# Declarations (.artf) Reference

A project is a directory containing `.artifactize/`. Every `.artf` file anywhere under `.artifactize/` is code in the `.artf` language, and together they form one program: what one file declares, every other file can use. `.artfprofiles` files beside them declare how Agent evals run ([Agent profiles](#project-settings)). See [Artifacts and the Graph](../concepts/artifacts.md) for how the Artifacts it declares form the project graph.

The language is small and declarative. It runs nothing: reading it only declares Artifacts with their evals and toolsets, and `artf!` **macros** write a repeated shape once. `artifactize config check` validates it; an error names the file, line and column, and, for code a macro made, the macro call too.

```rust
// .artifactize/profiles.artfprofiles
profile reviewer : agent = { backend = "codex", model = "gpt-6.2-sol", reasoning = "high" }
```

```rust
// .artifactize/engine.artf
// A Rust crate of the workspace: its folder, its code review and its tests.
artf! rust-crate(package, dir, extra_evals = {}) : files = {
  path = "crates/@{param dir}/"

  eval follows-style : reviewer = {
    title = "The crate follows the code checklist"
    red_at = "P1"
    instruction = """
      Read @{artf rules-code-style} and review @{artf self} against every item. Each item the
      code does not meet is an issue.
    """
  }

  // Run the crate's tests with the lockfile.
  eval tests : runtime(command = "cargo", args = ["test", "--locked", "-p", "@{param package}"], timeout_ms = 1800000) = {
    title = "The crate's tests pass"
    deps = [cargo-lock]
  }

  ...extra_evals
}

artf cargo-lock = { path = "Cargo.lock", basis = true }
artf rules-code-style : markdown = { path = "project-rules/DEV-CODING.md", basis = true }

artf core = rust-crate!(package = "app-core", dir = "core")
artf cli = rust-crate!(
  package = "app-cli",
  dir = "cli",
  extra_evals = {
    eval packages : runtime(command = "cargo", args = ["package", "--locked", "-p", "app-cli"]) = {
      title = "The CLI packages"
    }
  },
)
```

## Syntax {#syntax}

- **Comments** start with `//` and run to the end of the line. Layout and comments carry no meaning.
- **Strings** are `"…"` on one line, with JSON's escapes (`\"`, `\\`, `\n`, `\t`, `\uXXXX`, …), or `"""…"""` across lines: the line break after the opening quotes, a last line holding only the closing quotes' indentation, and the indentation every line shares are dropped, and a backslash at a line's end joins the next line. Inside a string, `@{…}` is a [slot](#slots); write `@@{` for a literal `@{`. Other braces (`{#id}`, `{"json": true}`, `${HOME}`) are ordinary text. `'''…'''` is a raw string: no escapes and no slots.
- **Values** are strings, integers, `true`/`false`, lists `[a, b]`, blocks `{ key = value }` (entries separated by line breaks or commas), names (an Artifact, a toolset, a profile, a parameter), calls `name(key = value, …)` and macro calls `name!(key = value, …)`. An optional field is left out rather than given an empty value.
- **Names** of Artifacts, toolsets, profiles and macros start with a letter and use letters, digits, `-` and `_`. They share one namespace across every file of the project: a name is declared once, as an Artifact, a toolset, a macro or a profile, and is unique ignoring case; avoid Windows device names such as `con` or `nul`. Eval IDs are unique within their Artifact.

## Declarations {#declarations}

| Declaration | What it declares |
|---|---|
| `artf NAME : TOOLSETS = { … }` | An Artifact with its evals; `: TOOLSETS` is optional. |
| `artf NAME = macro!(…)` | An Artifact made by an `artf!` macro. |
| `artf! macro(params) : TOOLSETS = { … }` | A macro that makes Artifacts. |
| `tool NAME = { … }` | A toolset; its name is its namespace. |
| `profile NAME : agent = …` | An Agent profile, in a `.artfprofiles` file ([Agent profiles](#project-settings)). |

## Artifact fields {#declaration-validation}

```rust
artf app : files & html = {
  path = "crates/app/"
  select = ["src", "Cargo.toml"]          // optional: makes a folder target an aspect
  parts = [app-ui, app-db]                // optional: explicit composition
  basis = false                           // optional: true only when the Artifact owns no evals
  tags = ["type:code", "tier:core"]       // optional: display metadata
  fingerprint = { files = ["src"], ignore = ["*.snap"] }  // optional: default is artifactsum

  eval review : reviewer = { … }
}
```

| Field | Rules |
|---|---|
| `path` | **Required.** Project-root-relative local path using `/` (`"dir/"` or `"./"` for a folder—trailing `/` required—or `"file.ext"` for a single regular file), or a Live URI (for example `"https://example.com"`; unlike the HTTP(S)-only built-in `open` tool, Live targets accept other URI schemes as well). |
| `: TOOLSETS` | The [toolsets](../guides/tools.md) it composes, joined with `&`: built-in ones (`files`, `markdown`, `artifactize`), an installed [plugin](../guides/viewer-plugins.md)'s, or the project's own `tool` declarations. Two composed tools with one qualified name must be identical. |
| `select` | Optional list of relative paths inside a folder `path`, defining an **aspect**. Not allowed on file or Live Artifacts. |
| `parts` | Optional list of Artifact names composing this whole. Cannot contain self-references or cycles. |
| `basis` | Optional boolean (`true` waives only this Artifact's own reviews; prohibited when it has evals). |
| `tags` | Optional list of nonblank display strings (shown in `status` and `config graph`). |
| `fingerprint` | Optional:<br>• **Folder**: omit for default `artifactsum`, `{ files? = [...], ignore? = [...] }` (`files` defaults to `["."]` and `ignore` to `[]`), `script(command = "...", args = [...], files? = [...], timeout_ms? = N)`, or `false`.<br>• **Aspect**: omit for default `artifactsum` over `select`, `{ files = [...], ignore? = [...] }` (`files` required and within `select`), `script(…)`, or `false`.<br>• **File**: omit or `{ files = ["<target-filename>"] }` (no `ignore`), `script(…)`, or `false`.<br>• **Live**: omit (no snapshot), `script(…)`, or `false`.<br>• **Definitions** (a target under `.artifactize/`): omit for the semantic definitions fingerprint.<br>See [Fingerprints and Reuse](../concepts/reuse.md#fingerprints-artifactsum-script-false). |
| `eval ID …` | Its evals, in the order written ([Eval fields](#eval-fields-evalsid)). `...block` spreads a block of `eval` declarations, such as a macro parameter's. |

## Eval fields {#eval-fields-evalsid}

`eval ID : EVALUATOR = { … }` inside an Artifact defines `<artifact>/<ID>`. The evaluator is one of:

- `runtime(command = "...", args = [...], timeout_ms? = N)`: a program whose exit code is the verdict.
- a profile's name: an Agent review, run as the [profile](#project-settings) says.
- `human`: a person's sign-off.

| Field | Rules |
|---|---|
| `title` | **Required.** Nonblank summary of what passes, written out (a macro parameter cannot fill it). |
| `instruction` | **Required** on Agent and Human evals, and not allowed on a runtime eval, whose verdict is its exit code: explain a runtime eval in a `//` comment. The criteria, written out, with `@{artf …}` [slots](#slots) naming the Artifacts they read. |
| `deps` | Runtime evals only: the Artifacts the command reads that its `args` do not name with `@{path …}`, such as a lockfile or a shared checker. Each is an input of the eval, as a slot is. |
| `red_at` | Optional priority threshold on Agent evals only (`"P0"`, `"P1"`, `"P2"`, or `"P3"`, where `P0` is highest). Any issue at `red_at` or higher makes the eval `RED`; omitting `red_at` makes any issue count. Issues below `red_at` are kept even when the eval is `GREEN`. |
| `mutex` | Optional list of hierarchical lock tags on runtime or Agent evals (`"use-computer"` or `"use-computer:<segment>..."`, such as `"use-computer:display"`). Two evals conflict if their tags are equal or one is an ancestor of the other (`use-computer` conflicts with all descendants); conflicting evals queue rather than running concurrently. |

## Slots {#slots}

A slot names something inside a string; Artifactize resolves it and records the relation it declares.

| Slot | Where | Becomes |
|---|---|---|
| `@{artf NAME}` | Agent and Human `instruction`, tool `description` (`@{artf self}` only) | The Artifact's name. Naming another Artifact makes it an input of the eval, and its tools join the review. |
| `@{path NAME}`, `@{path NAME/sub/path}` | runtime `args`, tool `args` | The absolute path (or Live URI) of the Artifact or a path inside it. A File Artifact's slot takes no `/sub/path`. Naming another Artifact makes it an input. |
| `@{param NAME}` | inside a macro, in `path` and arguments | The macro parameter's value as text. |

`self` is the Artifact being declared. In a toolset or a macro, `self` is bound late: it is the Artifact that composes the toolset or that the macro makes. A slot naming a missing Artifact or parameter is an error at its line.

## Toolsets {#toolsets}

```rust
tool review = {
  agent coverage = command(command = "python3", args = ["coverage.py", "@{path requirements/requirements.md}"], protocol = "json",
    description = "List the spec.md lines that cite each requirement.", input_schema = { type = "object" })
  agent section = builtin(op = "section", args = ["spec.md"], description = "Print one section of spec.md.")
  human notes = builtin(op = "read", args = ["@{path self/notes.md}"], description = "Print the review notes of @{artf self}.")
  human open = command(command = "open-notes", args = ["@{path self/notes.md}"], kind = "launch", description = "Open the notes.")
}
```

A toolset declares tools for two audiences, and sessions:

- `agent NAME = builtin(op = "...", args? = [...], description? = "...")` or `command(description, protocol = "json"|"plain", command, args, input_schema?, execution_paths?, timeout_ms?, session?)`: tools an Agent review calls. `args` is required for the `declarations`, `graph`, `help` and `section` builtins and optional for fixed-target `read`/`list`; see [Built-in tools](../guides/tools.md#builtin-tools).
- `human NAME = builtin(op = "...", args = [...], description?, kind?)` or `command(description, kind = "launch"|"output", command, args, timeout_ms?)` (`timeout_ms` bounds `output` commands and does not apply to `launch`), or, in a plugin with a viewer, `viewer(description = "...")`, which opens its Artifact Viewer in the desktop app.
- `session NAME = session(start = command(command, args, execution_paths?), stop = command(command, args, execution_paths?))`: a harness session that Agent tools bind with `session = "NAME"` (`start` and `stop` take no `timeout_ms`; bound tools declare their own).

A tool's qualified name is `NAMESPACE.TOOL` (`review.coverage`); on an Artifact, a reviewer calls it `NAMESPACE_TOOL_ARTIFACT` with dots as `_` (`review_coverage_spec`). An Artifact gets its tools by composing the toolset (`artf spec : files & review`); an Agent or Human eval reviews with the tools of its own Artifact and of every Artifact its instruction names. Compose a built-in toolset instead of declaring one that repeats it.

## Agent profiles {#project-settings}

Profiles say how Agent evals run. They are declared in `.artfprofiles` files under `.artifactize/`, never in `.artf` files, and each is one complete setting:

```rust
// .artifactize/profiles.artfprofiles
profile reviewer : agent = match TIER {
  default => { backend = "codex", model = "gpt-6-luna", reasoning = "max" },
  heavy => { backend = "codex", model = "gpt-6.2-sol", reasoning = "high" },
}
profile translator : agent = { backend = "codex", model = "gpt-6-luna" }
```

- **Fields**: a setting takes `backend` and `model`, and optionally `reasoning`, `timeout_ms`, `max_tool_calls` and `max_tokens`, as described in [Agent evals](../concepts/evals.md#agent-evals). Every branch is a complete setting: nothing is inherited from `default`.
- **Choosing by the environment**: `match VARIABLE { default => { … }, LABEL => { … } }` runs the branch named by the environment variable's value. `default` is required and runs while the variable is unset or empty, or names a label this profile does not have but another profile does. A value that no profile's `match` on that variable names is an error, so a misspelt value never runs silently as `default`. Every branch is checked, the ones the environment does not select included. A Run records the values it read in its saved graph.
- **Use**: an Agent eval names a profile (`eval review : reviewer = { … }`).
- **Reuse and fingerprints**: a profile is how an eval runs, not what it judges, so no profile or branch changes an Eval definition hash, reuse key or definitions fingerprint.

## Macros {#macros}

`artf!` declares a macro that makes Artifacts; `name!(…)` calls it where an Artifact is bound. A macro's parameters are names, each optionally with a default (`extra_evals = {}`); a call passes them by name, and a parameter without a default is required. Inside the macro, a parameter is a value where a value goes (`parts = parts`), `@{param NAME}` inside a `path` or an argument string, and `...NAME` spreads a block of evals into an Artifact.

A macro takes only what changes what is checked: a path, a package, a limit, extra evals. Titles, instructions and tool descriptions are written out in the macro, never filled from a parameter, so each reads the same wherever the macro is called.

Macros expand statically and never call themselves, directly or through another macro. `artifactize config expand ARTIFACT` prints the declaration a binding makes, every parameter filled in, and `artifactize config symbols` lists every global name and macro signature.

## Results {#owner-result-schemas}

Every eval's result has one shape: `{ verdict, kind, issues }`, with `kind` one of `runtime`, `agent` or `human`.

- **Agent**: each issue is `{ priority, reason }`; the verdict comes from `red_at`.
- **Human**: each issue is `{ reason }`, one thing the Artifact must change. A person answers `GREEN` with no issues, or `RED` with at least one.
- **Runtime**: `RED` keeps one issue `{ exitCode, reason }`, the end of what the command printed; the full output stays in the attempt's logs (`artifactize request logs REQUEST_ID`). Runtime results also carry `durationMs`.

## Migrating and renaming definitions {#upgrading-an-in-tree-project}

- **`artifactize config migrate [--repo PATH]`**: converts older definitions into the `.artf` language: in-tree `index.artf`, sidecar `.artf` and root `.artf` files, TOML `.artifactize/<name>.artf` files, and `.artifactize/settings.toml` profiles, which go to `profiles.artfprofiles`. Identical tool tables become one shared toolset, named apart from every Artifact, `type = "…"` becomes the composed toolset, `{name}` references become slots (a runtime eval's become `@{path …}` arguments or `deps`, and its instruction a `//` comment), comments are kept, and `pass_schema`/`fail_schema` are dropped with a note: a person now answers `GREEN`, or `RED` with issues. Saved results do not carry over; every eval runs once more under the new reuse keys.
- **`artifactize config rename OLD NEW`**: validates the project and renames the Artifact wherever the code names it: its binding, `parts` lists, slots and `deps`. A file named after the one Artifact it binds is renamed with it.

## Operating system rules {#operating-systems}

Write declarations once using `/` for logical paths; Artifactize runs them across Linux, macOS, Windows, and WSL 2:

- **Program lookup**: bare executable names (`python3`, `cargo`) resolve on `PATH` (using `PATHEXT` on Windows, so `npx` finds `npx.cmd`). Relative executables with `/` (`./tools/check`) resolve inside the Artifact's admitted scope for folders and aspects, beneath the containing folder for file Artifacts, and beneath the project root for Live targets (always rejecting `..` and symlinks).
- **Path case sensitivity**: logical paths are matched case-sensitively on every OS; on macOS and Windows case-insensitive volumes, Artifactize also verifies that the entry's exact on-disk spelling matches.
- **Opening files and URLs**: `artifactize login codex` opens the system browser to complete sign-in.
