# First Project in 5 Minutes

This tour walks through creating a minimal project, running `artifactize verify`, and seeing how Artifactize reuses results when only part of a project changes. No AI account or API key is required.

## 1. Create two Artifacts and a check

Create a temporary project folder with a `spec.md` file, a `guide.md` file, and `.artifactize/` definitions:

```sh
mkdir -p /tmp/my-first-artifactize/.artifactize
cd /tmp/my-first-artifactize

cat > spec.md <<'EOF'
# Greeting Spec
Every greeting guide must mention `hello`.
EOF

cat > guide.md <<'EOF'
# User Guide
Run `hello` to greet the team.
EOF
```

Now declare `spec` as an unreviewed input (`basis = true`) and `guide` with two runtime checks—one checking its own heading and one checking that it mentions `hello`, as `spec` requires, so `spec` is one of its `deps`:

```sh
cat > .artifactize/project.artf <<'EOF'
// The spec: an input accepted as it is.
artf spec = {
  path = "spec.md"
  basis = true
}

artf guide = {
  path = "guide.md"

  // Its verdict is grep's exit code: 0 when the guide has a top-level Markdown heading.
  eval heading : runtime(command = "grep", args = ["-q", "^# ", "@{path self}"]) = {
    title = "The guide includes a top-level heading"
  }

  // The greeting the spec requires; `deps` makes spec an input the command reads.
  eval mentions-hello : runtime(command = "grep", args = ["-q", "hello", "@{path self}"]) = {
    title = "The guide mentions the command required by the spec"
    deps = [spec]
  }
}
EOF
```

Validate the declarations and inspect the graph:

```sh
artifactize config check
artifactize config graph
artifactize status
```

`config check` confirms the definitions are valid without running any project commands. `status` predicts that both `guide/heading` and `guide/mentions-hello` will execute.

## 2. Verify and reuse

Run all checks and wait for completion:

```sh
artifactize verify --all --wait
```

Both checks pass with `GREEN` (exit code 0), and Artifactize prints the saved Run ID (such as `run-Abc123`). Now run `verify` a second time:

```sh
artifactize verify --all --wait
```

Nothing executes: both `guide/heading` and `guide/mentions-hello` reuse their `GREEN` results from the first Run because the file fingerprints (`artifactsum`) and the separate Eval definition hashes are unchanged, so their reuse keys still match.

## 3. Change one input and see what re-runs

Edit `spec.md` without touching `guide.md`:

```sh
printf '\nKeep greetings short.\n' >> spec.md
artifactize status
```

`status` shows that `guide/heading` will still **reuse** its earlier result (it only reads the guide), whereas `guide/mentions-hello` will **execute** again because `spec`, one of its inputs, has a changed fingerprint:

```sh
artifactize verify --all --wait
```

Only `guide/mentions-hello` runs; `guide/heading` is reused immediately.

## 4. Explore the repository examples

If you clone `https://github.com/lhj6102/artifactize`, the `examples/` directory contains ready-to-run projects demonstrating each capability:

| Example | What it shows |
|---|---|
| [`examples/runtime-relations`](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations) | Parent and `parts` composition (`guide` made of `intro` and `usage`), `@{path …}` references in argv, `basis = true`, and how a `RED` dependency blocks downstream checks. |
| [`examples/posts`](https://github.com/lhj6102/artifactize/tree/main/examples/posts) | A `post!` macro making three file Artifacts that share one style-checker Artifact (`house-style`), independent per-file reuse, and `tags`. |
| [`examples/agent-tools`](https://github.com/lhj6102/artifactize/tree/main/examples/agent-tools) | An Agent eval with composed toolsets of built-in (`read`, `grep`, `view_image`, `section`) and custom `json` tools returning prioritized issues, and a Human sign-off with `open` and `read` tools. |
| [`examples/live`](https://github.com/lhj6102/artifactize/tree/main/examples/live) | A Live URI target (`path = "https://..."`) observed by a read-only HTTP runtime check on every `verify`. |

## Next steps

- **[Working with Coding Agents](../agent-plugin/plugin.md)**: let **Arti** shape `.artifactize/` declarations for your own repository and iterate on failures with **focus-then-sweep**.
- **[Desktop App](../guides/interfaces.md)**: work with the built-in agent in Chat, watch Runs in Verify, and complete sign-offs in Review.
