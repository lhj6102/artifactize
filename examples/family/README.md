# An Artifact family

Three blog posts share one declaration, one check script and one fingerprint
script. Each post is still its own Artifact, with its own eval, result and
place in the graph.

```
family/
├── house-style/          basis Artifact: banned.txt lists words posts must avoid
└── posts/                the family "posts"; mounts house-style as "style"
    ├── artifactize.json  one declaration for every instance
    ├── instances.json    the instance list: welcome, release-notes, tip
    ├── check.py          shared material: the check every instance runs
    ├── fingerprint.py    shared material: the fingerprint every instance computes
    ├── welcome.md        material of the welcome instance
    ├── release-notes.md  material of the release-notes instance
    └── tip.md            material of the tip instance
```

What it demonstrates:

- **One declaration, many Artifacts.** `posts/artifactize.json` declares
  `"family": {"instances": "instances.json", ...}`. The family name `posts` is
  reserved and is not an Artifact. Each listed instance becomes an ordinary
  Artifact (`welcome`, `release-notes`, `tip`) with its own eval
  (`welcome/style`, ...). artifactize reads `instances.json` as static data and
  never runs code to produce it.
- **Parameters and variants.** `{"$param": "/pointer"}` values in `evals` and
  `views` are replaced with JSON from the instance's parameters. Parameters
  merge shallowly in this order: `family.params` (defaults: 120 words and a
  title), then the named variant (`tip` selects `brief`: 40 words and another
  title), then the instance's own `params` (`file`, a reference to its post).
  Nothing is interpolated inside strings.
- **Shared and per-instance material.** Each instance lists its own post as
  `material`. The material must exist and is passed to the fingerprint script.
  Every instance runs the shared `check.py` from the `posts` folder.
  `fingerprint.py` hashes the shared files, the banned-word list, the instance's
  own entry in `instances.json` and its own material. Editing `tip.md`
  therefore re-runs only `tip/style`, while editing `check.py` or
  `house-style/banned.txt` re-runs every instance.
- **The fingerprint script form.** This example keeps an owner-written
  `"fingerprint": {"script": {...}}` to show the script protocol: the stdin
  context with the instance's family material, and an argv reference
  (`{style}/banned.txt`). The built-in content form, `"fingerprint": {}`, gives
  the same per-instance reuse without a script: it hashes the shared folder minus
  every instance's material, adds the instance's own material, and covers the
  mounted `house-style` as a direct dependency. The runtime-relations example
  uses it.
- **A mount in a family.** Every instance mounts `house-style` as `style`,
  depends on it, and passes `{style}/banned.txt` to the check.

## Run it

You need `artifactize` on your `PATH` ([Install](https://artifactize.dev/docs/getting-started/install.html):
`cargo install --path crates/artifactize --locked`), plus `python3`.

```sh
cd examples/family
artifactize config check   # validates the expanded instances; runs no owner code
artifactize config graph   # "Family posts: release-notes, tip, welcome" and each instance's relations
artifactize config graph posts --json
artifactize status         # exit 1: all three evals would execute
artifactize verify --all   # three GREEN results, exit 0
artifactize verify --all   # nothing executes: each instance reuses its result
artifactize status posts   # exit 0: every instance shows "PASS — reuse"
artifactize run show RUN_ID
```

## Family selectors

A family name selects every instance, wherever an Artifact name is accepted:

```sh
artifactize verify posts                           # release-notes/style, tip/style, welcome/style
artifactize verify tip                             # one instance
artifactize verify --artifacts tip,posts          # names mixed; tip is selected once
artifactize verify --evals welcome/style,tip/style # qualified instance evals
artifactize status posts
artifactize config graph posts
```

The same names work in `--artifacts-file` and `--evals-file` (a JSON array or
one ID per line). `verify --json` shows each request's expanded `title` and resolved
`argv`: `tip/style` ends with `40`, and the other instances end with `120`.

## Make one instance RED

Edit a copy so the repository stays clean. From the repository root:

```sh
cp -r examples/family /tmp/family && cd /tmp/family
printf 'It helps you leverage every run.\n' >> posts/tip.md
artifactize verify posts   # exit 1: tip/style is RED ("banned word: leverage")
```

Only `tip/style` runs. `welcome/style` and `release-notes/style` reuse their
earlier results, because their fingerprints are unchanged.
