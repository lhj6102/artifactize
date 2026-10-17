# Guide (no model needed)

The tour uses the example projects in the repository. Unless you built from a
checkout, clone it first:

```sh
git clone https://github.com/lhj6102/artifactize
cd artifactize
```

Each reviewed piece is an **Artifact**. Its `index.artf` is a text configuration
file listing its checks, called **evals**. It uses **TOML**, a readable `key = value`
format. A **fingerprint** identifies the reviewed inputs; the built-in
**artifactsum** makes that value from files so artifactize can detect changes.

Artifactize's **state** is where it stores Runs and results.
Keep this tour's reviews apart from your normal state. In a POSIX shell (for
example Bash or zsh), the first line below creates a fresh temporary folder with
`mktemp -d` and tells artifactize to keep its state there. `export` also passes that
setting to commands launched from this terminal. Then enter the example and check
its declarations:

```sh
export ARTIFACTIZE_STATE_HOME=$(mktemp -d)
cd examples/runtime-relations
artifactize config check
artifactize status
artifactize config graph
```

In PowerShell, replace the first line with
`$env:ARTIFACTIZE_STATE_HOME = (New-Item -ItemType Directory (Join-Path $env:TEMP (New-Guid))).FullName`.
This creates a uniquely named folder under Windows' temporary directory and saves
its path in the same setting. The `artifactize` commands are unchanged.

`config check` validates the declarations without running project programs.
`status` predicts two checks would execute and a third waits for their evidence;
its exit code is 1 because the project is not yet satisfied. `config graph` shows
the pieces and their input relationships. Now run all three checks and wait for
the results:

```sh
artifactize verify --all --wait
```

All three results should be GREEN, with exit code 0 (success). The output names
the saved Run, with an identifier such as `run-Abc123`. Replace `RUN_ID` in the
next command with **your printed identifier**, not those literal words:

```sh
artifactize run show RUN_ID
```

This prints the saved Run as JSON, a structured text record. `argv` is the list
of arguments passed to a program; `stdout` is the text that program printed.
The record also includes fingerprints identifying the reviewed inputs.

Run the same checks again:

```sh
artifactize verify --all --wait
artifactize status
```

Nothing executes this time: all three results are reused. Both commands exit 0,
and status identifies the reusable results. Keep this terminal open so later
steps use the same temporary state directory.

The [runtime-relations README](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations) also shows how to make a check RED. Each folder declares itself in
its `index.artf` file. Leaving out `fingerprint` uses the built-in file-change
check, artifactsum. To disable reuse, set `fingerprint = false`. See [Declarations (.artf)](../features/declarations.md) for snake_case keys and the JSON/output naming
distinction.

## Files and Eval dependencies

A sidecar such as `hero.png.artf` declares the neighboring file as an independent
Artifact. It requires a `name`, uses the same TOML schema as a folder and hashes
only that file by default. Built-in tools see the file and admitted mounts and
references, not siblings. The [posts example](https://github.com/lhj6102/artifactize/tree/main/examples/posts)
uses three Markdown sidecars and one shared style checker. From the previous
example's directory:

```sh
cd ../posts
artifactize config check   # four Artifacts, three evals
artifactize verify --all --wait   # three GREEN results
artifactize verify tip/style --wait # qualified file eval; reused
artifactize status         # all three post reviews are reused
cd ../runtime-relations    # continue the tour below
```

Each file hashes only itself. The mounted `house-style` Artifact covers the
checker and banned words, so a shared change invalidates all three post keys.
Its README shows how to make one post RED while the other two reuse.

A runtime Eval references its inputs directly in its instruction or arguments. For
example, the application can run its E2E suite using `{e2e-tests}` while the test-code
Artifact reviews its own style and feature coverage. Only that application Eval
waits for the test-code reviews; unrelated application Evals remain runnable.
See [Eval dependencies](../features/artifact.md#eval-dependencies).

## Agent tools and the Human flow

Both terminals must use the same temporary state directory. Before starting the
waiting command below, print the first terminal's value with
`echo "$ARTIFACTIZE_STATE_HOME"` (PowerShell: `$env:ARTIFACTIZE_STATE_HOME`).
In the second terminal, replace `STATE_DIRECTORY` below with that printed path.
Use the new Run ID from the waiting command for `RUN_ID`; replace `REQUEST_ID`
with the waiting request identifier printed by `request list`.
For PowerShell, use `$env:ARTIFACTIZE_STATE_HOME = "STATE_DIRECTORY"` instead of
`export`. Then open the same example directory.

```sh
cd ../agent-tools
artifactize tools check     # static: schemas, executables and scoped paths
artifactize tools check --execute --artifact spec --audience agent --tool coverage --args '{}'
artifactize verify --eval spec/signoff --wait   # records a WAITING_HUMAN request and waits
# In a second terminal, in the same directory:
export ARTIFACTIZE_STATE_HOME="STATE_DIRECTORY"  # copy the first terminal's path
artifactize request list --run RUN_ID    # shows REQUEST_ID
artifactize request claim REQUEST_ID
artifactize request tool REQUEST_ID notes_spec
artifactize request submit REQUEST_ID --verdict GREEN --fields '{"approved":true,"comment":"Both questions are answered."}'
artifactize run show RUN_ID
```

`claim`, `tool` and `submit` use `$USER` as the reviewer unless you pass
`--reviewer NAME`; `request unclaim REQUEST_ID` gives a claim back. `artifactize
review` does the same in a terminal UI: it lists waiting requests and previews the
instruction. Enter opens Detail, `c` explicitly claims, Tab focuses tools and
Enter (or a click) runs one at once. `g`/`r` opens the GREEN/RED fields; Ctrl-S
submits. Esc stops editing before returning to the list, keeping the draft.
The artifactize-owned Run waits for Human results like any other, so dependents
continue in the same Run. Plain `verify` submits and returns; `verify --wait`
follows the Run in the foreground and exits 4 here once the sign-off arrives,
because `spec/review` has no result yet.

For a real Agent review, put an exact model ID in the profile you use in `examples/agent-tools/spec/index.artf`,
then run `artifactize verify --eval spec/review --wait`, adding `--profile anthropic` or `--profile codex` for the other backends. Without
credentials the review is recorded as ERROR and the waiting command exits 2. See the [agent-tools README](https://github.com/lhj6102/artifactize/tree/main/examples/agent-tools).

## Monitor

```sh
artifactize monitor .      # Initially select this repository
artifactize monitor        # Initially select ALL repositories in this state
```

Start it in a second terminal while a Run executes. Use Enter or → to move from
Scope through Runs and the tree to an eval's Detail; Esc steps back. `j`/`k`
selects, `?` shows help, and `q` quits. Detail shows Agent conversations, Runtime
logs or the requirements an eval waits for. Browsing runs no owner commands.

For Human claims, tools and verdict forms, see
[Human reviews and monitor](../tui/index.md#monitor).

## Team review store (optional)

A team review store shares reusable verdicts across machines and CI. Each machine
signs in with a token, a secret that grants its allowed actions; CI should receive
read-only access so it cannot create an approval. The [team guide](../features/team/index.md)
and [walkthrough](../features/team/walkthrough.md) cover server setup, tokens and
trying the same review from a second machine.

## Next steps

The [examples](examples.md) show each feature in a small project, and
[Artifacts and evals](../features/artifact.md) explains the declaration
you just ran.
