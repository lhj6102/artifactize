# Agent eval with built-in and declared tools

An LLM reviews a short specification against its requirements, using only the
tools that the specification declares. A Human eval asks a reviewer to sign off
on the open questions.

```
agent-tools/
├── requirements/        basis Artifact: requirements.md lists R1–R4, with a read tool
└── spec/                reviewed Artifact; its review references requirements
    ├── spec.md          the specification under review
    ├── diagram.png      checked with the built-in view_image tool
    ├── notes.md         review notes for the Human eval
    └── coverage.py      json-protocol Agent tool
```

What it demonstrates:

- **An Agent eval with strict schemas.** `spec/review` returns exactly one JSON
  object. GREEN must carry `covered` (requirement IDs) and `diagramMatches: true`,
  with optional `notes`. RED must carry `findings`. Fields outside
  `pass_schema`/`fail_schema` are rejected, and an invalid answer gets one repair
  turn with tools disabled. The profile also sets `timeout_ms` and
  `max_tool_calls` budgets.
- **Built-in tools.** `read`, `grep` and `view_image` are declared as
  `{ builtin = "read" }`. They run inside artifactize, read only, and each sees
  only the Artifact that declares it. `requirements` declares its own `read`, so
  the reviewer reads `requirements.md` with `read_requirements`.
- **A fixed-file built-in tool.** `section` is declared as
  `{ builtin = "section", args = ["spec.md"] }`. The model passes only
  `{"heading": ...}`. It prints the matching Markdown heading and its contents,
  stopping before the next heading at the same or a higher level; no shell script
  or external program is needed.
- **A declared `json` tool.** `coverage` reads `{version, context, args}` on
  stdin. Its argv gets `{requirements}/requirements.md` as an absolute path. It prints a
  structured observation: the spec lines that cite each requirement ID, and the
  IDs that nothing cites. An unknown `id` returns an authored error
  (`isError: true` with one text block).
- **Tool names.** The reviewer sees each tool as `<tool>_<artifact>`:
  `read_spec`, `grep_spec`, `view_image_spec`, `section_spec`, `coverage_spec`
  and `read_requirements`.
- **A basis and a dependency.** `requirements` is a basis Artifact. The
  instruction of `spec/review` names it as `{requirements}`, which makes it that
  eval's dependency and admits it, with its tools, to the review.
- **A Human eval with Human tools.** `spec/signoff` declares two Human tools.
  `open` uses `{ builtin = "open", args = ["{artifactPath}/notes.md"] }` to
  launch the desktop's default application on Linux, macOS or Windows; it records
  only that the application launch was accepted. `notes` uses the `read` builtin
  with the same fixed path to print the notes in the terminal. These tools run
  inside artifactize, take no model arguments and need no `artifactize-tools`
  executable on `PATH`.

`spec` uses the default artifactsum fingerprint, so unchanged files reuse earlier reviews.

## Check the tools (no model needed)

You need `artifactize` on your `PATH` ([Install](https://artifactize.dev/docs/getting-started/install.html)), plus `python3`.

```sh
cd examples/agent-tools
artifactize config check
artifactize config graph
artifactize tools check      # static: schemas, executables and scoped paths; runs nothing
artifactize tools check --execute --eval spec/review --tool coverage_spec --args '{}'
artifactize tools check --execute --eval spec/review --tool coverage_spec --args '{"id":"R9"}'
artifactize tools check --execute --artifact spec --audience agent --tool section --args '{"heading":"Dry run"}'
artifactize tools check --execute --artifact requirements --audience agent --tool read --args '{"path":"requirements.md"}'
artifactize tools check --execute --artifact spec --audience agent --tool view_image --args '{"path":"diagram.png"}'
artifactize tools check --execute --artifact spec --audience human --tool notes
```

An explicit check runs one tool and creates no Run, verdict or cache entry. It
exits 1 when the tool reports an error, as `coverage` does for `R9`. `coverage`
reads `{requirements}`, so it runs in the scope of `spec/review`, whose instruction
names `requirements`; `--artifact spec` admits only `spec` and its children. Static
checks validate built-in tools without launching a desktop application. Actually using
`open_spec` requires a working desktop default-application mechanism;
`notes_spec` works in any terminal.

## Choose a backend and model

`spec/review` names exactly one backend and one exact model. There is no
fallback or model catalog. The default profile uses `backend = "openai"` with
the placeholder `YOUR_OPENAI_MODEL_ID`. The `anthropic` and `codex` entries in
`profile_variants` are complete alternative profiles. Replace the `model`
placeholder of the profile you use in `spec/index.artf`:

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
`backend` and `model` in `profile`. See [Agent reviews](https://artifactize.dev/docs/features/agent.html#agent-reviews)
for details on each backend.

```sh
artifactize verify --eval spec/review   # exit 0 GREEN, 1 RED, 2 ERROR (for example, a missing API key)
artifactize run show RUN_ID             # verdict, owner fields, per-attempt usage
artifactize session show REQUEST_ID     # the review's conversation, every tool call included
artifactize session show REQUEST_ID --summary   # turns, tokens, tool calls per tool
```

To see a RED review, copy this folder (for example,
`cp -r examples/agent-tools /tmp/agent-tools` from the repository root) and
delete the `R3:` paragraph from the copy's `spec/spec.md`. `coverage` then
lists `R3` as uncited.

## Try the Human sign-off

A Human eval starts only when every other non-Human eval of its Artifact is
GREEN: a person signs off only on what the machine evals already pass. Run
`spec/review` first, as above. Once its GREEN result is saved:

```sh
artifactize verify --eval spec/signoff  # reuses spec/review, records a sign-off request, and waits
```

If `spec/review` has no current GREEN result, the sign-off is not requested: it
waits, or is BLOCKED when the review is RED or ERROR.

While `verify` waits, in a second terminal (claim within 2 minutes, or the Run ends YELLOW
and the next `verify` reuses your later sign-off):

```sh
artifactize request list --run RUN_ID   # shows the REQUEST_ID of spec/signoff
artifactize request claim REQUEST_ID
artifactize request tool REQUEST_ID notes_spec   # prints notes.md
artifactize request tool REQUEST_ID open_spec    # opens notes.md in the desktop's default application
artifactize request submit REQUEST_ID --verdict GREEN --fields '{"approved":true,"comment":"Both questions are answered."}'
artifactize run show RUN_ID
```

`claim`, `tool` and `submit` use `$USER` as the reviewer unless you pass
`--reviewer NAME`. Only the claimant can run tools or submit. Fields are checked
against the eval's schemas: `{"approved":false}` is rejected and the request
stays open. A RED sign-off looks like
`--verdict RED --fields '{"unresolved":["Which files count as old?"]}'`.
Once you submit GREEN, `verify` records the sign-off and exits 0: with the
reused `spec/review` result, every eval of `spec` is GREEN. The default
artifactsum fingerprint lets a later `verify` reuse the sign-off while the files
and eval criteria remain unchanged. Set `fingerprint = false` to ask for a fresh sign-off every time.
