# Posts as file Artifacts

Three blog posts share one style checker. One macro makes each post an Artifact
with a file target, an eval and a reusable result. No Artifact needs to target the
example root.

```text
posts/
├── .artifactize/
│   ├── house-style.artf    basis: path = "house-style/"
│   └── posts.artf          the post! macro; welcome and release-notes (120 words)
│                           and tip (40 words), each made by it
├── house-style/
│   ├── check.py
│   └── banned.txt
├── welcome.md
├── release-notes.md
└── tip.md
```

What it demonstrates:

- **One macro, three posts.** `posts.artf` declares `artf! post(file, words = "120", …)`
  once, and `artf tip = post!(file = "tip.md", words = "40", …)` binds `tip`, whose
  `style` eval is `tip/style`. The other posts call the macro with its defaults.
  `artifactize config expand tip` prints the declaration the call makes, every
  parameter filled in.
- **Default artifactsum.** Each declaration omits `fingerprint`, so a post hashes
  only its own file. A change to `tip.md` reviews only `tip/style`; sibling posts,
  the README and the definitions are not part of that file's artifactsum.
  Changing an eval's instruction or runtime argv still changes its definition
  hash. `fingerprint = false` would disable reuse.
- **A shared basis.** Every post's eval names the `house-style` basis Artifact in its
  arguments, `@{path house-style/check.py}` and `@{path house-style/banned.txt}`,
  because it runs the checker with the banned words. That makes `house-style` its
  dependency. The basis has no evals and uses default folder
  artifactsum. Each post's reuse key therefore covers both its file and the checker
  and banned words, including each covered Artifact's kind (`file` or `folder`), never
  its physical path. Changing `house-style/check.py` or `house-style/banned.txt`
  reviews all three posts.
- **File references and cwd.** Runtime argv resolves `@{path self}` to the absolute
  path of the post's file and `@{path house-style/check.py}` to the shared checker. A
  File Artifact's slot never takes a `/path` suffix. The process runs in `examples/posts`, the file's
  containing folder, with the author's environment. The checker reads its
  arguments without writing into the workspace.
- **Tags.** `type:post` and `length:brief` appear in status and graph output.
  They do not change fingerprints, relations or reuse keys.

`.artifactize/posts.artf` holds the macro and the three posts:

```rust
// .artifactize/posts.artf
artf! post(
  file,
  words = "120",
  tags = ["type:post"],
  title = "The post fits in 120 words and follows the house style",
) = {
  path = "@{param file}"
  tags = tags

  eval style : runtime(
    command = "python3",
    args = ["@{path house-style/check.py}", "@{path self}", "@{path house-style/banned.txt}", "@{param words}"],
    timeout_ms = 10000,
  ) = {
    title = title
    instruction = "Check that the post starts with a title, stays within @{param words} words, and avoids every word the house style bans."
  }
}

artf welcome = post!(file = "welcome.md")
artf release-notes = post!(file = "release-notes.md")
artf tip = post!(
  file = "tip.md",
  words = "40",
  tags = ["type:post", "length:brief"],
  title = "The brief post fits in 40 words and follows the house style",
)
```

## Run it

You need an artifactize build that supports `.artifactize/` definitions on your PATH
([Install](https://artifactize.dev/docs/getting-started/install.html)), plus
`python3`. No model or API key is needed. From the repository root:

```sh
export ARTIFACTIZE_STATE_HOME=$(mktemp -d)   # outside this checkout
cd examples/posts
artifactize config check   # four Artifacts, three evals; runs no owner code
artifactize config graph   # each post is [file], referencing house-style
artifactize config expand tip   # the declaration post! makes for tip
artifactize status         # exit 1: all three evals would execute
artifactize verify --all   # three GREEN results, exit 0
artifactize verify --all   # nothing executes; all three results are reused
artifactize status         # exit 0: every post shows PASS and reuse
artifactize run show RUN_ID
```

## Select posts

Select declared Artifact names, not filenames or the macro's name:

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
reuse rule is needed: those files belong to the referenced Artifact.
