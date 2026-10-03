# Artifactize

A Rust rebrand of [CCDD](https://github.com/lhj6102/ccdd), ported from CCDD 7.0.0 (`cbf28b4`).

Work in progress. See the [plan](docs/PLAN.md) and the [parity checklist](docs/ccdd-7-inventory.md).

## Runtime CLI

```sh
cargo run -q -p artifactize -- --repo crates/artifactize/tests/fixtures/runtime verify --all
cargo run -q -p artifactize -- --repo crates/artifactize/tests/fixtures/runtime run show RUN_ID
```

The fixture intentionally includes GREEN, RED, a timeout ERROR, RED-blocked and
ERROR-waiting dependents, and a two-Artifact cycle. Its overall exit code is 2.
`verify` requires exactly one selector: positional `ARTIFACT`, `--critic ID`,
`--critics CSV`, `--artifacts CSV`, `--critics-file PATH`, `--artifacts-file PATH`,
or `--all`. Critic IDs are qualified (`green/check`). Selection order is preserved,
with duplicates removed at their first occurrence; Artifacts expand to their
Critics in declaration order. Runtime Critics execute sequentially when their
dependency gates allow. An Artifact selector runs only that Artifact's Critics;
an individual Critic selector runs only that Critic. Both retain the full dependency
closure as a final obligation. `--recursive` includes every Critic in that closure,
including other Critics on the selected Artifact and cycle peers, in configuration
order. `--all` already includes every Critic and all no-Critic Artifact obligations.
A family name selects every instance, also in
`--artifacts` and `--artifacts-file`; overlapping family/instance entries are
deduplicated without selecting the family template itself.

RED blocks downstream execution; missing/operational evidence waits. Cycle peers
have no internal gates. `--ignore-gates` bypasses execution gates only: final
validation still requires actual GREEN evidence or explicit `basis: true` throughout
the required scope. A basis never waives its dependencies. Selected GREEN results
with missing obligations remain recorded in an INCOMPLETE Run; both text and JSON
output identify unmet obligations. Agent/Human Critics fail clearly before execution.

Root `reviewPolicy.dependencyGates` defaults to `green`; `ignore` enables bypass.
The library's `project::VerifyOptions.ignore_gates` can explicitly override either
policy, including `Some(false)` to enforce gates. `--force` marks only explicitly
selected Critics for a fresh review, not recursive dependencies; it neither expands
the execution scope nor bypasses gates. Runs record the resolved policy and each
request's force flag. Every included Critic currently executes without reuse;
P3.4 will connect force to cache lookup/join/publication bypass.

Selection files must be regular files no larger than 4 MiB, containing a JSON
string array or one trimmed ID per line (UTF-8 BOM and CRLF are accepted). They
must contain 1–100000 IDs before deduplication. Empty lines are ignored; IDs cannot
contain ASCII whitespace, control characters or commas. Malformed JSON arrays
never fall back to line parsing. Relative file paths resolve from the CLI's cwd,
not `--repo`.

`verify ... --profile NAME` selects a complete declared `profileVariants` entry
for every included Critic. Each Critic can declare up to 64 safely named variants,
all retaining its default reviewer kind. Unknown variants fail before creating a
Run, and source declarations are never rewritten. The library accepts
`project::selection::ProfileSelection::Named` or `ProfileSelection::Critics` (a
qualified-Critic-to-name map); mappings outside the included scope fail. With
`--recursive`, variants also apply to dependency Critics. Runtime
variant arguments rebuild scoped references and dependency gates. Stored request
profiles and argv describe the variant actually used.

P1 verification is foreground, with or without `--wait`: GREEN exits 0, RED 1,
ERROR 2, and INCOMPLETE 4. Ctrl-C/SIGTERM cancels the owned child group and records
ERROR/CANCELLED, never RED. P3.2 will add detached submission and CCDD's non-wait
acceptance codes; P3.15 adds following/wait timeouts without cancelling execution.
`--json` prints compact requester results; `--full` includes the runtime audit.
`run show RUN_ID` always prints full saved JSON and exits 0 on a successful read,
regardless of the saved verdict. It never discovers declarations or runs code.

Default receipts live in `$ARTIFACTIZE_STATE_HOME/<canonical-repo-sha256-prefix>`
(or the state home fallback), in `receipts.sqlite` (bundled SQLite, WAL, schema 1).
`--state-dir PATH` moves only Run receipts/history and binds that directory to its
original canonical repository. Use `--state-dir PATH run show RUN_ID` without
`--repo` to read even after the original repository is removed. Private run output
lives below the receipt directory; state/output inside the reviewed repository is
rejected, including through symlink ancestors. No writer transaction spans a
subprocess or async suspension. This phase does not reuse earlier Run evidence,
run identity hooks, or monitor/hash the workspace. End-of-review identity checks
arrive with identity commands in P3.3.

## Scoped input library

`config::read_workspace_config` (also used by `config check`) resolves nearest
child ownership, validates mounts and explicit references, and returns typed
input-to-consumer `relations` for graph scheduling. Instruction references resolve
an owner's mount alias or a global Artifact name. Backslash-escaped braces, doubled
braces, `${variables}`, nested/JSON groups and unmatched braces stay literal.
Payloads are never changed and references never expand file content.

`scope::critic_scope` admits the target and explicit references plus their child
and mount closure, not the referenced Artifacts' Critic instructions.
`Scope::resolve_path` follows logical child/mount paths to canonical Artifact
identities; `Scope::resolve_input` additionally requires existing files/directories
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
use these same resolvers. Tool enforcement is delivered by later tasks.

## Artifact families

A subfolder's `artifactize.json` can declare a static family with
`"family": {"instances": "instances.json"}` or an inline instance-name map.
The family name is reserved, not an Artifact; each of its 1–10000 instances gets
ordinary Artifact and `instance/critic` identities. Instance names must be globally
unique and cannot shadow entries in the shared folder. Families cannot be the
workspace root, contain nested markers, or declare `reviewPolicy`.

An instance accepts `variant`, object `params`, and up to 64 unique existing
owner-relative `material` paths, resolved without symlink traversal. Parameters
merge shallowly: family defaults, then the named family variant, then the instance.
Exact `{"$param":"/pointer"}` objects inside views and Critics copy JSON values
using RFC 6901 pointers, including arrays and the empty root pointer. No string
interpolation or parameter substitution occurs in names, mounts, identity hooks,
basis, or environment requirements. Expanded declarations receive normal validation.

All instance scripts use the shared folder as cwd. A parent addresses material as
`<family-folder>/<instance>/<path>`; bypassing the instance is rejected. Instance
material is an ownership declaration, not a sandbox hiding sibling files.
Discovery keeps each instance's membership, sorted material, and a SHA-256/JCS
entry digest independent of sibling entries. Material fingerprints arrive in P2.4;
identity execution and end-of-review rechecks remain P3.3. No workspace monitoring
or automatic reuse is added.

The runtime-only fixture demonstrates parameterized views, shared Critics,
independent inputs/results, and an identity hook that remains inert:

```sh
cargo run -q -p artifactize -- --repo crates/artifactize/tests/fixtures/families config check
cargo run -q -p artifactize -- --repo crates/artifactize/tests/fixtures/families verify --all
```

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
write. Durable child accounting and lease release belong to P3.10.
