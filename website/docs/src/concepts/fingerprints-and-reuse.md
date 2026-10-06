# Fingerprints and reuse

A fingerprint says what an Artifact's reviews depend on. An eval's earlier result
is reused when the eval is unchanged and the fingerprints of the Artifacts it
depends on are unchanged, so review cost follows the size of a change.

## Fingerprints

You define the fingerprint of each Artifact, in one of two forms, and artifactize
uses the value as it is. It adds nothing to it: no tool declarations and no other
Artifact's fingerprint. Dependencies enter through the
[reuse key](#the-reuse-key) instead.

- `"fingerprint": {}`, the built-in **content** form, is `content:` plus a
  SHA-256 over each of the Artifact's own input files: its owner-relative path
  and bytes.
- `"fingerprint": {"script": {...}}` runs your command, and its output is the
  fingerprint. It hashes nothing on its own, so the command must reflect every
  input, script and material change that should re-review.

## Content fingerprint

`"fingerprint": {}` hashes the Artifact's own files. An Artifact is reviewed again
when those files change or when an Artifact it depends on changes its fingerprint.

- `files`: 1–64 unique owner-relative paths, default `["."]` (the whole owner
  folder). Each must exist and stay inside the Artifact: a path into a child
  Artifact or a mount is rejected, because those are Artifacts of their own, with
  their own fingerprints. Directory walks skip child Artifact folders, the
  owner's `artifactize.json` and a family's instance list. Their effect already
  reaches the key through the Eval definition hash and the dependency
  fingerprints.
- `ignore`: up to 64 `.gitignore`-style globs relative to the owner, without
  negation. They always exclude, like the built-in ignores `.git`,
  `__pycache__/`, `*.pyc`, `target/` and `node_modules/`. `.gitignore` files
  exclude more, with git semantics: every one from the repository root (`--repo`)
  down through the Artifact and the walked directories applies, each pattern is
  relative to its own file's folder, negation works, and a deeper file overrides a
  shallower one. Explicitly named `files` are never ignored.

The 0.4 `dependencies` option (`none`, `direct`, `transitive`) is gone, and
`config check` rejects it with a message: a fingerprint covers only its own
Artifact. The walk limits and the recorded manifest are in the
[reference](../reference/artifactize-json.md#content-fingerprint); the script form
is under [fingerprint scripts](../reference/artifactize-json.md#fingerprint-scripts).

## The reuse key

An eval depends on its target Artifact and on what the target directly connects:

- the target's mounts;
- the target's child Artifacts;
- the Artifacts the eval names in its instruction or runtime args.

artifactize does not follow connections further than that. Whether a change two
connections away matters is up to how you define fingerprints: a fingerprint
script can read a file of any Artifact it names in its args. The key is

```text
hash(Eval definition hash, sorted (Artifact name, fingerprint) of each Artifact the eval depends on)
```

Artifact names are part of the key, so two Artifacts never share a result, even
when their fingerprints are equal. The **Eval definition hash** is lowercase
SHA-256 of canonical JSON with recursively sorted keys over the eval strategy:

- the eval kind (runtime, agent or human);
- `payload`, including the instruction;
- `passSchema` and `failSchema`;
- for runtime evals, the command and args.

Execution options are not part of the key: an Agent's backend, model, reasoning,
`timeoutMs`, `maxToolCalls` and `maxTokens`, a runtime `timeoutMs`, and the
selected profile variant. Neither are the eval id and
title, repository paths, unused profile variants, or tool declarations (`views`): a
tool is a way of viewing an Artifact, so changing only a tool's description or
schema does not review again. To make a tool change matter, include the relevant
files in the fingerprint.

An eval has a key only when its target and every Artifact it depends on declare a
fingerprint. Without one, it has no reuse: every `verify` reviews it again, and
`status` names the Artifact that lacks a fingerprint. A basis Artifact that other
Artifacts mount or name needs `"fingerprint": {}` for their results to be reused.

## Completed result reuse

A successful end-of-review recheck adds the GREEN or RED result to its key's
history: its self-contained `executions` row gets its completion time, size and last
use ([State](../reference/state-cache-limits.md#state)). Errors and cancellation are
never recorded there. When a key holds more than one record,
the most recent by completion time is reused, GREEN or RED. Results from different
profiles therefore reuse each other: a review that `--profile fast` produced
satisfies the declared profile, and the other way around.

Each record keeps how and by whom it was produced, next to its result:

- `options`: the backend, model, reasoning, `timeoutMs`, `maxToolCalls`,
  `maxTokens` and `variant` it ran with, as declared;
- `profile`: the effective profile;
- `producer` (`user@host` and artifactize version), the Human `reviewer`, and for
  a remote record its `origin` with the publisher;
- `completedAt`, `fingerprints` (each Artifact the key covers) and `key`.

A hit returns the original result without running the eval or re-validating it
against the requested profile or schema. The request saves the original
execution ID, the producing `profile` and `options`, `evalDefHash`, `key`,
`provenance` (repository, Run, request, eval, definition hash, completion time and,
for Agent results, the [pins of their tools' execution paths](../reference/agent-tools.md))
and the original attempts as `reusedUsage` when reported, alongside its own
`requestedProfile`. Its own `usage` is null: a reused request spent nothing. Its
`source`, `{"runId", "requestId", "kind"}`, names the request whose execution
produced the result and whether it was a completed record (`cache`), a live
execution the request waited for (`joined`) or a remote store record (`remote`);
`source` is null on a request that executed itself
([Runs](runs-and-status.md#verify-and-runs)).
Runtime usage is null, not an invented zero. A reused RED remains RED for gates
and final obligations. Dependencies outside execution selection can supply cached
evidence without running. Results remain readable after the source repository is
deleted; external paths embedded in result text are not made portable.

With a [team review store](../reference/review-store.md#remote-review-store-client),
`verify` also asks the store for every key and reuses whichever record completed
last, the local latest or the store's latest; a store that cannot be reached leaves
the local latest.

No key means no cache lookup or publication. `--force` re-executes explicitly
selected evals without reading or joining earlier results, local or in the store,
and its completed GREEN or RED is appended as a newer record (and published to a
configured store), which later runs then reuse. Forced and
unkeyed results still satisfy their own Run and keep their execution.

## Upgrading to 0.6

0.6 keeps the key of 0.5, but starts a new state: artifactize refuses a state an
earlier version wrote and does not migrate it
([State](../reference/state-cache-limits.md#state)). The first `verify` in the new state
reviews everything once, or reuses what a [team review store](../reference/review-store.md)
holds for the same keys.
