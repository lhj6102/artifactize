# artifactize core stack research

## Summary

- Recommend **Tokio + rusqlite**, with each SQLite connection owned by a dedicated blocking database thread; use **tokio-rusqlite** initially rather than inventing that bridge.
- Preserve SQLite WAL, short `BEGIN IMMEDIATE` transactions, read-only snapshot queries, explicit busy handling, and separate project-state versus machine-resource databases.
- Recommend **jsonschema 0.58.4** for dynamic argument and verdict-schema validation. **Schemars generates schemas; it does not replace runtime validation.**
- Recommend the official **rmcp 3.5.0** SDK with **stdio transport**, dynamic tool definitions, explicit message limits, and application-controlled cancellation/auditing.
- Use a **language-neutral command supervisor** for runtime evals, script tools, and owner identity. Do not retain CCDD’s Node-only Runtime restriction or Node import hooks.
- Use **tokio::process + process-wrap 10.0.1** for async pipes, Unix process groups/sessions, and Windows Job Objects; retain explicit admission-before-execution and cleanup logic.
- Use **SHA-256 and one defined RFC 8785/JCS encoding** for artifactize’s new identity format. Compatibility with CCDD config, state, and hashes is explicitly unnecessary.
- Use **Axum** for the optional loopback HTTP API, with the existing security boundary implemented deliberately; neither HTTPS nor a TLS crate is required for this API.
- Use **petgraph’s iterative SCC algorithm**, but keep graph ordering, identity construction, dependency gates, and evidence semantics in artifactize.
- **Drop the offline monkey-patch guard.** The load-check benchmark should structurally construct only a synthetic executor and never load credentials or instantiate a real provider client. OS network sandboxing is not required for this purpose.
- Use `cargo test`, `assert_cmd`, `insta`, and `proptest`, with real subprocess and multiprocess SQLite integration tests.
- Versions and licenses below were checked against the **crates.io API on 2026-10-03**, filtering out yanked/prerelease/future releases and selecting the highest stable semantic version, rather than trusting stale search snippets.
- A throwaway Rust spike compiled and ran successfully. It verified the selected crate combination, dynamic MCP schemas, JSON Pointer escaping, `contains` bounds, SQLite JSON functions/WAL contention, and canonical JSON examples.
- No files under `/home/user/code/artifactize` or `/home/user/code/ccdd` were modified. This complete report is returned rather than written to a report file, as required by my worker instructions.

## 1. Scope and interpretation

This report covers the CCDD 6.6.0 core engine, its CLI/process/filesystem/database dependencies, default script tools, and the HTTP side of the optional monitor. It excludes LLM-provider SDK selection and the desktop UI framework/layout stack.

The reviewed snapshot is `ccdd@eddf8f7`.

The recommendations incorporate two subsequent product decisions relayed by the coordinator:

1. **artifactize has a new format only.** It need not read CCDD configuration or SQLite state or reproduce CCDD record/content hashes.
2. **Execution is generic CLI execution.** Tests and functions are invoked using fixed command/argument declarations, not special Node runtime behavior.

CCDD is useful here as an inventory of responsibilities and invariants, not as a requirement to reproduce every implementation quirk.

I read the architecture map, repository guidance, implementation contracts, and the relevant core implementations. The current implementation/contracts take precedence over stale historical wording: for example, current CCDD gates dependency Critics outside the same SCC unless explicitly bypassed, despite an older sentence in `AGENTS.md` suggesting otherwise.

## 2. SQLite persistence and concurrency

### How CCDD uses it

Important implementations:

- [Broker initialization and transactions](ccdd@eddf8f7/src/broker/index.ts), especially initialization around lines 132–180.
- [Normalized immutable records and caches](ccdd@eddf8f7/src/broker/storage.ts).
- [Read-only project store](ccdd@eddf8f7/src/project/store.ts).
- [State-format gate](ccdd@eddf8f7/src/state-format.ts).
- [Machine-wide admission database](ccdd@eddf8f7/src/resources.ts).

This is not a simple single-process key/value store. Independent CLI processes, detached Run workers, Human-action processes, and monitor readers access the same project database. A second machine-wide database coordinates weighted identity capacity, provider/model execution slots, FIFO admission, durable submission budgets, process leases, and child cleanup.

CCDD uses synchronous `DatabaseSync`, prepared statements, SQLite JSON extraction/expression indexes, WAL, foreign keys, and a 5-second busy timeout. Most mutations use short synchronous `BEGIN IMMEDIATE` transactions. Read-only queries open existing stores and use `BEGIN` for consistent snapshots; they must not create missing state, reconcile owners, or mutate review lifecycle records.

State format is currently `PRAGMA user_version = 6`. Unmarked/noncurrent project stores are rejected, not migrated. This does **not** mean every database is migration-free: `resources.ts` explicitly adds an `execution_attempts.provenance` column using a rechecked `ALTER TABLE` inside `BEGIN IMMEDIATE`.

The project store separates immutable content-addressed definition/result nodes from small mutable lifecycle headers, memberships, gate counters/edges, append-only events, and lifecycle cursors. Its encoded-node cache combines `PRAGMA data_version` with `total_changes()` and clears on rollback. SQLite’s `data_version` detects other connections’ commits, not the current connection’s own commits, and comparisons are meaningful only on the same connection. [SQLite documentation](https://www.sqlite.org/pragma.html#pragma_data_version)

### Options

| Option | Fit | Drawbacks |
|---|---|---|
| **rusqlite 0.40.2** | Direct match for current SQL and transactional state machines; explicit connection flags, immediate transactions, prepared-statement caches, busy/interrupt support; bundled SQLite available. | Blocking API; cannot run potentially busy operations on Tokio runtime threads. Connection ownership must be explicit. |
| **rusqlite + tokio-rusqlite 0.8.0** | One dedicated thread per connection; a complete operation/transaction runs in a synchronous closure and returns asynchronously. Small conceptual change from CCDD. | Cancellation of the awaiting future is not transactional cancellation; queued/running work still needs application policy. Add bounded admission/backpressure rather than accepting an unlimited work queue. |
| **SQLx 0.9.0** | Good async API, pools, migration facilities, optional compile-time query checking, SQLite options. | SQLite still runs on background worker threads: it does not become a nonblocking database. More abstraction for a SQLite-only, transaction-heavy local engine. Checked queries need schema metadata/database availability or an offline cache. More opportunities to accidentally keep a write transaction across unrelated awaits. |
| Diesel/SeaORM or another ORM | Useful where model/query abstraction or multiple server databases is a requirement. | Adds little here. CCDD’s state transitions and integrity checks are explicit SQL, not ordinary CRUD. |
| A custom rusqlite actor | Gives precise queue bounds, cancellation metadata, priorities, and operation-level metrics. | More code and concurrency surface. Worth doing only if the thin adapter proves limiting. |

SQLx 0.9.0 defaults include a 5-second SQLite busy timeout and foreign keys enabled, but it deliberately does **not** choose a journal mode automatically. Its connection options document one worker thread per connection. [SQLx SQLite options](https://docs.rs/sqlx/0.9.0/sqlx/sqlite/struct.SqliteConnectOptions.html)

### Recommendation

Use **rusqlite 0.40.2**, preferably with `default-features = false, features = ["bundled", "cache"]`, behind **tokio-rusqlite 0.8.0** or an equivalent narrow database-thread interface.

Start with one serialized writer connection per logical store per process, plus a small number of read-only connections when useful. A large connection pool is counterproductive: WAL permits concurrent readers but still has only **one writer**. Keep transactions entirely inside one database-thread call. Never hold a SQLite write lock during process execution, filesystem hashing, Human waiting, admission waiting, HTTP I/O, or recursive scratch deletion.

Keep the project database and machine-admission database distinct, and explicitly model the state machine between them: there is no magical atomic transaction spanning these two independently opened databases. Admission, attempt-start recording, cleanup, and lease release must be idempotent and recoverable.

For artifactize’s new store:

- Define its own schema/state version, application identity, and explicit read/write opening policy.
- Initialize schema atomically under a write transaction; concurrent first-open must be tested.
- Use WAL and foreign keys explicitly; initially choose durable settings rather than silently changing SQLite’s `synchronous` behavior for speed.
- Preserve consistent read snapshots and no-create behavior for read-only commands.
- Implement bounded busy handling around **the whole replay-safe transaction**, not arbitrary individual statements.
- Distinguish a busy error from corruption, I/O failure, and semantic conflict.
- Decide how commit success is learned before retrying a canceled/timed-out caller. Do not blindly replay an operation that may already have committed.
- Keep caches connection-local and invalidate them for external commits and local writes/rollbacks.
- Preserve transactional lifecycle cursors: rollback must not expose a transition; telemetry should not create lifecycle changes.

The 5-second CCDD busy timeout is a useful baseline, not a promise of responsiveness. On a database thread it no longer stalls the async event loop, but it can still delay all queued database work. A short busy timeout plus a bounded outer retry/deadline may offer better cancellation behavior; benchmark before choosing values.

### Risks and maintenance

- WAL databases belong on a supported **local filesystem**, not a network share. Long-lived read transactions can prevent checkpoint progress and grow the WAL. A monitor should not hold a read transaction while waiting for a browser. [SQLite WAL](https://www.sqlite.org/wal.html)
- Read-only access to a live WAL database is not equivalent to SQLite `immutable=1`; the latter disables relevant locking/change detection and is wrong for actively changing state.
- Select bundled SQLite for predictable JSON-function support and security fixes. The spike resolved `libsqlite3-sys 0.38.2` and reported **SQLite 3.53.2**. Assert the actual runtime version in CI; do not infer it only from rusqlite’s version.
- The SQLite WAL documentation records a WAL-reset corruption fix in 3.51.3, with backports. This is another reason to avoid accidentally using an old system SQLite. [SQLite WAL](https://www.sqlite.org/wal.html)
- `rusqlite 0.40.2` was published 2026-08-08, `tokio-rusqlite 0.8.0` on 2026-09-06, and SQLx 0.9.0 on 2026-05-21. This is evidence of current releases, not a guarantee of future maintenance.
- Licenses: rusqlite/tokio-rusqlite MIT; SQLx MIT OR Apache-2.0.
- A migration crate is not required on day one. If artifactize later needs incremental migration orchestration, **rusqlite_migration 2.6.0** is compatible with the 0.40 rusqlite line, but currently declares Rust 1.95 and should not be added just to run an initial schema script.

## 3. JSON Schema, owner responses, and safe diagnostics

### How CCDD uses it

- [Schema dialect and argument diagnostics](ccdd@eddf8f7/src/tools/schema.ts).
- [Verdict-specific response schemas](ccdd@eddf8f7/src/response-schema.ts).
- [Final-result diagnostics](ccdd@eddf8f7/src/executors/final-result.ts).
- [Tool result normalization](ccdd@eddf8f7/src/tools/runner.ts).

TypeBox performs runtime compilation/checking of **user-authored JSON schemas**; this is not merely TypeScript type generation.

CCDD deliberately accepts a restricted dialect: object roots, selected object/array/string/number/composition keywords, maximum schema nesting 20, no `$ref`, no conditional keywords, and `minContains`/`maxContains` only alongside `contains`. Unions use composition rather than an array-valued `type`.

`passSchema` and `failSchema` define extra owner fields for GREEN/RED. CCDD adds a required verdict constant and closes top-level extra properties. Top-level composition and reserved response/audit fields are rejected. A built-in Runtime result cannot satisfy a schema requiring extra owner output fields.

There are two deliberately different error boundaries:

1. **Tool arguments:** object/64 KiB guard; up to five schema diagnostics and 4 KiB of safe text; bounded property names and instance pointers; no argument values. Large invalid structures get a diagnostic-budget notice instead of exhaustive error generation.
2. **Final responses:** bounded schema-path/keyword diagnostics, deliberately omitting response values and response-controlled property names. CCDD does not just serialize validator errors into logs or repair prompts.

Tool results are primarily checked by **custom envelope/content validators**, not by one arbitrary result JSON Schema. They enforce content kinds, block counts, text/JSON/image limits, observation semantics, and the special authored-domain-error shape.

### Options

| Option | Assessment |
|---|---|
| **jsonschema 0.58.4** | Best fit. Supports drafts 4/6/7/2019-09/2020-12, reusable compiled validators, boolean checks, structured errors with instance/schema locations, and selectable pattern engines. Actively released. |
| **boon 0.6.1** | Credible alternative with the same principal drafts and tree-structured/standard-format errors. Smaller/slower release cadence. Its regex translation does not support all JavaScript regex constructs. |
| **schemars 1.2.2** | Good schema generation from Rust/Serde types; useful for artifactize-owned protocol/config documents. **Not a validator for user-authored dynamic schemas.** |
| Hand validation only | Appropriate for a fixed tool-result envelope, path rules, limits, and schema keyword allowlists. Not a sensible replacement for JSON Schema composition semantics. |
| Rust struct validation frameworks | Useful for typed app structs, not sufficient for arbitrary `passSchema`, `failSchema`, or per-tool schemas loaded at runtime. |

### Recommendation

Use **jsonschema 0.58.4 with default features disabled**, explicitly choose **Draft 2020-12**, and continue to validate artifactize’s supported dialect separately before compilation. Draft 2020-12 handles `contains` counts; selecting Draft 7 would silently change their meaning.

Do not accidentally widen artifactize’s accepted configuration merely because the crate implements more keywords. Start with an explicitly documented subset, reject unknown keywords, and add functionality by deliberate schema-format changes. Do not enable remote/file reference retrieval; dynamic configuration should not create network or filesystem loads during schema compilation. The crate supports disabling default resolution features and an offline retrieval policy. [jsonschema documentation](https://docs.rs/jsonschema/0.58.4/jsonschema/)

Use a bounded compiled-schema cache keyed by canonical schema bytes. Keep a boolean validation fast path; only generate diagnostics when needed. Construct application-owned typed diagnostic categories from validator `kind()` and pointers. Never forward `Display`, arbitrary error parameters, the instance value, or raw exceptions to reviewers.

The spike verified:

- `a/b~c` produces escaped pointer `/a~1b~0c` and schema location `/properties/a~1b~0c/type`.
- `contains` with `minContains` rejects the expected under-count case.
- A JavaScript-style positive lookahead pattern compiles and validates with jsonschema’s default pattern engine.
- The same lookahead pattern fails to compile in boon 0.6.1, despite boon’s documented ECMA compatibility for its supported translated subset. This is a specific observed limitation, not a blanket claim that boon is nonconforming.

For regexes, prefer the default enhanced engine with an explicit backtracking/work budget if this syntax is needed. The linear-time `regex` option is attractive for hostile patterns, but rejects lookaround/backreferences. Since artifactize has a new schema format, it may instead deliberately specify the smaller linear-time pattern language. Make that a product decision, not an accidental behavior change. [Pattern options](https://docs.rs/jsonschema/0.58.4/jsonschema/)

Use Schemars only where generation is actually useful. Its default generated draft is currently 2020-12, and its documentation warns that exact schema output can change without a semver-major release. Therefore generated output should be reviewed/snapshotted and should not silently salt content identities on dependency updates. [Schemars documentation](https://docs.rs/schemars/1.2.2/schemars/)

### Risks and maintenance

- Truncating an iterator’s displayed errors does not establish a CPU-time bound for validation itself. Bound payload size, schema depth, regex cost, and diagnostic work separately.
- Boon’s most recent stable release in the authoritative API was **2025-01-07**. A docs/index rendering reported a different date; the registry timestamp was used for this report.
- jsonschema 0.58.4 was published **2026-10-01**, so it is current but very recent. Lock it, keep a representative conformance corpus, and test upgrades.
- jsonschema and Schemars are MIT; boon is MIT OR Apache-2.0.
- There is no reason to reproduce TypeBox’s unescaped-pointer repair workaround when the Rust validator already supplies correct pointers.

## 4. MCP server

### Current transport and behavior

[CCDD’s hand-written MCP server](ccdd@eddf8f7/src/artifacts/mcp-server.ts) is **newline-delimited JSON-RPC over stdin/stdout**. It is not HTTP, SSE, a WebSocket, or an LSP `Content-Length` stream.

It advertises protocol version `2024-11-05`, implements `initialize`, `ping`, `tools/list`, and `tools/call`, ignores notifications, and executes incoming requests serially. The tool registry is dynamically created from a manifest. JSON/launch blocks are rendered as MCP text; images remain image content. Tool failures return `isError: true` with safe text. The declared request-line cap is 64 KiB, although Node readline has already assembled the line before that check.

### Recommendation and comparison

Use **official rmcp 3.5.0**, not an unrelated similarly named MCP crate, and not a new hand-written full JSON-RPC implementation.

A minimal starting selection is:

`rmcp = { version = "3.5.0", default-features = false, features = ["server", "transport-io"] }`

The `server` feature already pulls in Schemars and async read/write support; disabling macros does not remove Schemars entirely. Dynamic schemas are supported: `Tool::new` accepts a JSON object schema, and the spike compiled a tool constructed from runtime JSON. There is no need to generate a Rust type or macro invocation for each user-declared tool.

The SDK uses Tokio and has a per-request cancellation token. Pass cancellation into artifactize’s actual supervisor and audit lifecycle; protocol cancellation alone must not leave child work running. [rmcp documentation](https://docs.rs/rmcp/3.5.0/rmcp/)

Keep application policy outside the SDK:

- Manifest/scope verification and dynamic schema validation.
- Mandatory audit-before-return and required observation semantics.
- Safe application error mapping, including authored errors versus operational failures.
- Bounded concurrency. To preserve a simple ordered observation audit, initially serialize tool execution per review even if the SDK accepts concurrent requests.
- Tool allowlists and minimal capabilities: do not enable prompts/resources/tasks/auth/HTTP transports just because the SDK supports them.
- Logs exclusively on stderr; stdout belongs to the MCP protocol.

**Important source-level finding:** rmcp 3.5.0’s ordinary `AsyncRwTransport` accumulates incoming lines with `read_until` and does not set an inbound line-length limit. A separate `JsonRpcMessageCodec::new_with_max_length` exists, but selecting ordinary `stdio()` is not equivalent to using that bounded decoder. Add/test a bounded framing transport or a bounded reader adapter. Limit incoming requests independently of outgoing image-sized responses.

SDK behavior also differs from CCDD’s hand-written parser for malformed input and EOF handling. Since artifactize is a new format, follow the selected protocol/SDK semantics and write protocol tests rather than reproducing every CCDD parser quirk.

### Risks and maintenance

rmcp 3.5.0 was published **2026-09-28**, is **Apache-2.0**, and is maintained in the official `modelcontextprotocol/rust-sdk` repository. The inspected source includes legacy `2024-11-05` and newer version constants. Its API/protocol evolution is active: lock the release and test initialization, cancellation, errors, and tool schemas against target clients.

A small bounded-framing adapter is justified; maintaining a second full MCP protocol implementation is not.

## 5. Generic command execution

### What to retain from CCDD, and what to discard

Primary implementations:

- [Process supervisor](ccdd@eddf8f7/src/executors/process.ts).
- [Runtime executor and Node-only guard](ccdd@eddf8f7/src/executors/index.ts).
- [Script tool runner](ccdd@eddf8f7/src/tools/runner.ts).
- [Readiness/identity/result-check process executor](ccdd@eddf8f7/src/tools/environment.ts).
- [Admission launch host](ccdd@eddf8f7/src/executors/launch-host.ts).

CCDD script tools already use fixed executable/argv, no shell interpolation, owner-folder cwd, JSON stdin, one JSON stdout result, external output/temp/home/cache directories, bounded output, cancellation, and timeout. Ordinary Runtime Critics, by contrast, are restricted to `node --test` with safe test-path arguments.

**Discard that Runtime restriction for artifactize.** Define an ordinary runtime profile such as `{command, args, timeout}`:

- exit 0 → GREEN;
- normal nonzero exit → RED;
- failure to spawn, timeout, cancellation, signal/abnormal termination, or supervisor failure → ERROR;
- all retained stdout/stderr are audit data, not an alternative source of verdict semantics.

This is consistent with the user’s direction and much more general than teaching the Rust engine every test runner.

### Crates and architecture

Use **tokio::process** plus **process-wrap 10.0.1** (`tokio1` frontend). It is the maintained successor to `command-group`; the older `command-group 5.0.1` release dates to 2023. process-wrap supports Unix process groups/sessions and Windows Job Objects. Version 10.0.1’s Job Object wrapper starts the child suspended while assigning it to the job, avoiding the ordinary create-then-attach race. [process-wrap](https://docs.rs/process-wrap/10.0.1/process_wrap/)

Implement one supervisor with adapters for distinct output contracts:

| Invocation kind | Exit/output meaning |
|---|---|
| Runtime Critic | Normal exit code determines GREEN/RED; process failures are ERROR. |
| Script tool | Exit 0 plus exactly one validated JSON ToolResult; nonzero/malformed output is operational failure. An explicitly authored safe domain error can be a valid ToolResult without becoming evidence. |
| Readiness check | Exit 0 means ready; other exit/timeout/output-limit failures mean not ready, not a review verdict. |
| Owner identity | Exit 0 plus bounded opaque identity output under the new protocol; no fallback to automatic identity on failure. |
| Result check | Exit 0 plus bounded structured errors; a check failure is distinct from a Runtime RED verdict. |

All these adapters are language-neutral. JS, Python, Rust binaries, shell scripts, or other commands can implement the contract. Choosing a shell explicitly as the fixed executable is different from the engine silently adding shell interpolation; document that explicitly configured shells remain trusted user code.

### Paths and argument resolution

Resolve **only explicitly marked path-reference arguments**, such as a complete argv token `{artifact}/relative/path`, through the logical Artifact scope and safe physical-path resolver. Every referenced Artifact must be admitted. Preserve other arguments byte-for-byte, including flags and ordinary strings. Do not heuristically interpret every existing file-looking argument as a scope path.

Initially require path-reference syntax to occupy an entire argv token. If users need `--input={artifact}/file`, define a deliberate structured argument form or exact substitution rule rather than inventing a shell-like language. Keep executable selection fixed by configuration, never by reviewer arguments.

Explicit path resolution is **not a filesystem sandbox**. A trusted command can still read other paths or use the network. It defines which explicit references the engine resolves and audits; it cannot prove what arbitrary code accesses. Require declaration of executable/runtime/config inputs that should affect identity.

Use ordinary PATH resolution, optionally **which 8.0.6**, after deciding the policy. CCDD’s special treatment of `node` and upward `node_modules/.bin` search is npm-specific and need not survive in the new format. Relative executable paths should have a clearly defined base, preferably the owner directory. Test Windows `.exe`/PATHEXT behavior separately; avoid silently invoking `.cmd`/`.bat` through a shell with different quoting semantics.

### Environment and output

Construct environments using `env_clear()` plus deliberate allowlists and engine-controlled paths. Preserve essential OS launch variables and configured locale. Put workspace/output/tmp/home/cache variables under the new artifactize namespace; keep temporary state outside reviewed input. Set language-specific output directories only where helpful and documented, for example `CARGO_TARGET_DIR`; a generic environment cannot stop a build tool from writing its default cache unless that tool honors the supplied configuration.

There is a meaningful distinction in CCDD worth preserving as policy, not blindly copying: tool/Runtime processes get private HOME/cache directories, whereas environment checks keep the real HOME and selected developer-tool locations to inspect installed prerequisites. Readiness may need a different explicit environment policy from evidence-producing tools. Never blanket-inherit credentials, preload variables, or provider authentication state.

Read stdout and stderr concurrently while writing stdin, with independent byte counters. A child that fills stderr while the parent only reads stdout is a classic deadlock. Use bounded raw byte buffers and deliberate UTF-8 policy; preserve arbitrary binary data only in explicitly supported encodings. Strip terminal escape/control sequences for displayed process audit, without treating sanitation as an excuse to log secrets.

For protocol commands, oversize output must fail rather than parsing a truncated prefix. For Runtime diagnostics, bounded capture with an explicit truncated flag is reasonable. Drain or close pipes according to supervisor policy; never use unbounded `wait_with_output()` for arbitrary tools.

### Cancellation and process-tree cleanup

Tokio does not kill a child merely because its handle/future is dropped. `kill_on_drop` is useful defense-in-depth but is not a complete process-tree cleanup protocol. Explicitly signal/kill, await process completion, and reap. [Tokio process caveats](https://docs.rs/tokio/1.53.1/tokio/process/index.html)

For Unix, start a dedicated group/session, send TERM to the group, wait a bounded grace period, then KILL. For Windows, use a Job Object and explicit job termination; do not claim POSIX SIGTERM semantics on Windows. When the leader exits, non-launch commands should not leave surviving descendants.

CCDD has two distinct grace behaviors: the generic process runner escalates after 1 second, while readiness/identity/result-check execution uses 500 ms and then destroys pipes to avoid hanging on a detached descendant. Artifactize should unify the mechanism while making grace/output policy explicit. A timeout on `wait()` alone does not guarantee cleanup, especially when grandchildren inherit pipes.

Retain **admission-before-user-execution**. CCDD’s inert launch host waits until its PID is registered against the resource lease, then launches user code. The Rust equivalent can be a hidden subcommand of the same binary using a private pipe and fixed protocol. process-wrap does not implement this application-level resource-accounting handshake for you.

Unix process groups do not catch a child that deliberately creates a new session. Full adversarial descendant containment would require stronger OS facilities, but that is not the current cooperative-execution contract. Intentional Human application launch/handoff must be a separate policy from ordinary commands that must die with their supervisor.

## 6. Workers, IPC, ownership, cancellation, and runtime choice

### CCDD’s lifecycle

- [Worker client](ccdd@eddf8f7/src/worker-client.ts).
- [Worker entrypoint](ccdd@eddf8f7/src/worker.ts).
- [Process identity/liveness](ccdd@eddf8f7/src/broker/ownership.ts).
- [CLI cancellation](ccdd@eddf8f7/src/cli-cancellation.ts).
- [Execution task-local scope](ccdd@eddf8f7/src/execution-scope.ts).

Despite the phrase “request-scoped worker,” the concrete worker owns **one Run**, potentially containing multiple Critics. It is not one OS process per individual tool call or one process per Critic. It stays alive during Human waiting, maintains workspace monitoring, and does not scan a global queue or accept arbitrary socket requests.

The submitting process launches a detached Node child using Node’s IPC channel, waits up to 15 seconds for ready/error, then unreferences/disconnects it. Readiness is sent after ownership starts. Submitted execution settings are durably saved with a worker protocol version; arbitrary saved fields are rejected. The actual ongoing cross-process control plane is **SQLite**, not the startup IPC channel.

Owner validity uses PID plus a process-start identity and a random ownership token. Linux reads `/proc/<pid>/stat`; other platforms fall back to `ps -o lstart`. Linux also rejects zombies. The current fallback treats unavailable identity information permissively in some paths, which should not be mistaken for strong cross-platform PID-reuse protection.

### Recommendation

Use the same executable with a hidden worker subcommand and a small versioned startup message. A private inherited pipe/stdio channel is enough for ready/error; no general IPC framework, local daemon, Unix socket service, or TCP listener is required. Do not use libc `fork()` inside a multithreaded Tokio process as the conceptual equivalent of Node `fork`; spawn/exec a fresh binary.

Use **Tokio 1.53.1** with explicit process, signal, time, sync, I/O, and runtime features, plus **tokio-util 0.7.19** for `CancellationToken`, task tracking, and codecs where useful.

Why not sync + threads? It is viable, but simultaneous child pipes, deadlines, signals, HTTP requests, MCP requests, filesystem notifications, and long Human waits would require a substantial thread/channel framework. Tokio already provides the needed coordination and is the natural runtime for rmcp/Axum. Conversely, “use Tokio” does not mean putting SQLite or large blocking scans directly on its executor.

Use:

- async tasks for orchestration and I/O;
- dedicated blocking threads for database connections;
- bounded blocking scan/hash jobs for filesystem work;
- explicit cancellation checks in scan loops;
- `std::time::Instant` for deadlines/elapsed time and a wall-clock type only for persisted timestamps.

`spawn_blocking` tasks cannot be forcibly canceled once running, and Tokio’s default blocking pool is intentionally large. Bound CPU/I/O concurrency before starting work; do not assume aborting a handle stops a hash scan or transaction. [Tokio spawn_blocking](https://docs.rs/tokio/1.53.1/tokio/task/fn.spawn_blocking.html)

Replace `AsyncLocalStorage` with **explicit execution context parameters** carrying cancellation, runtime pins, and child tracking. `tokio::task_local!` is an option for diagnostic context, but task locals do not automatically solve propagation across spawned tasks or blocking threads. Durable ownership and diagnostic-only state must not depend on ambient thread/task context.

Shutdown needs an async close/drain path. Rust `Drop` is useful for emergency handle cleanup but cannot await subprocesses, database work, output deletion, or task drains. First cancellation should request orderly cleanup, not immediately exit and abandon children.

Keep PID/start identity and ownership tokens. For native Windows support, add a small platform adapter using Windows process handles/creation times rather than treating absence of `ps` as adequate ownership validation. Treat liveness as alive/dead/unknown where necessary: never reclaim a slot solely because a heartbeat is old or identity lookup failed.

## 7. Filesystem scope, realpath, integrity, and cleanup

### CCDD behavior

- [Supplied workspace rules](ccdd@eddf8f7/src/workspaces/index.ts).
- [Scoped physical paths](ccdd@eddf8f7/src/tools/paths.ts).
- [Declared execution-input paths](ccdd@eddf8f7/src/tools/inputs.ts).
- [Pure logical scope resolver](ccdd@eddf8f7/src/artifact-scope.ts).
- [Explicit prune](ccdd@eddf8f7/src/project/prune.ts).

There are **two different policies**, not one generic recursive walk:

1. Static declaration discovery skips `.git`, `node_modules`, and symlink directories.
2. Default Artifact material hashing excludes separate child Artifacts and installed-runtime directories, with explicit mandatory/declaration/family exceptions.

Reviews use the existing workspace, not a copy. State/output must be outside it even through symlinked ancestors. Logical mounts/family paths are resolved without creating filesystem entries. Reader paths reject symlink traversal. Declared execution runtimes have their own contained-symlink policy.

Input changes during a review are detected by re-running the Artifact's identity command when the review completes ([ccdd#104](https://github.com/lhj6102/ccdd/issues/104)); there is no workspace monitoring.

### Recommendation

- **std::path/std::fs** for path representation, ordinary operations, and canonicalization.
- **rustix 1.1.5**, target-gated on Unix, for descriptor-relative/open flags needed by secure readers and pruning.
- **tempfile 3.27.0** for private engine-owned temporary roots.
- Optional **cap-std 4.0.3** if capability-relative directory APIs are adopted broadly; do not add it merely as a decorative wrapper around pathname operations.
- Optional **dunce 1.0.5** only for Windows-friendly canonical path presentation/interoperability; it is not a containment mechanism.

Implement explicit deterministic traversal. A generic walker can assist discovery, but it cannot replace the symlink policy and entry-type checks. Do not choose an ignore-aware walker whose defaults accidentally omit workspace input.

Distinguish lexical validation from canonical containment. A path can be lexically relative yet escape via symlinks. `canonicalize()` alone is not race-free authorization. For stronger boundaries, open directories relative to pinned parent descriptors with no-follow semantics, verify opened file metadata, and use the resulting handles. Windows requires a corresponding reparse-point/handle policy, not translated Unix flag names.

For a not-yet-existing output path, canonicalize its existing ancestor before creation and revalidate after creation; ensure that writes cannot enter the reviewed root through a symlink. Preserve root/outside checks even when directories are created concurrently.

CCDD’s prune operation is expressly **Linux-only**, using `/proc/self/fd`, no-follow directory opens, private quarantine renames, and device/inode verification. Artifactize can implement descriptor-relative `renameat`/`unlinkat` directly through rustix and potentially extend support later, but retain the safety design: claim only eligible terminal work, move to private quarantine under a short transaction, delete after commit, and preserve suspicious/quarantined data for manual recovery. Never replace this with an unconstrained recursive delete of a joined pathname.

`TempDir` cleanup errors matter when the contract requires successful removal. Use explicit close/remove paths that can report failure, rather than relying exclusively on best-effort destructor cleanup.

## 8. Hashing, canonical JSON, and immutable content identity

### Current use

[Project identity](ccdd@eddf8f7/src/project/identity.ts), the workspace scanner, configuration loader, runtime-input hashing, and immutable store all use **SHA-256**. There is no need to infer an xxHash/BLAKE3 dependency from “content identity.”

Workspace identities include sorted paths, types, file-content digests, executable bits, and relative symlink targets. Metadata identity separately includes inode/device/mode/size/timestamps for mutation detection. SCC identity propagates local/dependency changes while allowing cycles. Timestamps, review IDs, completion times, and package versions must not accidentally become content inputs.

### Recommendation for artifactize’s new format

Use **sha2 0.11.0** and define a single canonical encoding based on **RFC 8785/JCS**, implemented with **serde_json_canonicalizer 0.3.2** after explicit input validation. Use domain-separated envelopes/tags for different hash purposes, such as workspace structure, Artifact local identity, SCC identity, validation input, and immutable record nodes. Give the encoding/hash envelope an artifactize format version.

Define the accepted JSON domain before calling the canonicalizer:

- finite numbers only;
- reject malformed Unicode/lone surrogate escapes;
- decide an IEEE-754/safe-integer policy and reject integers that would lose required precision;
- encode larger exact counters/identifiers as strings when they cross JSON-safe numeric bounds;
- preserve array order;
- normalize negative zero and numeric lexical equivalents according to JCS;
- use JCS property ordering, never map iteration order or locale-dependent collation.

The canonicalizer documents conversion of numbers to doubles; arbitrary-precision input is not preserved by JCS serialization. Do not enable arbitrary precision and then assume hashing remains lossless. [Canonicalizer documentation](https://docs.rs/serde_json_canonicalizer/0.3.2/serde_json_canonicalizer/)

The spike showed ordinary `serde_json` output differing from JCS on `-0.0`, `1.0`, and non-BMP key ordering. JCS produced the intended normalized form. **This is a design-choice validation, not a CCDD migration blocker:** CCDD byte parity is explicitly out of scope. Do not expend effort preserving its multiple ad-hoc serialization paths or UTF-16-size thresholds.

A defined serde_json-based scheme would also work internally, but JCS provides a published cross-language specification and reduces future protocol ambiguity. Lock and test the relatively small canonicalizer implementation. Never change canonicalization implicitly through a dependency update.

Use **uuid 1.26.1 with v4** for random IDs, **getrandom 0.4.3** for cryptographic token bytes, and **hmac 0.13.0** plus SHA-256 for monitor CSRF tokens. Use the HMAC verification API rather than ordinary equality for secret-dependent verification. **base64 0.23.1** covers standard image encoding and URL-safe tokens. No OpenSSL dependency is needed for these operations.

## 9. Graph algorithms and gate propagation

### Current behavior

[Broker graph algorithms](ccdd@eddf8f7/src/broker/graph.ts) implements recursive Tarjan SCC detection. [Project gates](ccdd@eddf8f7/src/project/gates.ts) and the broker readiness tables use the condensation graph’s direct edges rather than a materialized transitive closure.

Cycles do not prove validity. SCC peers share external gating obligations, and all non-basis required members still need actual matching evidence. Local traversal/memoization and persisted reverse gate edges serve different purposes.

### Recommendation

Use **petgraph 0.8.3**, initially with `Graph`/node indices and **`kosaraju_scc`**. The inspected implementation uses explicit DFS state rather than recursive calls and is O(V+E) time, O(V) auxiliary space. This avoids copying CCDD’s recursive implementation into a language where an uncontrolled recursion depth can overflow the stack.

Keep artifactize’s domain graph ownable and serializable; petgraph should be an algorithmic implementation detail, not the persisted format. Build explicit mappings from canonical Artifact IDs to indices. Sort members and dependency inputs before hashing; do not hash library node IDs, incidental SCC output order, or HashMap iteration.

Audit edge direction carefully: CCDD relationships point from dependency/input to consumer, while some traversals invert them. SCC membership survives reversal, but topological evaluation/gating order does not. Build the condensation traversal explicitly.

A small hand-written iterative SCC algorithm is a valid dependency-minimal alternative, but it buys little unless binary/dependency minimization becomes a stated requirement. Keep logical path resolution and evidence/gate semantics custom; petgraph does not implement them.

Petgraph 0.8.3 dates to 2025-09-30, MIT OR Apache-2.0. Its slower release cadence is not, by itself, evidence of abandonment for a mature graph library.

## 10. CLI, HTTP, and remaining networking built-ins

### CLI

[Project CLI](ccdd@eddf8f7/src/project/cli.ts) manually parses commands/options, rejects duplicate/unsupported flags, selects Artifacts/Critics, produces compact versus full JSON, and preserves important exit-code semantics: 0 fulfilled/accepted, 1 RED, 2 ERROR, 3 wait timeout, 4 incomplete. Waiting timeout does not cancel background execution.

Use **clap 4.6.7** with derive or builder APIs for nested commands, mutually exclusive selections, typed values, and help. Preserve semantic differences between “accepted” and “finished.” A parser library’s default success/error exits should not overwrite artifactize’s domain exit codes. Keep stdout machine-clean under `--json`; diagnostics go to stderr.

No line-editing crate is needed to replace `node:readline`: in this core, readline is used for the MCP line stream, not an interactive REPL. Use Tokio buffered I/O/codecs. `node:util` deep equality becomes typed equality or `serde_json::Value` comparisons where appropriate; no generic JavaScript object utility replacement is required.

For `node:os`, ordinary `std::env`, `available_parallelism`, platform APIs, and explicit configured locations suffice. Whether artifactize adopts platform-native config/state directories versus existing XDG-like layouts is a product decision, not a reason to add multiple directory crates automatically.

### Loopback HTTP

[Monitor HTTP server](ccdd@eddf8f7/src/monitor/server.ts) is an ordinary HTTP/JSON server bound to `127.0.0.1`; default port 4318, with port 0 supported. It is not an MCP HTTP transport and does not require TLS.

The important requirements are behavioral/security-related:

- exact Host and same-origin checks, including POST Origin and Fetch Metadata checks;
- no broad CORS policy;
- GET/POST method restrictions and rejection of GET bodies;
- duplicate/unknown query parameter checks;
- 32 KiB streaming JSON-body limit and a 5-second body-read deadline;
- server/header deadlines and bounded concurrent explicit inspections/tools;
- opaque HttpOnly/SameSite=Strict cookie, reviewer identity, HMAC-derived CSRF token;
- browser reviewer identity derived from the server session, not a caller-supplied reviewer name;
- security headers and no-store responses;
- cancellation on disconnection/shutdown;
- read-only GETs, with current-input preparation and Human mutations delegated by explicit POSTs.

Use **Axum 0.8.9** on Tokio. It provides routing, extractors, typed state, and response handling, while **tower-http 0.7.1** can provide global body limits, timeout layers, and selected headers. Bare Hyper is possible but would recreate more of CCDD’s hand-written routing/error plumbing. A synchronous server would complicate shared async process/cancellation behavior.

Axum’s default extractor body limit is **2 MiB**, not CCDD’s 32 KiB; configure it deliberately. It applies only to participating extractors, whereas a tower-http body-limit layer can protect all routes. [Axum body limit](https://docs.rs/axum/0.8.9/axum/extract/struct.DefaultBodyLimit.html)

Axum’s default convenience server is not automatically an exact match for all low-level Node header/connection timeout settings. If those limits are required, configure the underlying Hyper connection builder through a thin server adapter. A request-future timeout also does not kill a child unless the handler forwards a cancellation token to the supervisor.

Do not use lossy query extraction that silently discards repeated keys where the contract rejects duplicates. Do not assume “loopback only” eliminates DNS-rebinding/CSRF threats. Do not expose filesystem previews or static serving of arbitrary workspace paths.

### Why `tls`, `https`, and `net` appear

Outside provider code, those imports are in **the offline guard**, where Node connection APIs are replaced with throwing functions. Their presence does **not** mean the core requires a TLS client/server implementation, certificate store, or general socket protocol stack.

`node:http` is needed for the monitor and also patched by the guard. `node:url` mostly supports file/module location and URL parsing. Artifactize can use the current executable/embedded resources for its own helper entrypoints, and Axum/HTTP URI utilities for routes. Add `url 2.5.8` only if broader standards-compliant URL handling is actually needed, not simply because Node imported `URL`.

## 11. Offline load-check: remove the monkey-patch guard

### Actual CCDD scope

- [Offline guard](ccdd@eddf8f7/src/offline-guard.ts).
- [Load-check launcher/driver](ccdd@eddf8f7/src/project/load-check.ts).
- [Environment import host](ccdd@eddf8f7/src/tools/environment-host.ts).

The guard is activated by **load-check**, the synthetic performance benchmark. It blocks imports of the real provider/auth modules, patches `fetch`, HTTP(S) request/get, TCP connection entrypoints, and TLS connect, sets an activation marker, and can append a proof record. The benchmark propagates it to selected Node children through explicit imports/NODE_OPTIONS.

It is **not enabled for real reviews or doctor**. Its purpose is preventing accidental real-provider use during synthetic load measurement, not enforcing a general offline execution boundary. The contracts explicitly call it a tripwire; DNS promises/resolvers, UDP, native code, and arbitrary children are not a process-tree network guarantee.

The separate environment import host restricts Node module imports to snapshot files/Node builtins and supplies a `.js`→`.ts` fallback. It is not a filesystem sandbox either. Generic language-neutral commands cannot inherit this Node-specific import-hook model.

### Recommendation

**Remove both the need for Node hooks and the offline monkey-patch design.**

Make synthetic benchmarking structural:

- a synthetic executor/provider type that does not accept a real provider client or credential loader;
- preferably a separate benchmark entrypoint/module dependency graph where provider construction is unavailable;
- explicit diagnostic-only state that normal evidence readers reject;
- isolated temporary benchmark state/output and machine-admission configuration unless shared-resource benchmarking is explicitly selected;
- no live provider/account readiness probe from this path;
- tests asserting that credential loading/provider factories are unreachable or never invoked.

A fake provider returning synthetic data must never be exposed as real review evidence. Keep that distinction in persistent state, not only in an in-memory flag.

Real doctor/provider readiness remains an explicitly online capability handled by the separate provider research. Static readiness can inspect executables/declarations without executing scripts; script-executing readiness/identity/result checks remain ordinary trusted commands under the generic supervisor.

**Considered, not needed:** network namespaces, Landlock, macOS `sandbox-exec`, and Windows AppContainer would introduce substantial platform policy to solve a broader problem than the benchmark has. Landlock 0.4.7 only exposes through ABI 9 and is not by itself complete arbitrary-command network denial; `sandbox-exec` is deprecated; Windows Job Objects do not provide network isolation. Do not add any of these crates/adapters for load-check. [Landlock](https://docs.rs/landlock/0.4.7/landlock/), [macOS man page](https://keith.github.io/xcode-man-pages/sandbox-exec.1.html), [Windows AppContainer](https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation)

An environment variable may announce “synthetic benchmark,” but is advisory, not enforcement. If a benchmark intentionally runs arbitrary project commands, those commands can still use the network; do not label the benchmark a no-network sandbox. Structural exclusion guarantees no real provider construction by **artifactize’s benchmark code**, not the behavior of arbitrary subprocesses.

## 12. Additional nontrivial responsibilities

### Default tools: do not add large media/text stacks unnecessarily

[Bounded reader](ccdd@eddf8f7/packages/default-tools/src/reader.ts) streams whole LF/CRLF lines, retains only the requested region, validates UTF-8 strictly, preserves line bytes, rejects NUL/binary content, and caps returned bytes. It does not require transcoding arbitrary encodings.

Use buffered byte I/O plus `std::str::from_utf8`, with careful tests for BOM, CRLF, invalid UTF-8 in unrequested versus requested regions, empty files, and oversized single lines. **encoding_rs is unnecessary** unless the product explicitly adds other encodings.

[Image reader](ccdd@eddf8f7/packages/default-tools/src/image.ts) bounds reads to 4 MiB, checks PNG/JPEG/WebP signatures, and excludes animated PNG. It does not resize/render/decode images. The runner’s image checks are lighter than a full decoder. Therefore **base64 plus small bounded format checks** is sufficient for equivalent functionality; do not add `image` merely because image blocks exist. If artifactize wants full image validation or transforms, `image 0.25.10` is the relevant candidate, with only selected codecs enabled and decoded-dimension/resource limits. That would be added functionality, not a required Node replacement.

Default desktop open is a fixed external command (macOS `/usr/bin/open` unless explicitly configured). This is a command adapter, not a reason to choose a Rust GUI framework. Its receipt is not a content observation or verdict.

### Runtime pinning and provenance

[Execution provenance](ccdd@eddf8f7/src/provenance.ts) is an important exception to “never copy the workspace”: CCDD copies **declared execution runtime material only** into an external content-addressed runtime store, verifies before/after, remaps selected command arguments, and records raw/structural hashes. It does not copy the whole reviewed input.

Artifactize can use std filesystem operations, SHA-256, and the same canonical/input resolver primitives for this. Preserve atomic publication and verification; do not relabel an unverified filesystem copy as immutable. With generic command execution, choose explicitly how executable binaries and interpreter/runtime files are declared and pinned.

### Configuration and families

[Configuration discovery](ccdd@eddf8f7/src/broker/config.ts) performs static JSON parsing, unknown-field checks, identifier/path validation, up to 10,000 family instances, shallow parameter merges, and RFC 6901-style parameter substitution. It never imports configuration code.

Use Serde for artifactize-owned typed config, `deny_unknown_fields` where appropriate, bounded `serde_json::Value` for dynamic schemas/parameters, and custom validation for cross-field invariants. No configuration framework, JS interpreter, expression engine, or general template engine is needed. Preserve the distinction between static discovery and explicit execution of owner identity/preparation scripts.

### Errors, diagnostics, telemetry, and time

Use **thiserror 2.0.21** for typed domain/operational errors and **tracing 0.1.44 + tracing-subscriber 0.3.23** for diagnostic instrumentation. `anyhow` is optional at binary boundaries, not a substitute for typed error classes that determine ERROR/RED/INCOMPLETE behavior or whether text may cross a credential-safe boundary.

Mandatory observation/audit persistence and best-effort telemetry must remain separate operations. A failed audit cannot silently return successful observed content. A failed optional timing sink should not change semantic evidence. No prompts, raw tool output, credentials, or hidden reasoning should be inserted into generic tracing spans.

Use `Instant` for monotonic lease/deadline bounds and a timestamp crate such as **time 0.3.55** for persisted RFC 3339 UTC timestamps. A wall-clock rollback must not extend a coalescing lease indefinitely. Clock abstractions are worthwhile for deterministic tests.

## 13. Actual operating-system scope

CCDD has cross-platform branches for Linux/macOS/Windows, historical Windows performance runs and Windows path fixes, and historical macOS verification. However, that is not equivalent to a fully verified uniform platform guarantee:

- [Current CI](ccdd@eddf8f7/.github/workflows/ci.yml) runs on Ubuntu.
- Explicit prune is Linux-only.
- Linux has stronger `/proc`-based process identity/group cleanup.
- Non-Linux process identity relies on `ps`, which is not a reliable native-Windows implementation.
- Windows branches often kill a direct child rather than guaranteeing the whole descendant tree.
- The default Human desktop opener is macOS-only; other platforms require explicit commands.

Recommendation: make artifactize’s support matrix explicit. **Linux first is the lowest-risk complete-engine target**, macOS is feasible with dedicated process/permission tests, and native Windows needs deliberate Job Object, process identity, reparse-point, executable-resolution, and cleanup work. WSL is Linux execution with filesystem caveats, not a native-Windows verification substitute. Do not advertise Windows support just because all dependencies compile there.

## 14. Testing strategy and spike results

Replace Node’s test runner for the engine with **`cargo test`**. This is independent of runtime evals: a runtime eval may run any configured test CLI.

Recommended layers:

1. Pure unit tests for selection, scope resolution, schema allowlists, canonicalization, SCCs, evidence projection, and state transitions.
2. **proptest** for graph insertion-order independence, cycles/self-loops, family parameter isolation, path parsing, JSON Pointer escaping, and canonical hashing properties.
3. **insta** for reviewed stable JSON/CLI/schema output snapshots; redact only deliberate nondeterminism such as IDs/timestamps, not actual semantic data.
4. **assert_cmd** integration tests for exit codes, stdout/stderr separation, invalid flags, cancellation, hidden-worker protocol, and generic commands.
5. Real subprocess fixtures for full pipes, malformed/oversize output, inherited pipes, descendants, SIGTERM escalation, leader exit, and parent death. Mocking `Command` cannot establish process cleanup.
6. Multiprocess temporary SQLite tests for concurrent first-open, lock contention, coalescing/admission races, claim CAS, worker death, canceled publication, cursor rollback, cache invalidation, and WAL checkpoint pressure. An in-memory database is insufficient for these.
7. Real filesystem tests for symlink swaps, special files, permission/executable changes, edit-and-restore, create-and-delete, metadata-only limitations, and scan-generation freshness.
8. Protocol tests for MCP initialization/framing/errors/cancellation and loopback HTTP Host/Origin/CSRF/body limits/disconnection/readonly GET behavior.
9. Linux/macOS/Windows CI jobs for declared supported behavior, with explicit unsupported cases rather than broad silent skips.
10. Optional **Criterion 0.8.2** microbenchmarks plus an end-to-end synthetic load-check. Measure scheduler work/hydrated bytes/queue delay, not just a faster isolated helper, and keep synthetic state unusable as evidence.

Do not run real provider tests in default offline CI. Those belong to explicit provider integration suites owned by the other research track.

### Throwaway spike performed

Files:

- `(session scratchpad, not kept)/spikes-core/Cargo.toml`
- `(session scratchpad, not kept)/spikes-core/src/main.rs`
- `(session scratchpad, not kept)/spikes-core/Cargo.lock`

Compiled and ran on Linux/WSL with **Rust 1.98.1**. The tested dependency combination included rusqlite 0.40.2, tokio-rusqlite 0.8.0, jsonschema 0.58.4, rmcp 3.5.0, process-wrap 10.0.1, petgraph 0.8.3, and serde_json_canonicalizer 0.3.2.

Observed results:

- Dynamic rmcp tool construction compiled without per-tool derives.
- Schema instance/schema pointers correctly escaped `/` and `~`.
- `contains` count constraints worked in the chosen draft.
- A lookahead regex worked in jsonschema and was rejected at compile time by boon.
- Bundled SQLite was 3.53.2 and `json_extract` worked.
- With a writer holding `BEGIN IMMEDIATE`, a second connection read the old committed WAL value; its competing write returned busy after the configured approximately 30 ms; after commit it saw the new value.
- JCS normalized selected numeric/key-order examples; serde_json rejected a lone-surrogate string.

These are **focused compatibility/functionality checks**, not full conformance, performance, process-tree, or cross-platform validation. The WAL test used two connections in one process, so the required multiprocess stress tests remain implementation work. No product code was written.

## 15. Proposed dependency list

Versions below are **verified highest non-yanked stable releases available by 2026-10-03**, not a claim that every newest release should be upgraded automatically. Links point to authoritative version metadata. Commit a lockfile and review upgrades. The successful spike is not an MSRV test; pin an actual tested toolchain initially and derive the supported MSRV from the complete resolved dependency graph.

### Recommended production dependencies

| Crate | Version | Purpose | License |
|---|---:|---|---|
| [tokio](https://crates.io/api/v1/crates/tokio/1.53.1) | 1.53.1 | Async orchestration, process I/O, timers, signals, networking | MIT |
| [tokio-util](https://crates.io/api/v1/crates/tokio-util/0.7.19) | 0.7.19 | Cancellation tokens, task tracking/codecs | MIT |
| [rusqlite](https://crates.io/api/v1/crates/rusqlite/0.40.2) | 0.40.2 | SQLite state/admission; bundled SQLite and statement cache | MIT |
| [tokio-rusqlite](https://crates.io/api/v1/crates/tokio-rusqlite/0.8.0) | 0.8.0 | Dedicated-thread async bridge for complete SQLite operations | MIT |
| [serde](https://crates.io/api/v1/crates/serde/1.0.229) | 1.0.229 | Typed config/protocol/state serialization | MIT OR Apache-2.0 |
| [serde_json](https://crates.io/api/v1/crates/serde_json/1.0.151) | 1.0.151 | JSON parsing/values/serialization | MIT OR Apache-2.0 |
| [jsonschema](https://crates.io/api/v1/crates/jsonschema/0.58.4) | 0.58.4 | Dynamic schemas; disable default remote/file resolution features | MIT |
| [rmcp](https://crates.io/api/v1/crates/rmcp/3.5.0) | 3.5.0 | Official MCP server, stdio transport | Apache-2.0 |
| [process-wrap](https://crates.io/api/v1/crates/process-wrap/10.0.1) | 10.0.1 | Tokio process groups/sessions and Windows Job Objects | MIT OR Apache-2.0 |
| [rustix](https://crates.io/api/v1/crates/rustix/1.1.5) | 1.1.5 | Target-gated descriptor-relative filesystem operations | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [tempfile](https://crates.io/api/v1/crates/tempfile/3.27.0) | 3.27.0 | Private temporary output roots | MIT OR Apache-2.0 |
| [sha2](https://crates.io/api/v1/crates/sha2/0.11.0) | 0.11.0 | SHA-256 content/identity hashes | MIT OR Apache-2.0 |
| [serde_json_canonicalizer](https://crates.io/api/v1/crates/serde_json_canonicalizer/0.3.2) | 0.3.2 | RFC 8785/JCS canonical hash input | MIT |
| [uuid](https://crates.io/api/v1/crates/uuid/1.26.1) | 1.26.1 | Random v4 Run/request/attempt IDs | Apache-2.0 OR MIT |
| [getrandom](https://crates.io/api/v1/crates/getrandom/0.4.3) | 0.4.3 | OS cryptographic randomness for tokens | MIT OR Apache-2.0 |
| [base64](https://crates.io/api/v1/crates/base64/0.23.1) | 0.23.1 | Image transport and URL-safe tokens | MIT OR Apache-2.0 |
| [petgraph](https://crates.io/api/v1/crates/petgraph/0.8.3) | 0.8.3 | Iterative SCC/condensation support | MIT OR Apache-2.0 |
| [clap](https://crates.io/api/v1/crates/clap/4.6.7) | 4.6.7 | CLI commands, arguments and help | MIT OR Apache-2.0 |
| [thiserror](https://crates.io/api/v1/crates/thiserror/2.0.21) | 2.0.21 | Typed safe error boundaries | MIT OR Apache-2.0 |
| [tracing](https://crates.io/api/v1/crates/tracing/0.1.44) | 0.1.44 | Structured operational diagnostics | MIT |
| [tracing-subscriber](https://crates.io/api/v1/crates/tracing-subscriber/0.3.23) | 0.3.23 | Diagnostic output/filtering in binaries | MIT |
| [time](https://crates.io/api/v1/crates/time/0.3.55) | 0.3.55 | UTC/RFC 3339 stored timestamps | MIT OR Apache-2.0 |

### Conditional production dependencies

| Crate | Version | Add when | License |
|---|---:|---|---|
| [axum](https://crates.io/api/v1/crates/axum/0.8.9) | 0.8.9 | Building the loopback HTTP adapter | MIT |
| [tower-http](https://crates.io/api/v1/crates/tower-http/0.7.1) | 0.7.1 | HTTP body-limit/timeout/header middleware | MIT |
| [hmac](https://crates.io/api/v1/crates/hmac/0.13.0) | 0.13.0 | Loopback browser CSRF session tokens | MIT OR Apache-2.0 |
| [schemars](https://crates.io/api/v1/crates/schemars/1.2.2) | 1.2.2 | Directly generating artifactize-owned schemas; already transitive through rmcp server | MIT |
| [which](https://crates.io/api/v1/crates/which/8.0.6) | 8.0.6 | Deliberate cross-platform PATH executable resolution | MIT |
| [windows-sys](https://crates.io/api/v1/crates/windows-sys/0.61.2) | 0.61.2 | Native Windows ownership/file-handle adapter | MIT OR Apache-2.0 |
| [cap-std](https://crates.io/api/v1/crates/cap-std/4.0.3) | 4.0.3 | Broad adoption of capability-relative filesystem APIs | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| [dunce](https://crates.io/api/v1/crates/dunce/1.0.5) | 1.0.5 | Windows path normalization/presentation convenience | CC0-1.0 OR MIT-0 OR Apache-2.0 |
| [rusqlite_migration](https://crates.io/api/v1/crates/rusqlite_migration/2.6.0) | 2.6.0 | Future explicit incremental migrations, if wanted | Apache-2.0 |
| [image](https://crates.io/api/v1/crates/image/0.25.10) | 0.25.10 | Full decoding/transforms, not simple bounded image transport | MIT OR Apache-2.0 |

Do not add SQLx alongside rusqlite without a compelling reason; multiple SQLite wrappers can introduce `libsqlite3-sys` linkage/version conflicts. Do not add TLS/OpenSSL, Landlock/seccomp, a JS runtime, an ORM, or a graph-layout library for the responsibilities covered here.

### Development dependencies

| Crate/tool | Version | Purpose | License |
|---|---:|---|---|
| Cargo test | Rust toolchain | Unit/integration/doc tests | Toolchain |
| [insta](https://crates.io/api/v1/crates/insta/1.48.0) | 1.48.0 | Reviewed JSON/CLI/schema snapshots | Apache-2.0 |
| [assert_cmd](https://crates.io/api/v1/crates/assert_cmd/2.2.2) | 2.2.2 | CLI/subprocess integration tests | MIT OR Apache-2.0 |
| [proptest](https://crates.io/api/v1/crates/proptest/1.11.0) | 1.11.0 | Graph/path/schema/canonicalization properties | MIT OR Apache-2.0 |
| [criterion](https://crates.io/api/v1/crates/criterion/0.8.2) | 0.8.2 | Optional controlled microbenchmarks | Apache-2.0 OR MIT |

Alternative comparison metadata: [SQLx 0.9.0](https://crates.io/api/v1/crates/sqlx/0.9.0), [boon 0.6.1](https://crates.io/api/v1/crates/boon/0.6.1), [command-group 5.0.1](https://crates.io/api/v1/crates/command-group/5.0.1). Version/date claims above use the registry metadata rather than the initial web-search snippets, which were stale for several crates.

All selected top-level crates have permissive licenses. This is not a complete transitive license audit: generate an SBOM/license inventory for the resolved lockfile, especially platform-specific dependencies and bundled SQLite code.

## 16. Open questions for the user

1. **Supported platforms:** Is v1 Linux/WSL-first, Linux+macOS, or native Windows too? This determines how much process identity, handle-based filesystem safety, and cleanup work must precede release.
2. **Path-reference syntax:** Should generic command arguments use whole-token `{artifact}/path` references, or a typed argv schema supporting literal and path arguments? Are embedded flag substitutions required?
3. **Executable/runtime identity:** Must every executable/runtime be explicitly declared and content-pinned, or may ordinary installed PATH programs be treated as external environment prerequisites? How should a changed installed compiler/test runner affect evidence reuse?
4. **Schema dialect:** Retain a restrictive no-`$ref` subset with enhanced regex syntax, or intentionally choose linear-time regexes and a smaller language? Should owner response schemas for runtime evals remain verdict-only, or should a separate structured-result Runtime adapter exist?
5. **Environment policy:** Which developer environment variables and actual HOME/config locations may readiness checks see, versus private HOME/cache for ordinary tools and Runtime execution?
7. **Persistence policy:** Should the first artifactize release reject old artifactize state on incompatible schema versions, or establish incremental migrations immediately? Is power-loss durability required for every accepted observation/result?
8. **Human program lifetime:** Which tool kinds may intentionally hand off a long-running GUI/application process rather than kill descendants after command completion?
9. **Synthetic benchmark commands:** Will load-check execute only generated synthetic tools, or may it invoke real project scripts? Either is possible, but only structural exclusion of artifactize’s real provider clients is guaranteed; arbitrary command networking is not blocked.
10. **HTTP scope:** Should the loopback HTTP adapter be a separately enabled feature now, or deferred with the desktop monitor? The engine interfaces should support it without choosing a UI stack.

## Sources

Primary crate version/license sources are the exact crates.io API links in the dependency tables. Principal behavioral references used:

- [Rusqlite 0.40.2](https://docs.rs/rusqlite/0.40.2/rusqlite/)
- [Tokio-rusqlite 0.8.0](https://docs.rs/tokio-rusqlite/0.8.0/tokio_rusqlite/)
- [SQLx SQLite connection options](https://docs.rs/sqlx/0.9.0/sqlx/sqlite/struct.SqliteConnectOptions.html)
- [SQLite WAL](https://www.sqlite.org/wal.html)
- [SQLite data_version](https://www.sqlite.org/pragma.html#pragma_data_version)
- [jsonschema 0.58.4](https://docs.rs/jsonschema/0.58.4/jsonschema/)
- [boon 0.6.1](https://docs.rs/boon/0.6.1/boon/)
- [Schemars 1.2.2](https://docs.rs/schemars/1.2.2/schemars/)
- [Official rmcp 3.5.0](https://docs.rs/rmcp/3.5.0/rmcp/)
- [Official Rust MCP SDK repository](https://github.com/modelcontextprotocol/rust-sdk/)
- [Tokio process lifecycle](https://docs.rs/tokio/1.53.1/tokio/process/index.html)
- [Tokio blocking-task limitations](https://docs.rs/tokio/1.53.1/tokio/task/fn.spawn_blocking.html)
- [process-wrap 10.0.1](https://docs.rs/process-wrap/10.0.1/process_wrap/)
- [rustix descriptor-relative open](https://docs.rs/rustix/1.1.5/rustix/fs/fn.openat.html)
- [JCS canonicalizer](https://docs.rs/serde_json_canonicalizer/0.3.2/serde_json_canonicalizer/)
- [Axum 0.8.9](https://docs.rs/axum/0.8.9/axum/)
- [Axum body limits](https://docs.rs/axum/0.8.9/axum/extract/struct.DefaultBodyLimit.html)
- [Landlock crate, considered but not recommended here](https://docs.rs/landlock/0.4.7/landlock/)
- [macOS sandbox-exec deprecation](https://keith.github.io/xcode-man-pages/sandbox-exec.1.html)
- [Windows AppContainer isolation](https://learn.microsoft.com/en-us/windows/win32/secauthz/appcontainer-isolation)

The relevant published crate tarballs were also inspected directly for dependency/features and source-level behavior, including rmcp framing, process-wrap groups/jobs, boon regex translation, petgraph SCC iteration, and the bundled SQLite version.