# Code checklist

Rules for Rust code. Items marked `M-` come from
Microsoft's [Pragmatic Rust Guidelines](https://microsoft.github.io/rust-guidelines/guidelines/checklist/);
`T-` (type-driven) and `F-` (functional) are this checklist's own.

## Verification and safety

- **M-STATIC-VERIFICATION**: `cargo fmt`, and `cargo clippy --all-targets -- -D warnings`, pass.
- **M-LINT-OVERRIDE-EXPECT**: a lint override uses `#[expect(lint, reason = "…")]`, never `#[allow]`.
- **M-UNSAFE**: `unsafe` has a reason, a `// SAFETY:` comment, and is avoided when a safe API exists.
- **M-PANIC-ON-BUG**: a programming bug panics; a failure the program can expect (I/O, input, the network) is an error.

## Names and structure

- **M-DOCUMENTED-MAGIC**: a limit, size or other magic value is a named constant with a comment saying why.
- **M-WEASEL-WORDS / M-SHORT-NAMES**: names are short and concrete, without words like Manager, Helper or Util.
- **M-BALANCED-MODULES**: modules are balanced in size and scope; a module that grows past one concern is split.

## Types (type-driven)

- **M-STRONG-TYPES**: values use the proper type, such as `PathBuf` for paths and `Duration` for time.
- **M-STRONG-TYPES-GUARD / T-NEWTYPE-IDS**: identifiers (IDs, keys, hashes) are newtypes that guard their invariants.
- **T-ILLEGAL-STATES**: a state is an enum, not a string or a set of booleans, so that illegal states cannot be written.
- **T-PARSE-AT-EDGE**: external input (configuration, CLI arguments, database rows, network responses) is parsed into types once, at the edge. The core does not pass `serde_json::Value` around.

## Functions (functional)

- **F-PURE-CORE**: computations (keys, hashes, decisions, summaries) are pure functions; effects (the database, processes, the network, files) stay at the edges.
- **F-EXPRESSIONS**: iterator transformations and expressions are preferred over mutable accumulation loops where they read better.
- **F-IMMUTABLE**: bindings are immutable by default, `mut` scopes are small, and shared mutable state is explicitly synchronized.

## Code written with AI

- **M-RUST-SHAPED**: the code solves the problem the way Rust does, not by translating another language.
- **M-TAUTOLOGICAL-TESTS**: tests check behavior against independent expectations, not by restating the implementation.
