# Declarations (.artf) Reference

A project is a directory containing `.artifactize/`. See [Artifacts and the Graph](../concepts/artifacts.md) for how `.artifactize/<name>.artf` files form the project graph.

Declaration keys use `snake_case` (`timeout_ms`, `pass_schema`, `input_schema`, `execution_paths`); JSON Schema keywords (`minLength`, `additionalProperties`) and declaration fields serialized in JSON (`passSchema`, `timeoutMs`, `agentTools`) use `camelCase`, while other CLI JSON outputs may use command-specific field names such as `signed_in` or `display_name`.

## Top-level Artifact fields {#declaration-validation}

```toml
# .artifactize/app.artf
path = "crates/app/"
select = ["src", "Cargo.toml"]           # optional: makes a folder target an aspect
parts = ["app-ui", "app-db"]             # optional: explicit composition
basis = false                            # optional: true only when the Artifact owns no evals
tags = ["type:code", "tier:core"]        # optional: display metadata
fingerprint = { files = ["src"], ignore = ["*.snap"] }  # optional: default is artifactsum

[annotations.style]
at = [120, 40]
parts = { app-ui = [40, 80] }

[annotations.comment]
text = "Canvas note shown in the desktop app."
```

| Field / identifier | Rules |
|---|---|
| Identifiers | Artifact names come from the `.artf` filename stem and must be case-insensitively unique across `.artifactize/` (avoid Windows device names such as `con` or `nul`); eval IDs are unique within their Artifact, and tool/session/variant IDs within their table. |
| `path` | **Required.** Project-root-relative local path using `/` (`"dir/"` or `"./"` for a folder—trailing `/` required—or `"file.ext"` for a single regular file), or a Live URI (for example `"https://example.com"`; unlike the HTTP(S)-only built-in `open` tool, Live targets accept other URI schemes as well). |
| `type` | Optional [Artifact type](../guides/viewer-plugins.md). Use a built-in or installed type, or omit it to declare a Custom Artifact. |
| `select` | Optional array of relative paths inside a folder `path`, defining an **aspect**. Not allowed on file or Live Artifacts. |
| `parts` | Optional array of Artifact names composing this whole. Cannot contain self-references or cycles. |
| `basis` | Optional boolean (`true` waives only this Artifact's own reviews; prohibited when `evals` is non-empty). |
| `tags` | Optional array of nonblank display strings (shown in `status` and `config graph`). |
| `fingerprint` | Optional:<br>• **Folder**: omit or `{}` for default `artifactsum`, `{ files? = [...], ignore? = [...] }` (`files` defaults to `["."]` and `ignore` defaults to `[]`), `{ script = { command = "...", args = [...], files? = [...], timeout_ms? = N } }`, or `false`.<br>• **Aspect**: omit for default `artifactsum` over `select`, `{ files = [...], ignore? = [...] }` (`files` required and within `select`; `fingerprint = {}` is rejected on an aspect), `{ script = ... }`, or `false`.<br>• **File**: omit or `{ files = ["<target-filename>"] }` (no `ignore`), `{ script = ... }`, or `false`.<br>• **Live**: omit (no snapshot), `{ script = ... }`, or `false`.<br>See [Fingerprints and Reuse](../concepts/reuse.md#fingerprints-artifactsum-script-false). |
| `views.agent_tools` | Map of `<tool_id>` to `{ builtin = "...", args? = [...], description? = "..." }` (`args` is required for `graph`, `help`, and `section`, and optional for fixed-target `read`/`list`; see [Built-in tools](../guides/tools.md#builtin-tools)) or `{ description, protocol = "json"\|"plain", command, args, input_schema?, execution_paths?, timeout_ms?, session? }`. |
| `views.agent_sessions` | Map of `<session_id>` to `{ start = { command, args, execution_paths? }, stop = { command, args, execution_paths? } }` (`start` and `stop` do not take `timeout_ms`; bound tools declare their own `timeout_ms`). |
| `views.human_tools` | Map of `<tool_id>` to `{ builtin = "...", args = [...], description?, kind? }` or `{ description, kind = "launch"\|"output", command, args, timeout_ms? }` (`timeout_ms` bounds `output` commands and does not apply to `launch`). For a type with a viewer, `{ kind = "viewer", description = "..." }` opens its Artifact Viewer. |
| `annotations` | Optional presentation metadata (`style.at`, `style.parts`, `comment.text`). Ignored by `artifactsum`'s normalized `.artf` view and by Eval definition hashes. |

## Eval fields (`[evals.<id>]`) {#eval-fields-evalsid}

Each `[evals.<id>]` table defines `<artifact>/<id>`:

| Field | Type & rules |
|---|---|
| `title` | **Required.** Nonblank summary of what passes. |
| `profile` | **Required.** Reviewer profile table (`kind = "runtime"`, `"agent"`, or `"human"`):<br>• `runtime`: `{ kind = "runtime", command = "...", args = [...], timeout_ms? = N }`<br>• `agent`: `{ kind = "agent", backend = "...", model = "...", reasoning? = "...", timeout_ms? = N, max_tokens? = N, max_tool_calls? = N }`<br>• `human`: `{ kind = "human" }` |
| `profile_variants` | Optional map of named alternative complete profiles of the same `kind` (selected via `verify --profile NAME` or `status --profile NAME`, which applies to every executable eval in the initial selection and recursion policy before applying variants; dependencies newly introduced only by a runtime variant's `args` keep their base profiles). |
| `mutex` | Optional array of hierarchical lock tags on `runtime` or `agent` evals (`"use-computer"` or `"use-computer:<segment>..."`, such as `"use-computer:display"`). Two evals conflict if their tags are equal or one is an ancestor of the other (`use-computer` conflicts with all descendants); conflicting evals queue rather than running concurrently. |
| `payload.instruction` | **Required.** Nonblank criteria string. `{name}` references to other Artifacts in `payload.instruction` or runtime `args` declare input Artifact dependencies (referencing the eval's own target Artifact adds no dependency). |
| `red_at` | Optional priority threshold on `agent` evals only (`"P0"`, `"P1"`, `"P2"`, or `"P3"`, where `P0` is highest). Any finding at `red_at` or higher makes the eval `RED`; omitting `red_at` makes any finding count. Findings below `red_at` are kept even when the eval is `GREEN`. |
| `pass_schema`, `fail_schema` | Optional JSON Schema for owner result fields on `human` evals (rejected on `agent` evals; accepted on `runtime` declarations too, where they participate in the Eval definition hash without changing the exit-code verdict). |

## Owner-result schemas (`pass_schema` and `fail_schema`) {#owner-result-schemas}

Human results contain `"verdict": "GREEN" | "RED"` plus your declared owner fields.

- **Envelope rules**: root `type = "object"` and `additionalProperties = false` are supplied automatically when omitted. Every allowed top-level owner field must appear explicitly in `properties` (neither `$ref` nor `patternProperties` can admit undeclared top-level fields, and schema composition keywords such as `allOf`/`anyOf`/`oneOf`/`not` are rejected at both the root and inside Human properties). Reserved audit field names (`verdict`, `reference`, `reusedFrom`, `executionProvenance`, `attemptId`, `provider`, `model`, `stdout`, `stderr`, `durationMs`, `exitCode`) cannot be declared. Omitting a verdict's schema allows no owner fields (`{}`) for that verdict.
- **Human form shapes** (`#human-result-schemas`): because a person fills a Human schema in a UI form, `pass_schema` and `fail_schema` on a `human` eval allow only these form-enterable property shapes:
  - `{ type = "string", minLength?, maxLength? }` (text box)
  - `{ type = "integer" | "number", minimum?, maximum?, exclusiveMinimum?, exclusiveMaximum?, multipleOf? }` (number box)
  - `{ type = "boolean" }` (true/false selector)
  - `{ enum = ["a", "b"] }` (string choice)
  - `{ const = ... }` (auto-filled constant)
  - `{ type = "array", items = { type = "string" } }` (list of text boxes)
  - `{ type = "array", items = { type = "object", properties = { <scalars above> } } }` (repeated field groups)

## Migrating and renaming definitions {#upgrading-an-in-tree-project}

- **`artifactize config migrate [--repo PATH]`**: converts legacy in-tree `index.artf`, sidecar `.artf`, and root `.artf` files into `.artifactize/<name>.artf` files with explicit `path` and `parts`, removes legacy `pass_schema` and `fail_schema` declarations from `agent` evals while preserving Human schemas, uses `.artfignore` to identify and skip excluded nested projects, and removes the converted legacy files and `.artfignore`. For an already migrated project, it can also add `type = "files"` to local Artifacts that lack a required review tool. If a custom tool would conflict with the plugin, add the missing tool yourself.
- **`artifactize config rename OLD NEW`**: validates the project and updates `.artifactize/**/<OLD>.artf` to `<NEW>.artf` along with all `parts`, recognized `{OLD}` references, and saved `[annotations.style]` part positions.

## Operating system rules {#operating-systems}

Write declarations once using `/` for logical paths; Artifactize runs them across Linux, macOS, Windows, and WSL 2:

- **Program lookup**: bare executable names (`python3`, `cargo`) resolve on `PATH` (using `PATHEXT` on Windows, so `npx` finds `npx.cmd`). Relative executables with `/` (`./tools/check`) resolve inside the Artifact's admitted scope for folders and aspects, beneath the containing folder for file Artifacts, and beneath the project root for Live targets (always rejecting `..` and symlinks).
- **Path case sensitivity**: logical paths are matched case-sensitively on every OS; on macOS and Windows case-insensitive volumes, Artifactize also verifies that the entry's exact on-disk spelling matches.
- **Opening files and URLs**: `artifactize login codex` opens the system browser to complete sign-in.
