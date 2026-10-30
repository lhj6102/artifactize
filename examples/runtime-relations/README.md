# Runtime evals with parts and references

This example is a two-page guide checked by runtime evals. It shows how
Artifacts relate to each other and how a content fingerprint lets a second
`verify` reuse earlier results and re-review only what a change touched.

```
runtime-relations/
├── glossary/            basis Artifact: terms.txt is accepted as is
└── guide/               parent Artifact: eval guide/terms, references glossary
    ├── check_terms.py   runtime check used by guide/terms
    ├── intro/           part Artifact: eval intro/heading
    └── usage/           part Artifact: eval usage/heading
```

What it demonstrates:

- **Parts.** `guide/intro` and `guide/usage` have their own `index.artf`,
  so they are separate Artifacts, and because `guide`'s folder holds them they are
  parts of `guide`: `guide` passes only when they pass too, and
  `artifactize verify guide` runs or reuses all three evals. Being part of `guide`
  makes them no input of its evals, and their folders' files are not `guide`'s.
  `guide/terms` reads both pages, so it names them, and that is why it waits for
  `intro/heading` and `usage/heading` to be GREEN.
- **A basis Artifact.** `glossary` declares `basis = true`: it has no evals
  and counts as accepted input. A basis never waives its own dependencies.
- **`{artifact}` references.** The instruction of `guide/terms` names
  `{intro}`, `{usage}` and `{glossary}`. Each one is a graph relation,
  and the text is never expanded. A reference is what makes an Artifact an eval's
  dependency. Its argv operands `{glossary}/terms.txt`, `{intro}/page.md` and
  `{usage}/page.md` are resolved to absolute paths before the command runs.
- **Runtime evals.** The exit code is the verdict: 0 is GREEN, anything else is RED.
  `intro/heading` and `usage/heading` run `grep`, and `guide/terms` runs
  `python3 check_terms.py`. Each command runs from its owner's folder with only
  `PATH` and `LANG` inherited.
- **Content fingerprint and reuse.** Each Artifact omits `fingerprint`, so it uses the
  built-in content fingerprint, artifactsum, which hashes the Artifact's own files (nested
  Artifact folders, every `*.artf` file, `__pycache__` and `.gitignore`d files excluded). An eval's reuse key adds the
  fingerprints of the Artifacts it names: `guide/terms` depends on `intro`,
  `usage` and `glossary`. That is
  why the `glossary` basis participates in reuse too. The default fingerprint is
  artifactsum; setting `fingerprint = false` on a dependency would leave `guide/terms`
  without a reuse key, so it would be reviewed on every `verify`. While the eval
  and those fingerprints are unchanged since a saved GREEN or RED result, `verify`
  reuses that result without running the eval, and `status` lists the files and
  dependencies that changed since the last cached result.

## Run it

You need `artifactize` on your `PATH` ([Install](https://artifactize.dev/docs/getting-started/install.html)), plus `python3` and `grep`.

```sh
cd examples/runtime-relations
artifactize config check    # static validation; runs no owner code
artifactize config graph    # Artifacts, evals, components and part/instruction/argv relations
artifactize status          # exit 1: two evals would execute, guide/terms waits for them
artifactize verify --all    # three GREEN results, exit 0
artifactize verify --all    # nothing executes: each line says "(reused from RUN_ID)"
artifactize status          # exit 0: every eval shows "PASS — reuse"
artifactize run list
artifactize run show RUN_ID # full saved JSON: argv, stdout, fingerprint, provenance
```

`config graph`, `status` and `verify` also accept `--json`. In the second Run,
`verify --all --json` reports `"executionsStarted": 0`, and each request's
`executionId` points to the first Run's execution. State goes to
`~/.local/state/artifactize` by default. To keep this example's state apart,
set `ARTIFACTIZE_STATE_HOME`, for example `export ARTIFACTIZE_STATE_HOME=$(mktemp -d)`.

## Make it RED

Edit a copy so the repository stays clean. From the repository root:

```sh
cp -r examples/runtime-relations /tmp/runtime-relations && cd /tmp/runtime-relations
artifactize verify --all    # reuses the results from the original folder: keys name Artifacts and content, not paths
```

1. Use a term the glossary does not define:

   ```sh
   printf 'Results live in the **cache**.\n' >> guide/usage/page.md
   artifactize status        # usage/heading: "changed: page.md"; guide/terms: "dependency usage changed"
   artifactize verify --all  # exit 1
   ```

   `usage/heading` runs again because its folder changed, and stays GREEN.
   `guide/terms` runs again because the fingerprint of `usage`, which it names,
   changed, and is RED, with `cache: NOT DEFINED` in its stdout. `intro/heading`
   is reused.
   Adding `cache: ...` to `glossary/terms.txt` changes `glossary`'s fingerprint,
   which `guide/terms`'s key covers, so the next `verify` runs `guide/terms` again
   and it turns GREEN.

2. Remove the `# Usage` heading from `guide/usage/page.md`. `usage/heading`
   is RED, and `guide/terms` is BLOCKED: a RED dependency blocks the evals that name
   it, so they do not run. `guide` is RED as well, because a failing part
   fails the Artifact it is a part of.
