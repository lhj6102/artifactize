# Hey-arti harness

The harness keeps **source fixtures** separate from builder outputs and live
quality evidence. `cases.toml` supplies goals, obligations, edit/evolution requests
and explicit private-fixture consent, not completed declarations or a golden
partition. Source fixtures are ignored by project declaration discovery but their
bytes are covered by `agent::harness`'s fingerprint.

| Case | Observable boundary |
| --- | --- |
| `library-consumer` | Pricing library, real receipt consumer, independent label |
| `docs-assets` | Content/theme, independently reviewed SVG file and licensing input |
| `monorepo` | UI/API contract, existing person-owned integration gate, preview session |

## Deterministic checks

Requires Python 3.11+ on Linux; only the standard library is used. The driver
observes a process's descendants through `/proc`, so it refuses other systems,
where it could not own them once the process itself exits:

```sh
python3 -I harness/hey-arti/driver.py check
python3 -I harness/hey-arti/driver.py check --artifactize <absolute-candidate-binary>
```

The first checks source runtime commands, private external copies and scripted
edits, denied traversal and symlinked paths, the skill's TOML examples and the
live-run lifecycle. It also checks the monorepo preview session against actual
UI/API bytes, including an API edit. It starts no coding host/provider or Human
review.

The second additionally uses a deterministic fake builder and the **explicit
candidate** for `config check`, graph/status JSON and one private session-bound
`tools check --execute`. Cargo's `hey_arti` integration test supplies its candidate,
so these checks run in ordinary CI. Fake declarations stay in private copies and
never substitute for actual builder/reuse/Human-boundary evidence.

The runtime eval runs the first form. Temporary outputs live in a named private
subdirectory of the external TMPDIR, never loose files in its root. Observation
deadlines bound failures, not ordering.

## Manual host registration smoke

The two B2 seams need installed coding hosts and are ignored in ordinary cargo/CI:
`b2_claude_code_registration_lists_tools` (also requires normal inference auth) and
`b2_codex_registration_lists_tools`. The equivalent standalone driver is:

```sh
python3 -I harness/hey-arti/host_smoke.py --host claude \
  --artifactize <absolute-candidate-binary> --output <fresh-external-directory>
python3 -I harness/hey-arti/host_smoke.py --host codex \
  --artifactize <absolute-candidate-binary> --output <another-fresh-directory>
```

It follows the docs' `mcp add`/`mcp get` registration, launching in a private
project with temporary `CLAUDE_CONFIG_DIR`/`CODEX_HOME` and a child PATH containing
only the selected candidate ahead of the normal PATH. Claude Code's actual startup
inventory and Codex's actual app-server `mcpServerStatus/list` must contain exactly
20 tools. Registration/get output, host transcripts and result JSON remain
external. No owner's configuration or credentials are copied or changed.

The inventory includes `config_graph`, `request_logs` and `verify_watch`.
The Rust MCP seams exercise change-driven notification delivery, final-outcome
separation, bounded long-log tail/range/search and missing/pruned evidence.
These deterministic checks are not evidence that a real host injects notifications
into an agent conversation. The live builder prompt asks for graph discovery and
impact, log-specific diagnosis where needed, and watch where the host supports it;
bounded wait remains a valid fallback. Keep actual host transcripts as evidence,
never replace them with a claimed stream delivery.

## Manual live builders

One actual Claude Code builder runs in each private fixture, using the canonical
skill, case consent and the registered candidate's MCP tools:

```sh
python3 -I harness/hey-arti/live.py \
  --artifactize <absolute-candidate-binary> \
  --output <fresh-external-builder-directory> \
  --evidence-root <fresh-external-evidence-directory>
```

`--model` selects the installed host's normally authenticated model; the default
names the project's routed Sol model. For a configured loopback Anthropic-compatible
local gateway only, the driver uses a **nonsecret placeholder** API key. It never
reads, copies or persists real credentials. Other providers use the host's normal
authentication. The driver does not change permission mode or use bypass flags;
it grants ordinary file-editing and this MCP server's tools through the host's
normal per-invocation allowlist. Use a builder directory outside sensitive host
configuration trees such as `.claude`.

The host gets real goals, consent and staged reuse/edit/judgment/evolution requests,
not generated declarations or verdicts. It runs runtime checks and real reviews
through artifactize, leaves read-only-token CI and records its evidence. The
monorepo builder must yield at the already-agreed Human sign-off, never claim,
submit or release it. The host is told to inspect tool-session ownership/cleanup.
All application programs run only through runtime evals or owned review sessions.

After the host finishes, **the driver**, not the builder, explicitly cancels any
remaining private fixture Runs and waits for settlement before removing fixture
working directories. Human authority is never exercised. The archive retains
repository snapshots, candidate SHA-256/host/model identity, actual transcripts,
state and post-host cancellation audit. A successful host exit alone is not a
quality verdict; missing work and permission failures remain actual evidence.

The archive's `index.json` names these external records. Output repositories,
state and transcripts never become reviewed source-fixture/fingerprint inputs.

No eval reads the live archive: the skill's quality is judged by the skill's own
evals against written guidance, and live builder runs remain a manual aid for a
person to read.
