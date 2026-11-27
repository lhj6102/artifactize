# Artifacts and the Graph

An **Artifact** is any reviewable piece of your project: a folder, a single file, selected paths inside a folder (an **aspect**), or observed external state (a **Live** URI). You declare each Artifact with an `artf` statement in a `.artf` file under `.artifactize/`, written in the [`.artf` language](../reference/declarations.md); its name is unique across the project. Files and subfolders inside `.artifactize/` only organize the code for people: one file can declare many Artifacts, and they create no namespace or relationship.

```rust
// .artifactize/guide.artf
artf guide = {
  path = "guide/"
  parts = [intro, usage]
  tags = ["type:docs"]

  eval terms : runtime(command = "python3", args = ["check_terms.py", "@{path glossary/terms.txt}", "@{path intro/page.md}", "@{path usage/page.md}"]) = {
    title = "Every bold term is defined in the glossary"
    instruction = "Check that every bold term in intro and usage is defined in the glossary."
  }
}
```

Run `artifactize config check` to validate declarations statically. Syntax errors exit 2 with the file, line, and column; a missing local target prints a note and exits 0 (becoming `ERROR/PREPARATION_FAILED` only on the evals that use it when `verify` runs).

## Target kinds (`path` and `select`) {#target-kinds-path-and-select}

Every local `path` is relative to the project root, while a **Live** target is a URI:

| Declaration | Kind | Target and execution base |
|---|---|---|
| `path = "src/"` (or `"./"`) | **Folder** | That directory relative to the project root. Commands run in that folder. |
| `path = "hero.png"` | **File** | That single regular file relative to the project root. Commands run in its containing folder, and `@{path name}` resolves to the file path (no `/suffix`). |
| `path = "src/"` + `select = ["api", "model.rs"]` | **Aspect** | Selected paths below that project-relative folder, so a cross-cutting slice can be reviewed and reused on its own. |
| `path = "https://example.com"` | **Live** | External state identified by URI. Artifactize never fetches it itself; it passes `@{path name}` (or `@{path name/suffix}`) to your declared read-only commands at the project root. File builtins and `artifactsum` do not apply, and omitting `fingerprint` leaves the Live target without a snapshot (no reuse key). |

The target kind is separate from the [toolsets](../guides/tools.md) the Artifact composes (`artf app : files & html`), which give it tools and, for a [plugin](../guides/viewer-plugins.md)'s, a viewer.

## The Artifact Graph and eval dependencies {#eval-dependencies}

The **Artifact Graph** connects inputs to the evals that consume them. Sharing a repository or directory never creates an implicit dependency. Instead:

- **Inputs**: naming `spec` in an eval makes `spec` an input of that specific eval: `@{artf spec}` or `@{path spec/…}` in an Agent or Human `instruction`, `@{path spec}` in a runtime eval's `args`, or `inputs = [spec]` on a runtime eval. An input is admitted to the review's [tool scope](../guides/tools.md) and is part of that eval's [reuse key](reuse.md#the-reuse-key).
- **Per-eval gating**: plain `verify` schedules dependency-first. If `app/e2e` names `e2e-tests` (outside `app`'s own reference cycle), then `app/e2e` waits until every eval of `e2e-tests` (and any reference cycle `e2e-tests` belongs to) has current passing evidence (`WAIT_DEPENDENCY`; missing upstream checks run only if they are included in the selection, for example with `--recursive` or `--all`, and a selected waiting Human sign-off can be deferred so downstream checks proceed as `PROVISIONAL`; see [Sign-offs and the end of a Run](runs.md#sign-offs-and-the-end-of-a-run)) and is `BLOCKED` if any of them is `RED` or `BLOCKED` (naming the root blocking eval in `blockedBy`). Gates control fresh execution and final satisfaction: if `app/e2e` already has a matching cached result while its gate is `RED`, `verify` can attach that cached result without running `app/e2e` (`status` shows state `BLOCKED` with action `reuse`). Other evals on `app` that do not name `e2e-tests` can still run immediately.
- **Cycles**: Artifacts whose evals name each other directly or through a chain of inputs form a **reference cycle**. References between peers in a reference cycle do not gate on the peer itself, though an eval that references a cycle peer inherits the cycle's external gates from outside the cycle (and Human evals still wait for their own Artifact's machine evals). Note that `config graph` components also group `parts` composition edges: a cycle closed through `parts` rather than mutually named inputs does not remove reference gates.
- **Human sign-off prerequisite**: a Human eval on an Artifact starts only after every non-Human (`runtime` and `agent`) eval on that same Artifact passes (`GREEN` or `PROVISIONAL`) and the Human eval's own upstream dependency gates are satisfied; see [Sign-offs and the end of a Run](runs.md#sign-offs-and-the-end-of-a-run).

## Composition (`parts`) {#composition}

An Artifact can be made of other Artifacts by listing their names in `parts = [combat, physics]`. Target path nesting alone never creates composition.

- **Verdicts**: a whole cannot pass while any of its `parts` (recursively) fails. An Artifact with `parts` and no evals of its own summarizes its parts' states and passes once they all pass.
- **Selection**: `artifactize verify game` selects `game` and all of its `parts` recursively.
- **Reuse**: each Artifact's own `artifactsum` excludes the files of its declared `parts` (folder, file, and aspect parts alike), while a whole's eval reuse keys include all of its parts' fingerprints; see [Fingerprints and Reuse](reuse.md#the-reuse-key).
- **Tool visibility**: built-in file tools on a whole pass over folder `parts` unless the eval also names the part; see [Built-in tools](../guides/tools.md#builtin-tools) for how dynamic and fixed-path tools scope folder, file, and aspect parts.

## Basis Artifacts, definitions, and tags

- **`basis = true`**: marks an Artifact that owns no evals and is accepted as-is (such as an external requirement list or lockfile). Its fingerprint still participates in the reuse keys of evals that name it.
- **Reviewing `.artifactize/` definitions**: declare an Artifact whose `path` is `.artifactize/` or a folder or `.artf` file inside it to review your project's structure, for example with the built-in `artifactize` toolset (`artf graph : artifactize = { path = ".artifactize/" }`). Its fingerprint follows what the definitions declare once expanded, not their text; see [Fingerprints and Reuse](reuse.md#fingerprints-artifactsum-script-false).
- **`tags`**: `tags = ["type:code"]` adds display metadata for `status` and `config graph` without being part of the evaluation criteria or the definitions fingerprint.
