# Fingerprints and reuse

A fingerprint is what a review depends on. While it is unchanged, artifactize
reuses the earlier GREEN or RED result instead of reviewing again, so review cost
follows the size of a change.

## Content fingerprint

`"fingerprint": {}` makes review cost follow the size of a change: an Artifact is
reviewed again only when its own files or its dependencies change, and every other
result is reused. The fingerprint is `content:` plus a SHA-256 over the Artifact name,
each input file's owner-relative path and bytes, and one entry per dependency in
the chosen scope.

- `files`: 1–64 unique owner-relative paths, default `["."]` (the whole owner
  folder). Each must exist and stay inside the Artifact: a path into a child
  Artifact or a mount is rejected, because dependencies come from `dependencies`.
  Directory walks skip child Artifact folders, the owner's `artifactize.json` and
  a family's instance list. Their effect already reaches the fingerprint through the
  Eval definition hash and the dependency list.
- `dependencies`: `none`, `direct` (default) or `transitive`. Dependencies are the
  graph's own: children, mounts and `{artifact}` references in instructions and
  runtime argv. With `direct`, merging a change into `core` re-reviews `core` and
  the Artifacts that use it directly, and only those: the new pairing. Artifacts
  further downstream keep their results, because `direct` never looks past one
  hop. `transitive` covers the whole dependency closure and re-reviews everything
  downstream. Choose it when a review really reads indirect dependencies.
- `ignore`: up to 64 `.gitignore`-style globs relative to the owner, without
  negation. They always exclude, like the built-in ignores `.git`,
  `__pycache__/`, `*.pyc`, `target/` and `node_modules/`. `.gitignore` files
  exclude more, with git semantics: every one from the repository root (`--repo`)
  down through the Artifact and the walked directories applies, each pattern is
  relative to its own file's folder, negation works, and a deeper file overrides a
  shallower one. Explicitly named `files` are never ignored.

How dependency entries are hashed, the walk limits and the recorded manifest are
in the [reference](../reference/artifactize-json.md#content-fingerprint); the script form is under
[fingerprint scripts](../reference/artifactize-json.md#fingerprint-scripts).

## Completed result reuse

A successful fingerprint recheck publishes either GREEN or RED to `cache_entries`,
pointing to a self-contained `executions` row. Errors and cancellation are never
published. The key is **(fingerprint, Eval definition hash)**, intentionally
departing from CCDD, whose key was its identity alone. The definition hash is lowercase SHA-256
of canonical JSON with recursively sorted keys, containing the effective
`profile`, `payload` (including instruction), `passSchema` and `failSchema`.
The profile is the selected variant's full definition when `--profile` is used:
runtime command/args/timeout, Agent backend/model/reasoning/budgets/timeout, or
Human. Eval id/title, repository paths and unused profile variants are excluded.
Equal definitions still share across evals and repositories; changing criteria,
schema, args or effective profile requires a separate execution.

A script fingerprint hashes no script or material file contents. Owners must still
encode input, script and material changes that invalidate results in its output.

A hit returns the original result without running the eval or re-validating it
against the requested profile/schema. The request saves the original execution ID,
actual `profile`, `evalDefHash`, `provenance` (repository, Run, request, eval,
definition hash and completion time)
and the original attempts as `reusedUsage` when reported, alongside
`requestedProfile`. Its own `usage` is null: a reused request spent nothing.
Runtime usage is null, not an invented zero. Cached RED remains RED for gates and final obligations.
Dependencies outside execution selection can supply cached evidence without
running. Results remain readable after the source repository is deleted; external
paths embedded in result text are not made portable.

No fingerprint means no cache lookup or publication. `--force` bypasses lookup and
publication for explicitly selected evals, leaving any existing entry unchanged.
Forced and uncached results still satisfy their own Run and retain execution audit.
