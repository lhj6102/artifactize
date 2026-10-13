# Hey-arti harness

The harness keeps **source fixtures** separate from builder outputs and live
quality evidence. `cases.toml` supplies goals, obligations, edit/evolution requests
and explicit private-fixture consent, not completed declarations or a golden
partition. Source fixtures are ignored by project declaration discovery but their
bytes are covered by `arti-harness`'s fingerprint.

| Case | Observable boundary |
| --- | --- |
| `library-consumer` | Pricing library, real receipt consumer, independent label |
| `docs-assets` | Content/theme, independently reviewed SVG file and licensing input |
| `monorepo` | UI/API contract, existing person-owned integration gate, preview session |

## Deterministic checks

Requires Python 3.11+; only the standard library is used:

```sh
python3 -I harness/hey-arti/driver.py check
python3 -I harness/hey-arti/driver.py check --artifactize <absolute-candidate-binary>
```

The first checks source runtime commands, private external copies and scripted
edits, session readiness/stop, JSON envelopes, bounded UTF-8 reads, denied
host/traversal/hidden paths and rejection of fake evidence as live. It also checks
the monorepo preview against actual UI/API bytes, including an API edit. It starts
no coding host/provider or Human review.

The second additionally uses a deterministic fake builder and the **explicit
candidate** for `config check`, graph/status JSON and one private session-bound
`tools check --execute`. Cargo's `hey_arti` integration test supplies its candidate,
so these checks run in ordinary CI. Fake declarations stay in private copies and
never substitute for actual builder/reuse/Human-boundary evidence. Deterministic
sessions use a deliberately absent evidence index; locally available live output
cannot turn a source-driver assertion into a live success.

The runtime eval runs the first form. Temporary outputs live in a named private
subdirectory of the external TMPDIR, never loose files in its root. Readiness is a
newline barrier and observation deadlines bound failures, not ordering.

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
17 tools. Registration/get output, host transcripts and result JSON remain
external. No owner's configuration or credentials are copied or changed.

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

The archive's `index.json` names these external records. A machine-local ignored
`live-evidence.json` points at the latest archive; it is not a declaration or
fingerprint input. For a different checkout, supply the archive index through
`HEY_ARTI_EVIDENCE_INDEX` in the author environment. Output repositories, state
and transcripts never become reviewed source-fixture/fingerprint inputs.

## Review tool session and live evidence gate

`index.artf` declares the `cases` start/stop session and bounded `case`/`source`
command tools. They use the existing version-1 JSON envelope. Reviewers cannot
choose host paths, launch builders or control session lifetime:

- `case`: `{"case":"monorepo"}` returns agreed goals, original source files,
  imported builder file names and actual live-evidence availability/identity.
- `read`: `{"case":"monorepo","path":"builder-evidence.json","offset":0,"limit":16384}`
  reads UTF-8 bytes with `next_offset`. Also inspect `evidence/transcript.jsonl`,
  `evidence/live.json` and `evidence/cleanup.json`; reads are bounded. When the
  record names a post-fix inspection follow-up, read its actual products under
  `evidence/inspection-recheck/` as well; missing fields in an earlier candidate
  are not promoted to success without that live follow-up.
- `start`/`stop`: internal empty-argument operations, called only by artifactize.

Before its readiness newline, start materializes private cases and imports the
selected archive's actual repository/evidence snapshots. It excludes host config,
credentials, symlinks and hidden inputs except CI workflow YAML, then remains
alive on its private stop endpoint. Artifactize owns stop, joining and final
session-directory cleanup. It starts no builders during an Agent review. Reads
stay confined to that case, never arbitrary host paths. These trusted commands
are not an OS sandbox.

`builder-quality` must be RED for missing/blocked evidence and judges actual
builder behavior in all three fixtures, not one preferred partition. Neither
deterministic checks nor a host exit-zero record establish skill quality. The
`arti-onramp/ready` dependency requires the skill and harness judgments, without
a discretionary Human gate or plugin-distribution dependency.
