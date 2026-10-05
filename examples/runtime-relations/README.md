# Runtime evals with children, mounts and references

This example is a two-page guide checked by runtime evals. It shows how
Artifacts relate to each other and how a content fingerprint lets a second
`verify` reuse earlier results and re-review only what a change touched.

```
runtime-relations/
├── glossary/            basis Artifact: terms.txt is accepted as is
└── guide/               parent Artifact: eval guide/terms, mounts glossary as "terms"
    ├── check_terms.py   runtime check used by guide/terms
    ├── intro/           child Artifact: eval intro/heading
    └── usage/           child Artifact: eval usage/heading
```

What it demonstrates:

- **Children.** `guide/intro` and `guide/usage` have their own `artifactize.json`,
  so they are separate Artifacts. A parent depends on its nearest marked
  children, so `guide/terms` waits for `intro/heading` and `usage/heading` to be GREEN.
- **A basis Artifact.** `glossary` declares `"basis": true`: it has no evals
  and counts as accepted input. A basis never waives its own dependencies.
- **A mount.** `guide` mounts `glossary` under the alias `terms`. The mount
  is a logical name that adds a dependency. Nothing is copied or linked.
- **`{artifact}` references.** The instruction of `guide/terms` names
  `{intro}`, `{usage}` and `{terms}`. Each one is a graph relation, and the
  text is never expanded. Its argv shows three operand forms that
  artifactize resolves to absolute paths before the command runs:
  `{terms}/terms.txt` (a mount alias), `{guide}/intro/page.md` (a logical path
  from the owner through a child) and `{usage}/page.md` (an Artifact name).
- **Runtime evals.** The exit code is the verdict: 0 is GREEN, anything else is RED.
  `intro/heading` and `usage/heading` run `grep`, and `guide/terms` runs
  `python3 check_terms.py`. Each command runs from its owner's folder with only
  `PATH` and `LANG` inherited.
- **Content fingerprint and reuse.** Each evaluated Artifact declares the
  built-in content fingerprint, `"fingerprint": {}`, which hashes the Artifact's own files
  (child folders, `artifactize.json`, `__pycache__` and `.gitignore`d files
  excluded) plus one entry per direct dependency. `guide` spells out the
  defaults, `"files": ["."]` and `"dependencies": "direct"`, so its dependencies
  are `intro`, `usage` (children) and `glossary` (the mount). While the
  fingerprint is unchanged since a saved GREEN or RED result, `verify` reuses
  that result without running the eval, and `status` lists the files and dependencies that changed
  since the last cached result.

## Run it

You need `artifactize` on your `PATH` ([Install](https://artifactize.dev/docs/getting-started/install.html):
`cargo install --git https://github.com/lhj6102/artifactize --tag v0.4.0 --locked artifactize`), plus `python3` and `grep`.

```sh
cd examples/runtime-relations
artifactize config check    # static validation; runs no owner code
artifactize config graph    # Artifacts, evals, components and child/mount/instruction/argv relations
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
artifactize verify --all    # reuses the results from the original folder: fingerprints name content, not paths
```

1. Use a term the glossary does not define:

   ```sh
   printf 'Results live in the **cache**.\n' >> guide/usage/page.md
   artifactize status        # usage/heading: "changed: page.md"; guide/terms: "dependency usage changed"
   artifactize verify --all  # exit 1
   ```

   `usage/heading` runs again because its folder changed, and stays GREEN.
   `guide/terms` runs again because its direct dependency `usage` changed, and
   is RED, with `cache: NOT DEFINED` in its stdout. `intro/heading` is reused.
   Adding `cache: ...` to `glossary/terms.txt` changes `guide`'s fingerprint, so
   the next `verify` runs `guide/terms` again and it turns GREEN.

2. Remove the `# Usage` heading from `guide/usage/page.md`. `usage/heading` is
   RED, and `guide/terms` is BLOCKED: a RED dependency blocks its dependents,
   so they do not run.
