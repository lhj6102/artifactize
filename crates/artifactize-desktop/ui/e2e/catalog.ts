// The executable journey catalog is explicit: old node:test suites are not runtime inputs.
import { scenarios as accounts } from "./cases/accounts.ts";
import { scenarios as settings } from "./cases/settings.ts";
import { scenarios as currentGraph } from "./cases/current-graph.ts";
import { scenarios as graph } from "./cases/graph.ts";
import { scenarios as navigation } from "./cases/navigation.ts";
import { scenarios as states } from "./cases/states.ts";
import { scenarios as monitor } from "./cases/monitor.ts";
import { scenarios as review } from "./cases/review.ts";
import { scenarios as canonicalReview } from "./cases/canonical-review.ts";
import { scenarios as humanSubmitFocus } from "./cases/human-submit-focus.ts";
import { scenarios as launch } from "./cases/launch.ts";
import { scenarios as requests } from "./cases/requests.ts";
import { scenarios as search } from "./cases/search.ts";
import { scenarios as palette } from "./cases/palette.ts";
import type { Scenario } from "./scenario.ts";

export const scenarios: Scenario[] = [
  ...accounts, ...settings, ...currentGraph, ...graph, ...navigation, ...states,
  ...monitor, ...review, ...canonicalReview, ...humanSubmitFocus, ...launch,
  ...requests, ...search, ...palette,
];

const ids = new Set<string>();
for (const item of scenarios) {
  if (!/^[a-z0-9][a-z0-9-]*$/.test(item.id) || ids.has(item.id)) throw new Error(`invalid or duplicate desktop scenario id: ${item.id}`);
  ids.add(item.id);
}

export function selectedScenarios(selected: string[]): Scenario[] {
  if (selected.length === 0) return scenarios;
  for (const id of selected) if (!ids.has(id)) throw new Error(`unknown desktop scenario: ${id}`);
  return scenarios.filter((item) => selected.includes(item.id));
}
