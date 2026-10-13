# Hey-arti harness

The harness keeps **source fixtures** separate from builder outputs and live
quality evidence. `cases.toml` supplies goals, obligations, edit/evolution requests
and explicit private-fixture consent, not completed declarations or a golden
partition. The fixtures are ignored by project declaration discovery but their
source bytes are covered by `arti-harness`'s fingerprint.

| Case | Observable boundary |
| --- | --- |
| `library-consumer` | Pricing library, real receipt consumer, independent label |
| `docs-assets` | Content/theme, independently reviewed SVG file and licensing input |
| `monorepo` | UI/API contract, existing person-owned integration gate, preview session |

## Deterministic checks

Requires Python 3.11+; only the standard library is used. Run from any directory:

```sh
python3 -I harness/hey-arti/driver.py check
python3 -I harness/hey-arti/driver.py check --artifactize <absolute-candidate-binary>
```

The first form checks all source fixture runtime commands, isolated external
copies and scripted edits, session readiness/stop, JSON tool envelopes, bounded
UTF-8 reads, rejected host/traversal/hidden paths, and that fake evidence cannot
be presented as live. It also checks the monorepo preview against actual UI/API
bytes, including an API edit. It starts no coding host/provider or Human review.

The second additionally gives a deterministic fake builder each fixture and
runs the **explicit candidate** through `config check`, graph/status JSON, and
session-bound `tools check --execute` (one private preview). It never resolves an
installed `artifactize` from PATH. Cargo's `hey_arti` integration test supplies its
candidate executable, so these checks run in ordinary CI. The fixture subprocesses
use literal argv and Python isolated mode, with the review store off. Fake builder
declarations are generated in private copies, never committed into source
fixtures. They do not substitute for actual builder/reuse/Human-boundary evidence.

The runtime eval runs the first form. Its test runner creates its own named
private directory under artifactize's external TMPDIR, never loose fixture files
in the temporary-directory root. All test processes have observation deadlines;
readiness is a newline barrier, not a sleep-length assertion.

## Review tool session

`index.artf` declares `cases` start/stop plus session-bound `case` and `source`
command tools. The driver accepts the existing version-1 JSON envelope on stdin.
Only `case`/`read` accept reviewer arguments:

- `case`: `{"case":"monorepo"}` returns agreed goals, source files and live
  evidence availability. There is no builder command/instance/lifetime argument.
- `read`: `{"case":"monorepo","path":"ui/view.py","offset":0,"limit":16384}`
  returns bounded source text and `next_offset` (UTF-8 byte offset).
- `start`/`stop`: internal commands with empty args, called only by artifactize.

Start creates fresh owner-private case repositories, state/evidence areas and a
loopback stop endpoint under `ARTIFACTIZE_SESSION_DIR` **before** its readiness
newline, then remains alive. Stop asks it to finish; start removes its allocated
area. Artifactize owns stop invocation, process joining/crash cleanup and final
session-directory removal. The driver starts no child builders in this stage.
Read is confined to a selected private case repository, rejects symlinks and
hidden files, and never reads arbitrary host paths. These commands are trusted
local programs, not an OS sandbox.

## Live evidence gate

The live builder pass is intentionally wired later, after the Run service and
MCP server are available. `case` currently reports live evidence unavailable;
`builder-quality` must be RED if it is missing. Neither `driver check`, fake
builder declarations nor a cached deterministic success can make that eval GREEN.
The downstream `arti-onramp/ready` requires both the skill and harness judgments,
without a discretionary Human gate or plugin-distribution dependency.

The later live pass must use the canonical skill, actual MCP registration, an
isolated child PATH pointing at the candidate, and ordinary host permissions and
authentication (no bypass flags). For each of the three fixtures in one real host,
record candidate/host versions, actual declarations, settled Run results,
unchanged and edited reuse, judgment/evolution checks, tool cleanup, read-only CI
and the Human handoff. Only an explicitly simulated **person** in a private
fixture may submit its Human evidence; the builder must yield. Outputs, state and
transcripts stay external and are not fingerprint inputs. The Agent eval judges
actual behavior against the cases, not one preferred partition.
