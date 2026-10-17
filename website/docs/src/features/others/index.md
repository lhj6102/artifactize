# Others

Supporting features keep reviews usable on a real machine:

- [State, cache and limits](state.md) explains where Runs and evidence live,
  cache inspection and removal, concurrent backend capacity, saved Agent
  conversations and retention. `artifactize doctor` checks local readiness without
  calling a provider or starting a Run; `models` lists available model IDs and
  `prune` removes eligible finished output while preserving audit records.
- [Operating systems](platforms.md) covers portable command declarations, executable
  lookup, paths, opening applications, editors and state on Linux, macOS and Windows.
- [Supporting reference](reference.md) links to the detailed schemas, library APIs,
  protocols and limits behind the feature guides.

For command syntax and exit codes, use the [CLI](../../cli/index.md) area.
