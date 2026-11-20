# Observe a Live Artifact

Requires Node.js 22 or newer and a build of artifactize that reads `.artifactize/` definitions.
From this folder, run `artifactize verify --all --wait` with network access.

`.artifactize/website.artf` names `website`; its URI target is external state, not a file.
artifactize never fetches it. `observe.mjs` receives the exact URI as argv, runs from this
project folder with your terminal environment, and performs the read-only HTTP check.
The runtime eval has no reuse key and runs on every verify, even if the definition stays
unchanged. Offline or non-200 responses fail with the address and a diagnostic.

This node has no snapshot fingerprint. For an Agent or Human review that should be reused
while observed state remains unchanged, declare a fingerprint script that prints a stable
snapshot identifier. File fingerprints and file builtins do not apply to Live targets.
