# Fingerprints and Reuse

When you run `artifactize verify`, any eval whose criteria and covered inputs have not changed since a saved `GREEN` or `RED` verdict reuses that earlier result instead of executing again.

## Fingerprints (`artifactsum`, `script`, `false`) {#fingerprints-artifactsum-script-false}

Each Artifact declares how its own state is fingerprinted:

- **Default (`artifactsum` for local targets)**: omit `fingerprint` on a `folder`, `file`, or `aspect` Artifact (or write `fingerprint = {}` on a folder) to hash its own files with SHA-256.
  - A **folder** hashes all files under its `path`, automatically excluding built-in ignores (`.git`, `target/`, `node_modules/`, `__pycache__/`, `*.pyc`), `.gitignore` matches, and the targets of all declared `parts` (folder, file, and aspect parts alike). Narrow or filter a folder with `fingerprint = { files = ["src"], ignore = ["*.log"] }` (built-in ignores, `.gitignore`, and `ignore` filter directory walks, whereas a file path named explicitly in `files` is hashed unless owned by a declared file/aspect part; naming a path inside a declared folder part in `files` is rejected).
  - A **file** Artifact hashes only its target file.
  - An **aspect** hashes only its `select` paths (minus any parts' targets).
  - A **definition** target (`.artf` files) hashes the normalized **artf view**, which ignores `[annotations]`, `#` comments, and formatting changes.
- **Custom script**:
  ```toml
  fingerprint = { script = { command = "python3", args = ["hash.py"], files = ["input.txt"], timeout_ms = 10000 } }
  ```
  The command runs in the Artifact's execution folder, receives `{"version": 1, "artifactId": "<name>"}` on `stdin`, and must exit `0` and print one single-line stable fingerprint token on `stdout` (both `verify` and `status` may run it; optional `files` lists execution-folder-relative paths that must exist before the script runs, without hashing them automatically). Omitting `fingerprint` on a **Live** Artifact leaves it without a snapshot (and therefore without a reuse key); declare a script that prints a stable snapshot (such as a deployed commit SHA) if Agent or Human reviews on the Live target should be reused while that snapshot stands. Any `runtime` eval whose target, target `parts`, or directly referenced `{name}` dependencies include a Live Artifact has no reuse key and cannot reuse a saved verdict (even when the Live Artifact declares a snapshot script), so it executes whenever selected, unblocked by dependency gates, and permitted by `--reuse-only`.
- **Disable reuse**: `fingerprint = false` disables caching and reuse for that Artifact and any eval that depends on it; those evals have no reuse key and execute whenever selected, unblocked, and permitted by `--reuse-only`.

## The reuse key {#the-reuse-key}

An eval's **reuse key** is a 64-character hex hash combining:

1. **The Eval definition hash**: the eval's `kind` (`runtime`, `agent`, or `human`), `payload` (including `instruction`), `red_at` (on `agent` evals), `pass_schema`, `fail_schema`, and (for `runtime` evals) `command` and `args`.
2. **The target Artifact**: its name, kind (`folder`, `file`, `aspect`, or `live`), and current fingerprint.
3. **All `parts` of the target Artifact** (recursively): each part's name, kind, and fingerprint (adding the first part to or removing the last part from a target invalidates reuse even if that part was already referenced directly). Changing a part therefore re-reviews the part and the whole's evals, whereas composition alone does not re-review a part when only the whole's own files change (unless the part's eval explicitly references `{whole}`).
4. **Direct `{name}` dependencies** named in the eval's instruction or runtime `args`: each referenced Artifact's name, kind, and fingerprint (one hop only; a dependency's own dependencies do not enter this eval's key, though they still gate final Run satisfaction).

### What does NOT change the Eval definition hash

Execution options and presentation metadata are excluded from the Eval definition hash so you can tune execution without changing the evaluation criteria:

- Agent `backend`, `model`, `reasoning`, `timeout_ms`, `max_tokens`, `max_tool_calls`, and selected `--profile` variant (unless a runtime variant changes `command` or `args`).
- Eval `title`, `mutex` tags, Artifact `tags`, physical repository paths, and tool declarations (`views`). (Note that if an Artifact's own `artifactsum` covers `.artf` files—such as a definition-review node targeting `.artifactize/`—editing semantic fields in those `.artf` files changes that Artifact's file fingerprint.)

Because physical paths are not part of the key, separate worktrees, clones, teammates' machines (via the [team review store](../guides/team-and-ci.md)), and CI jobs reuse each other's verdicts whenever Artifact names, kinds, fingerprints, and eval criteria match.

## Completed result reuse and `--force` {#completed-result-reuse}

- **Latest record wins (and live execution joining)**: completed `GREEN` and `RED` eval executions (never `ERROR` or cancelled evals) are stored under their reuse key when inputs remain unchanged at the end of that eval's execution (even if the enclosing Run is later cancelled; see [Speculation](runs.md#speculation) for how a speculative result cached before its upstream gates resolve is withdrawn from reuse if an upstream gate fails). If multiple records exist locally or in a configured remote store, `verify` reuses the one with the newest completion timestamp. When no completed result exists yet, a non-forced Run can join an already running `runtime` or `agent` execution with the same reuse key and effective profile in the same state directory instead of starting a duplicate execution.
- **Inspecting what changed**: when an eval with a reuse key has no current reusable result and a prior locally cached execution exists for both the same qualified eval ID and the same eval criteria, `artifactize status` compares current fingerprints against that latest record and reports which `{name}` dependencies or target files changed (naming individual files when comparable file-level artifactsum records exist, or reporting a changed fingerprint for script-based fingerprints).
- **Forcing re-evaluation (`--force`)**: `artifactize verify SELECTOR --force` bypasses local cache reuse and live execution joining only for the explicitly selected executable evals (and selected Artifacts' parts, never dependencies), skips remote store lookups for the entire Run, and publishes newly completed `GREEN` or `RED` results to the cache and configured store as the newest record for their reuse key.
