# Artifactize

A Rust rebrand of [CCDD](https://github.com/lhj6102/ccdd), ported from CCDD 7.0.0 (`cbf28b4`).

Work in progress. See the [plan](docs/PLAN.md) and the [parity checklist](docs/ccdd-7-inventory.md).

## Runtime CLI

```sh
cargo run -q -p artifactize -- --repo crates/artifactize/tests/fixtures/runtime verify
cargo run -q -p artifactize -- --repo crates/artifactize/tests/fixtures/runtime run show RUN_ID
```

The fixture intentionally includes GREEN, RED, a timeout ERROR, RED-blocked and
ERROR-waiting dependents, and a two-Artifact cycle. Its overall exit code is 2.
`verify [ARTIFACT]` runs runtime Critics sequentially in dependency-gate order;
`--all` or an omitted selector selects everything. An Artifact selector runs only
that Artifact's Critics, while its dependency closure remains a final obligation.
RED blocks downstream execution; missing/operational evidence waits. Cycle peers
have no internal gates. Selected GREEN results with missing obligations remain
recorded in an INCOMPLETE Run. Agent/Human Critics fail clearly before execution.

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
use these same resolvers. Family expansion and tool enforcement are delivered by
later tasks.

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
