# Agent eval with built-in and declared tools

An LLM reviews a short specification against its requirements, using only the
tools that the specification composes. A Human eval asks a reviewer to sign off
on the open questions.

```
agent-tools/
├── .artifactize/
│   ├── profiles.artfprofiles  how the review runs, by provider
│   ├── requirements.artf
│   └── spec.artf        the review toolset, and spec's two evals
├── requirements/        basis Artifact: requirements.md lists R1–R4
└── spec/                reviewed Artifact; its review reads requirements
    ├── spec.md          the specification under review
    ├── diagram.png      checked with the built-in view_image tool
    ├── notes.md         review notes for the Human eval
    └── coverage.py      json-protocol Agent tool
```

What it demonstrates:

- **An Agent eval with prioritized issues.** `spec/review` returns a list of
  issues, each with a priority from P0 (the most important) to P3 and a reason.
  `red_at = "P1"` makes the eval RED when an issue is P1 or above; P2 and P3
  issues are reported but do not fail it. An invalid answer gets one repair
  turn with tools disabled. The profile also sets a `max_tool_calls` budget.
- **Toolsets.** The built-in `files` toolset gives `list`, `glob`, `grep` and
  `read`, and `tool review = { … }` declares the review's own tools.
  `artf spec : files & review` composes both; `artf requirements : files` composes
  only `files`. Built-in tools
  such as `builtin(op = "read")` run inside artifactize, read only, and each sees
  only the Artifact that composes it.
- **A fixed-file built-in tool.** `section` is `builtin(op = "section", args = ["spec.md"])`.
  The model passes only `{"heading": ...}`. It prints the matching Markdown heading
  and its contents, stopping before the next heading at the same or a higher
  level; no shell script or external program is needed.
- **A declared `json` tool.** `coverage` reads `{version, context, args}` on
  stdin. Its argv gets `@{path requirements/requirements.md}` as an absolute path.
  It prints a structured observation: the spec lines that cite each requirement
  ID, and the IDs that nothing cites. An unknown `id` returns an authored error
  (`isError: true` with one text block).
- **Tool names.** The reviewer sees each tool as `<toolset>_<tool>_<artifact>`:
  `files_read_spec`, `files_grep_spec`, `review_section_spec`,
  `review_view_image_spec`, `review_coverage_spec` and `files_read_requirements`.
  An instruction names the Artifacts whose tools the review uses, not the tools.
- **A basis and a dependency.** `requirements` is a basis Artifact. The
  instruction of `spec/review` names it as `@{artf requirements}`, which makes it
  that eval's dependency and admits it, with its tools, to the review.
- **A Human eval with Human tools.** `review` declares two Human tools. `open` is
  `builtin(op = "open", args = ["@{path self/notes.md}"])`: it launches the
  desktop's default application on Linux, macOS or Windows and records only that
  the launch was accepted. `notes` uses the `read` builtin with the same fixed path
  to print the notes in the terminal. These tools run inside artifactize, take no
  model arguments and need no `artifactize-tools` executable on `PATH`.

`spec` uses the default artifactsum fingerprint, so unchanged files reuse earlier reviews.

## Check the tools (no model needed)

You need `artifactize` on your `PATH` ([Install](https://artifactize.dev/docs/getting-started/install.html)), plus `python3`.

```sh
cd examples/agent-tools
artifactize config check
artifactize config graph
artifactize tools check      # static: schemas, executables and scoped paths; runs nothing
artifactize tools check --execute --eval spec/review --tool review_coverage_spec --args '{}'
artifactize tools check --execute --eval spec/review --tool review_coverage_spec --args '{"id":"R9"}'
artifactize tools check --execute --artifact spec --audience agent --tool review_section --args '{"heading":"Dry run"}'
artifactize tools check --execute --artifact requirements --audience agent --tool files_read --args '{"path":"requirements.md"}'
artifactize tools check --execute --artifact spec --audience agent --tool review_view_image --args '{"path":"diagram.png"}'
artifactize tools check --execute --artifact spec --audience human --tool review_notes
```

With `--artifact`, a tool is named without its Artifact (`review_section`); with
`--eval`, by the name the reviewer sees. An explicit check runs one tool and
creates no Run, verdict or cache entry. It exits 1 when the tool reports an
error, as `coverage` does for `R9`. `coverage` reads requirements, so it runs in
the scope of `spec/review`, whose instruction names `requirements`; `--artifact spec`
admits only `spec`. Static checks validate built-in tools without launching a
desktop application. Actually using `review_open_spec` requires a working desktop
default-application mechanism; `review_notes_spec` works in any terminal.

## Choose a backend and model

`spec/review` runs with the `reviewer` profile, whose setting names exactly one
backend and one exact model. There is no fallback or model catalog.
`profiles.artfprofiles` gives `reviewer` one setting per provider, chosen by the
`REVIEW_PROVIDER` environment variable, each with a model placeholder
(`YOUR_OPENAI_MODEL_ID` and so on). Replace the placeholder of the provider you
use:

| Backend | Credentials | Models | Review command |
|---|---|---|---|
| `openai` (default) | `OPENAI_API_KEY` | `artifactize models openai` | `artifactize verify --eval spec/review` |
| `anthropic` | `ANTHROPIC_API_KEY` | `artifactize models anthropic` | `REVIEW_PROVIDER=anthropic artifactize verify --eval spec/review` |
| `codex` | `artifactize login codex` (a ChatGPT plan with Codex) | `artifactize models codex` | `REVIEW_PROVIDER=codex artifactize verify --eval spec/review` |

`artifactize doctor` reports which keys and sign-ins are present, without calling
any provider.

`reasoning` must be a value that the backend accepts (`openai` and `codex`:
`none` to `max`; `anthropic`: `low`, `medium`, `high`, `max`).
artifactize never remaps it. A value of `REVIEW_PROVIDER` that names no setting
is an error rather than the default. A profile is how an eval runs, so a result
another setting produced is reused. Each setting is complete: change one without
changing the others.

```sh
artifactize verify --eval spec/review   # exit 0 GREEN, 1 RED, 2 ERROR (for example, a missing API key)
artifactize run show RUN_ID             # verdict, issues, per-attempt usage
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
artifactize request tool REQUEST_ID review_notes_spec   # prints notes.md
artifactize request tool REQUEST_ID review_open_spec    # opens notes.md in the desktop's default application
artifactize request submit REQUEST_ID --verdict GREEN
artifactize run show RUN_ID
```

`claim`, `tool` and `submit` use `$USER` as the reviewer unless you pass
`--reviewer NAME`. Only the claimant can run tools or submit. A person answers
GREEN with no issues, or RED with at least one: a RED sign-off looks like
`--verdict RED --issue "Which files count as old?"`, one `--issue` for each thing
to change. Once you submit GREEN, `verify` records the sign-off and exits 0: with
the reused `spec/review` result, every eval of `spec` is GREEN. The default
artifactsum fingerprint lets a later `verify` reuse the sign-off while the files
and eval criteria remain unchanged. Set `fingerprint = false` to ask for a fresh sign-off every time.
