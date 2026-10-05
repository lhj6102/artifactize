# Agent eval with built-in and declared tools

An LLM reviews a short specification against its requirements, using only the
tools that the specification declares. A Human eval asks a reviewer to sign off
on the open questions.

```
agent-tools/
├── requirements/        basis Artifact: requirements.md lists R1–R4
└── spec/                reviewed Artifact; mounts requirements as "reqs"
    ├── spec.md          the specification under review
    ├── diagram.png      checked with the built-in view_image tool
    ├── notes.md         review notes for the Human eval
    ├── section.sh       plain-protocol Agent tool
    └── coverage.py      json-protocol Agent tool
```

What it demonstrates:

- **An Agent eval with strict schemas.** `spec/review` returns exactly one JSON
  object. GREEN must carry `covered` (requirement IDs) and `diagramMatches: true`,
  with optional `notes`. RED must carry `findings`. Fields outside
  `passSchema`/`failSchema` are rejected, and an invalid answer gets one repair
  turn with tools disabled. The profile also sets budgets: `timeoutMs`,
  `maxToolCalls` and `maxTokens`.
- **Built-in tools.** `read`, `grep` and `view_image` are declared as
  `{"builtin": ...}`. They run inside artifactize, read only, and see `spec` plus
  its mount, so `reqs/requirements.md` can be read through the alias.
- **A declared `plain` tool.** `section` validates `{"title": ...}` against its
  `inputSchema` and substitutes the value for `{title}` as one argv element.
  Its stdout is the text result, and a nonzero exit makes it a tool error.
- **A declared `json` tool.** `coverage` reads `{version, context, args}` on
  stdin. Its argv gets `{reqs}/requirements.md` as an absolute path. It prints a
  structured observation: the spec lines that cite each requirement ID, and the
  IDs that nothing cites. An unknown `id` returns an authored error
  (`isError: true` with one text block).
- **Tool names.** The reviewer sees each tool as `<tool>_<artifact>`:
  `read_spec`, `grep_spec`, `view_image_spec`, `section_spec`, `coverage_spec`.
- **A basis and a dependency.** `requirements` is a basis Artifact. The mount
  makes `spec` depend on it, and the instruction names it as `{reqs}`.
- **A Human eval with Human tools.** `spec/signoff` declares two Human tools.
  `open` is a `launch` tool: it runs `xdg-open {artifactPath}/notes.md` and
  records only that the program started. `notes` is an `output` tool: it prints
  the notes in the terminal. Human tools take no arguments and run in your real
  environment.

`spec` declares no fingerprint, so every `verify` asks for a fresh review.

## Check the tools (no model needed)

You need `artifactize` on your `PATH` ([Install](https://artifactize.dev/docs/getting-started/install.html):
`cargo install artifactize --locked`), plus `python3`.

```sh
cd examples/agent-tools
artifactize config check
artifactize config graph
artifactize tools check      # static: schemas, executables and scoped paths; runs nothing
artifactize tools check --execute --artifact spec --audience agent --tool coverage --args '{}'
artifactize tools check --execute --artifact spec --audience agent --tool coverage --args '{"id":"R9"}'
artifactize tools check --execute --artifact spec --audience agent --tool section --args '{"title":"Dry run"}'
artifactize tools check --execute --artifact spec --audience agent --tool grep --args '{"pattern":"archive","path":"reqs"}'
artifactize tools check --execute --artifact spec --audience agent --tool view_image --args '{"path":"diagram.png"}'
artifactize tools check --execute --artifact spec --audience human --tool notes
```

An explicit check runs one tool and creates no Run, verdict or cache entry. It
exits 1 when the tool reports an error, as `coverage` does for `R9`. On a
machine without `xdg-open`, the static check reports `open_spec` as
unavailable. `notes_spec` works in any terminal.

## Choose a backend and model

`spec/review` names exactly one backend and one exact model. There is no
fallback or model catalog. The default profile uses `"backend": "openai"` with
the placeholder `YOUR_OPENAI_MODEL_ID`. The `anthropic` and `codex` entries in
`profileVariants` are complete alternative profiles. Replace the `model`
placeholder of the profile you use in `spec/artifactize.json`:

| Backend | Credentials | Models | Review command |
|---|---|---|---|
| `openai` (default) | `OPENAI_API_KEY` | `artifactize models openai` | `artifactize verify --eval spec/review` |
| `anthropic` | `ANTHROPIC_API_KEY` | `artifactize models anthropic` | `artifactize verify --eval spec/review --profile anthropic` |
| `codex` | `artifactize login codex` (a ChatGPT plan with Codex) | `artifactize models codex` | `artifactize verify --eval spec/review --profile codex` |

`artifactize doctor` reports which keys and sign-ins are present, without calling
any provider.

`reasoning` must be a value that the backend accepts (`openai` and `codex`:
`none` to `max`; `anthropic`: `low`, `medium`, `high`, `max`).
artifactize never remaps it. Use `--profile` together with
`--eval spec/review`, because a named profile must exist on every included
eval and `spec/signoff` is a Human eval. To change the default instead, edit
`backend` and `model` in `profile`. See [Agent reviews](https://artifactize.dev/docs/guides/agent-evals.html#agent-reviews)
for details on each backend.

```sh
artifactize verify --eval spec/review   # exit 0 GREEN, 1 RED, 2 ERROR (for example, a missing API key)
artifactize run show RUN_ID             # verdict, owner fields, every tool call, per-attempt usage
```

To see a RED review, copy this folder (for example,
`cp -r examples/agent-tools /tmp/agent-tools` from the repository root) and
delete the `R3:` paragraph from the copy's `spec/spec.md`. `coverage` then
lists `R3` as uncited.

## Try the Human sign-off

```sh
artifactize verify --eval spec/signoff  # prints "Run: RUN_ID", records a WAITING_HUMAN request, exits 4
artifactize request list --run RUN_ID   # shows the REQUEST_ID of spec/signoff
artifactize request claim REQUEST_ID
artifactize request tool REQUEST_ID notes_spec   # prints notes.md
artifactize request tool REQUEST_ID open_spec    # opens notes.md with xdg-open
artifactize request submit REQUEST_ID --verdict GREEN --fields '{"approved":true,"comment":"Both questions are answered."}'
artifactize run show RUN_ID
```

`claim`, `tool` and `submit` use `$USER` as the reviewer unless you pass
`--reviewer NAME`. Only the claimant can run tools or submit. Fields are checked
against the eval's schemas: `{"approved":false}` is rejected and the request
stays open. A RED sign-off looks like
`--verdict RED --fields '{"unresolved":["Which files count as old?"]}'`.
The Run stays INCOMPLETE while `spec/review` has no result, because an
Artifact is satisfied only when every one of its evals is GREEN. Without a
fingerprint, a submission settles only its own Run, and a later `verify` asks for
a new sign-off.
