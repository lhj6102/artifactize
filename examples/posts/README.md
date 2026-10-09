# Posts as file Artifacts

Three blog posts share one style checker. Each post has its own TOML sidecar,
Artifact name, eval and reusable result. No folder declaration is needed at the
example root.

```text
posts/
├── house-style/            basis Artifact with the shared checker and banned words
│   ├── index.artf
│   ├── check.py
│   └── banned.txt
├── welcome.md
├── welcome.md.artf         file Artifact welcome, 120-word limit
├── release-notes.md
├── release-notes.md.artf   file Artifact release-notes, 120-word limit
├── tip.md
└── tip.md.artf             file Artifact tip, 40-word limit
```

What it demonstrates:

- **One file, one declaration.** `tip.md.artf` declares the neighboring `tip.md`.
  `name` is required, and `[evals.style]` becomes `tip/style`. The other posts
  have separate declarations; their word limits and titles are ordinary TOML,
  not template parameters. Projects that want templates generate `.artf` files
  themselves and commit them.
- **Default artifactsum.** Each declaration omits `fingerprint`, so a post hashes
  only its own file. A change to `tip.md` reviews only `tip/style`; sibling posts,
  the README and `.artf` declarations are not part of that file's artifactsum.
  Changing an eval's instruction or runtime argv still changes its definition
  hash. `fingerprint = false` would disable reuse.
- **A shared mount.** Every post mounts the `house-style` basis Artifact as
  `style`. It has no evals and uses default folder artifactsum. Each post's reuse
  key therefore covers both its file and the checker and banned words. Changing
  `house-style/check.py` or `house-style/banned.txt` reviews all three posts.
- **File references and cwd.** Runtime argv resolves `{tip}` to the absolute
  file path and `{style}/check.py` to the mounted checker. File references never
  take a `/path` suffix. The process runs in `examples/posts`, the file's
  containing folder, with only PATH and LANG inherited. The checker reads its
  arguments without writing into the workspace.
- **Tags.** `type:post` and `length:brief` appear in status and graph output.
  They do not change fingerprints, relations or reuse keys.

For example, `tip.md.artf` contains:

```toml
name = "tip"
tags = ["type:post", "length:brief"]
mounts = { style = "house-style" }

[evals.style]
title = "The brief post fits in 40 words and follows the house style"
profile = { kind = "runtime", command = "python3", args = ["{style}/check.py", "{tip}", "{style}/banned.txt", "40"], timeout_ms = 10000 }
payload.instruction = "Check that {tip} starts with a title, stays within 40 words, and avoids every word banned in {style}."
```

## Run it

You need artifactize 0.9 or later on your PATH
([Install](https://artifactize.dev/docs/getting-started/install.html)), plus
`python3`. No model or API key is needed. From the repository root:

```sh
export ARTIFACTIZE_STATE_HOME=$(mktemp -d)   # outside this checkout
cd examples/posts
artifactize config check   # four Artifacts, three evals; runs no owner code
artifactize config graph   # each post is [file], with a mount to house-style
artifactize status         # exit 1: all three evals would execute
artifactize verify --all   # three GREEN results, exit 0
artifactize verify --all   # nothing executes; all three results are reused
artifactize status         # exit 0: every post shows PASS and reuse
artifactize run show RUN_ID
```

## Select posts

Select declared Artifact names, not filenames or a template name:

```sh
artifactize verify tip                            # just tip/style, reused
artifactize verify tip/style                      # the qualified eval, reused
artifactize verify --artifacts welcome,tip         # two posts
artifactize verify --evals welcome/style,tip/style # two qualified evals
artifactize status tip
artifactize config graph tip --json
```

The same names work in `--artifacts-file` and `--evals-file` (a JSON array or one
ID per line). `verify --json` saves each resolved `argv`, cwd and fingerprint.
`tip/style` ends its argv with `40`; the others use `120`. JSON graph and saved
Artifact definitions show `kind: "file"` and a target path such as `tip.md`.

## Make one post RED

Edit a copy so the repository stays clean. Keep the same state directory to reuse
the first Run's results. From the repository root:

```sh
cp -r examples/posts /tmp/artifactize-posts && cd /tmp/artifactize-posts
printf 'It helps you leverage every run.\n' >> tip.md
artifactize status         # tip/style: changed: tip.md; the other two reuse
artifactize verify --all   # exit 1: tip/style is RED (banned word: leverage)
```

Only `tip/style` executes; `welcome/style` and `release-notes/style` reuse their
GREEN results. Remove the added line and it reuses the original GREEN again.

The checker also rejects a missing `# ` title and posts over their declared word
limit. Appending a comment to `house-style/check.py` changes the shared basis
fingerprint, so the next `verify --all` executes all three checks again. Adding a
word to `house-style/banned.txt` does the same. No special template or shared-file
reuse rule is needed: those files belong to the mounted Artifact.
