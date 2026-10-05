# `artifactize.json`

The exact rules for declarations: validation, both fingerprint forms, families,
scoped references and the runtime execution environment.
[Artifacts and evals](../concepts/artifacts-and-evals.md) introduces them.

## Declaration validation

[Artifacts and evals](../concepts/artifacts-and-evals.md#folder-configuration) describes the declaration and the
two `fingerprint` forms.

Discovery validates these fields without opening or executing scripts or inputs.
There is no implicit or always-stale mode: an Artifact without `fingerprint` has no
reuse, and neither does an eval that depends on it, so every `verify` reviews them
again. The former `staleKey` (0.2 and 0.3) and `stale` (0.1) fields fail
`config check` with a message showing the `fingerprint` shape, and so does the
content form's 0.4 `dependencies` option. The old `critics`, `stale.paths`,
`envRequirements`, `reviewPolicy.maxConcurrentExecutors` and tool metadata
`observation` fields are rejected, and so is CCDD's `resultCheck` `script` wrapper:
an Agent eval's [`resultCheck`](../guides/agent-evals.md#result-check) is a flat
`{command, args, timeoutMs}`. Agent and Human tools use the separate flat
declarations below.

## Content fingerprint

[Fingerprints and reuse](../concepts/fingerprints-and-reuse.md#content-fingerprint) describes `files` and `ignore`.

The value is `content:` plus the SHA-256 over each input file's owner-relative path
and the SHA-256 of its bytes, in path order. It covers only the Artifact's own
files: a dependency's change reaches an eval through the dependency's own
fingerprint in the [reuse key](../concepts/fingerprints-and-reuse.md#the-reuse-key).
A family instance hashes the shared folder without any instance's material, plus
its own material. Editing one instance's material re-reviews only that instance.

Walks follow the scoped path rules. Symlinks and special files fail closed unless
they are ignored, nothing is followed out of the owner, and a walk stops with an
error after 10,000 entries or 1 GiB. Hashing runs on preparation and on each
end-of-review recheck. Files a review writes into ignored paths, such as Python's
`__pycache__`, therefore never cause `INPUT_CHANGED`. A content fingerprint records a
manifest with the execution: per-file digests (16 hex digits) and the inputs digest.
A file map that would exceed 64 KiB is dropped from the manifest but still covered
by the fingerprint. `status` diffs this manifest against the current one, and the
recorded `fingerprints` of the key against the current dependency fingerprints.

## Fingerprint scripts

Before executing any eval, `verify` computes each declared fingerprint in the selected
required dependency closure, including dependencies whose evals are not selected.
Up to `--fingerprint-jobs N` fingerprints are computed at once (default: the CPUs
available to the process), in `status` as in `verify`, where the same bound also
covers the end-of-review rechecks. Nothing depends on completion order: values are
keyed by Artifact, and a failure reports the first failing Artifact in name order,
cancelling the fingerprints after it, which cannot change that report. Scripts can
therefore run at the same time as each other, each in its own private output
directory; a script that must run alone needs `--fingerprint-jobs 1`.
Every eval on an Artifact receives the same literal value, also saved on its
request and in the Run's Artifact validation. artifactize mixes nothing into a
script's output: no repository, eval, profile, tool view, dependency or content
salt. The eval's [reuse key](../concepts/fingerprints-and-reuse.md#the-reuse-key) combines it with
the Artifact's name, the fingerprints of the Artifacts the eval depends on and the
Eval definition hash. A content fingerprint failure (a missing input, a link, a
limit) aborts preparation the same way.

The command runs from its owner's folder with JSON on stdin:
`{"version":1,"artifactId":"example"}`. Family instances additionally receive
`"family":{"name":"family","material":["input.txt"]}`. Bare commands resolve
through PATH; absolute executables run as given, while relative executable paths
containing `/` (such as `./fingerprint.sh`) must remain inside the owner without
symlinks. Arguments stay literal except explicit scoped Artifact references,
resolved with the same rules as runtime argv: `{name}` may be a mount alias or a
global Artifact name, and each referenced Artifact (with its child and mount
closure) is admitted to the script's scope without becoming a graph relation.
`config check` rejects unknown, family and malformed references, and paths are
checked for existence and symlinks when the command is prepared. There are no
interpreter-specific flags, wrappers or entry-file rules.

`files` accepts up to 64 unique owner-relative literal file/directory paths.
These paths and family material must exist without symlink traversal on every call;
contents are never hashed. Commands share runtime isolation, cancellation, bounded
raw output and a 30,000 ms default timeout (1–2,147,483,647 ms allowed). Their private
external output, HOME and temporary directories are removed after each invocation.
Stdout must be exactly 1–128 ASCII characters from `[A-Za-z0-9._:-]`, optionally
followed by one LF. It is not trimmed or cleaned. Nonzero exit, malformed output,
timeout, cancellation, missing files or cleanup failure abort preparation with
an operational error, without starting any eval or falling back to an uncached
review. Stderr is not forwarded as a fingerprint diagnostic.

After each runtime or Agent review completes, the fingerprints its key covers are
recomputed before accepting a GREEN or RED verdict. A changed value records
ERROR/INPUT_CHANGED with no semantic result; a failed recheck also records ERROR. Force does not skip preparation or
this recheck. Artifacts without a fingerprint run without either step. There is no
workspace monitoring: content fingerprints hash files only at preparation and recheck.

## Artifact families

A subfolder's `artifactize.json` can declare a static family with
`"family": {"instances": "instances.json"}` or an inline instance-name map.
The family name is reserved, not an Artifact; each of its 1–10000 instances gets
ordinary Artifact and `instance/eval` ids. Instance names must be globally
unique and cannot shadow entries in the shared folder. Families cannot be the
workspace root, contain nested markers, or declare `reviewPolicy`.

An instance accepts `variant`, object `params`, and up to 64 unique existing
owner-relative `material` paths, resolved without symlink traversal. Parameters
merge shallowly: family defaults, then the named family variant, then the instance.
Exact `{"$param":"/pointer"}` objects inside views and evals copy JSON values
using RFC 6901 pointers, including arrays and the empty root pointer. No string
interpolation or parameter substitution occurs in names, mounts, fingerprint scripts,
or basis. Expanded declarations receive normal validation.

All instance scripts use the shared folder as cwd. A parent addresses material as
`<family-folder>/<instance>/<path>`; bypassing the instance is rejected. Instance
material is an ownership declaration, not a sandbox hiding sibling files.
Discovery keeps each instance's family membership and sorted material, without
computing any digest. Only a declared `fingerprint` can enter a reuse key.
Fingerprint scripts receive each selected instance's family name and material paths. A content fingerprint hashes the shared folder without any
instance's material, plus the instance's own material. Each review rechecks its
own instance's fingerprint. No workspace monitoring or automatic reuse is added.

The runtime-only fixture demonstrates parameterized views, shared evals,
independent inputs/results, and a shared fingerprint script (inert during discovery):

```sh
cargo run -q -p artifactize -- --repo crates/artifactize/tests/fixtures/families config check
cargo run -q -p artifactize -- --repo crates/artifactize/tests/fixtures/families verify --all
```

## Scoped input library

`config::read_workspace_config` (also used by `config check`) resolves nearest
child ownership, validates mounts and explicit references, and returns typed
input-to-consumer `relations` for graph scheduling. Instruction references resolve
an owner's mount alias or a global Artifact name. Backslash-escaped braces, doubled
braces, `${variables}`, nested/JSON groups and unmatched braces stay literal.
Payloads are never changed and references never expand file content.

`scope::eval_scope` admits the target and explicit references plus their child
and mount closure, not the referenced Artifacts' eval instructions.
`Scope::resolve_path` follows logical child/mount paths to canonical Artifact
ids; `Scope::resolve_input` additionally requires existing files/directories
without symlink traversal. Logical paths reject absolute paths, traversal, empty
components, backslashes, colons, controls and lengths above 4096 characters.

Before preparing a runtime command, call `scope::resolve_argv` with the admitted
scope. Explicit `{name}`, `{name}/path` and `--flag={name}/path` operands resolve to
absolute scoped input paths and add dependencies even without instruction
references. Use `{owner}/mount/path` for logical paths starting at the owner.
Other arguments (including escaped references) remain literal, and the command
is never interpolated. `config check` validates reference names and syntax but
does not open runtime operands or execute programs. Input existence and symlink
checks happen during argument preparation. Graph closure and runtime CLI execution
use these same resolvers. The Agent tool registry uses the same admitted scope.

## Runtime execution library

`runtime::Command::prepare(program, args, workspace, run_dir, timeout_ms)` prepares
one invocation for `runtime::execute`. Arguments are literal; the runner never adds
a shell. The default cwd is the canonical workspace; a scoped caller can set
`command.cwd` to the resolved Artifact directory. `process::run` remains the
lower-level API for already-resolved commands with a complete explicit environment.
`verify` uses this policy with the resolved owner Artifact as cwd.

Only `PATH` and `LANG` are inherited (`LANG` defaults to `en_US.UTF-8`). Each prepared
runtime command gets a fresh 0700 directory below the caller's external `run_dir`,
with 0700 `output`, `tmp`, `home`, and `cache` subdirectories. Children receive
`ARTIFACTIZE_WORKSPACE_DIR`, `ARTIFACTIZE_OUTPUT_DIR`, `ARTIFACTIZE_TMP_DIR`, private
`HOME` and `XDG_CACHE_HOME`, and `TMPDIR`/`TMP`/`TEMP` pointing at private temporary
storage. Existing output ancestors are canonicalized before creation; output
inside the canonical workspace, including through symlinks, is rejected. The
caller owns the run directory; prepared output persists after execution for
receipts and explicit pruning. Failed preparation removes its newly owned
invocation directory, not the caller's root.

The runtime deadline defaults to 30,000 ms and accepts 1 through 2,147,483,647 ms.
It covers inert-child registration and execution without a reset. Each raw stdout
and stderr stream is capped at 128 KiB while excess bytes are drained. Runtime
results decode UTF-8 lossily, remove ANSI CSI sequences and C0 controls except tab,
LF and CR (DEL is preserved, matching CCDD), and retain the truncation flag and
actual exit status/duration. Low-level process results remain unmodified bytes.

Cancellation and timeout signal the entire owned process group with SIGTERM,
then SIGKILL after at most one second; ordinary leader exit also kills its group.
Capture stops waiting at most one second after cleanup if a pipe remains open.
Dropping the caller cancels the supervisor, which still cleans up and reaps the
leader. Configured programs are trusted local code, **not an OS sandbox**: they
can use ordinary OS access, must keep reviewed input unchanged, and descendants
that deliberately detach into another process group can escape cleanup. These
controls do not promise confinement or detection of every adversarial transient
write.
