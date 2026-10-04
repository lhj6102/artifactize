# Artifacts and evals

An Artifact is anything you review: code, docs, designs, images. You
declare it, and the evals that review it, in a static `artifactize.json` in its
folder. artifactize reads these files into one dependency graph.

## Folder configuration

A folder with an `artifactize.json` is an Artifact. It has a `name` and usually
`evals`; `mounts`, `basis`, `views`, `fingerprint` and, at the root, `reviewPolicy`
are optional. Each eval has an `id`, a `title`, a `profile` whose `kind` is
`runtime`, `agent` or `human`, and a `payload` with an `instruction`;
`passSchema`, `failSchema` and `profileVariants` are optional. A runtime eval's
exit code is its verdict (0 is GREEN); an Agent or a Human returns GREEN or RED
with owner fields. This is the `guide` Artifact of the
[runtime-relations example](https://github.com/lhj6102/artifactize/tree/main/examples/runtime-relations), whose README
explains children, mounts, `{artifact}` references and basis Artifacts:

```json
{
  "name": "guide",
  "mounts": {
    "terms": "glossary"
  },
  "fingerprint": {
    "files": ["."],
    "dependencies": "direct"
  },
  "evals": [
    {
      "id": "terms",
      "title": "Every bold term is defined in the glossary",
      "profile": {
        "kind": "runtime",
        "command": "python3",
        "args": [
          "check_terms.py",
          "{terms}/terms.txt",
          "{guide}/intro/page.md",
          "{usage}/page.md"
        ],
        "timeoutMs": 10000
      },
      "payload": {
        "instruction": "Check that every bold term in {intro} and {usage} is defined in {terms}."
      }
    }
  ]
}
```

Declarations use `evals`, with qualified eval IDs such as `green/check`.

## The fingerprint field

`fingerprint` declares what a review depends on. artifactize hashes it: while
the fingerprint is unchanged, the prior verdict is reused; when it changes, the
Artifact is reviewed again. It takes one of two forms:

- `{"files":["."],"dependencies":"direct","ignore":[]}`, the built-in
  [content fingerprint](fingerprints-and-reuse.md#content-fingerprint). Every field is optional and these
  are the defaults, so `"fingerprint": {}` covers the whole owner folder.
- `{"script":{"command":"/bin/sh","args":["fingerprint.sh"]}}`, an owner-written
  [fingerprint script](../reference/artifactize-json.md#fingerprint-scripts), optionally with
  `files` (paths that must exist, never hashed) and `timeoutMs`; `weight` is rejected.

## Families and validation

One folder can also declare a [family](../reference/artifactize-json.md#artifact-families) of
instances, as in the [family example](https://github.com/lhj6102/artifactize/tree/main/examples/family). Discovery rules
and rejected legacy fields are in
[Declaration validation](../reference/artifactize-json.md#declaration-validation).
