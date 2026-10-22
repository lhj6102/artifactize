# Runtime

A Runtime eval runs an ordinary program. Use it for checks whose answer is already
decided by a test, linter, renderer or other deterministic command. No model or API
key is needed.

```toml
name = "library"

[evals.tests]
title = "The library tests pass"
profile = { kind = "runtime", command = "cargo", args = ["test", "--locked"], timeout_ms = 240000 }
payload.instruction = "Run the tests of {library}."
```

`command` and `args` are separate: artifactize starts an argument vector, not a
shell command. The working directory is the Artifact folder, or the containing
folder for a file Artifact. Programs use the author's normal toolchain environment,
with artifactize's caller variables withheld and private output and temporary
space outside the repository. See [Operating systems](others/platforms.md) for
executable lookup and portable commands.

Exit 0 is **GREEN**; another exit code is **RED**. Failure to start, a timeout,
interruption or invalid inputs produce operational errors instead. Runtime commands do not return Agent/Human owner fields. Saved results include the
exit code and bounded stdout/stderr after completion. While execution is still
appending output, `artifactize request logs REQUEST_ID` can read bounded snapshots;
unconfirmed captures are explicitly marked partial. This is snapshot inspection,
not an automatic streaming log feed. The [runtime-log CLI reference](../cli/index.md#agent-streaming-and-runtime-logs)
explains tail/range/search and capture evidence; the shared
[runtime execution reference](declarations.md#runtime-execution) owns
retention details, including the raw captures current pruning leaves intact. The CLI, [TUI](../tui/index.md) and [Desktop](../desktop/index.md)
read the same saved evidence.

References such as `{spec}/requirements.json` in the declared operands admit those
inputs. If a file its fingerprint covers changes while the command runs, the result is
ERROR (`INPUT_CHANGED`) instead of a verdict; files outside the fingerprint are not
checked, and an eval without reuse (`fingerprint = false`) is not rechecked. A later Run reuses
an unchanged result, including a RED result, rather than rerunning the command.
See [Fingerprint](fingerprint.md) and the [runtime execution reference](declarations.md#runtime-execution)
for exact limits, environment, substitution and process cleanup.
