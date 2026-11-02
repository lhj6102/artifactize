// The executable journey catalog is explicit: old node:test suites are not runtime inputs.
import { scenarios as accounts } from "./cases/accounts.ts";
import { scenarios as settings } from "./cases/settings.ts";
import { scenarios as currentGraph } from "./cases/current-graph.ts";
import { scenarios as graph } from "./cases/graph.ts";
import { scenarios as detailContent } from "./cases/detail-content.ts";
import { scenarios as catalogLayout } from "./cases/catalog-layout.ts";
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
import { scenarios as branches } from "./cases/branches.ts";
import { scenarios as release } from "./cases/release.ts";
import { scenarios as workbench } from "./cases/workbench.ts";
import type { Scenario } from "./scenario.ts";
import { supports } from "./platform.ts";

const declared: Scenario[] = [
  ...accounts, ...settings, ...currentGraph, ...graph, ...navigation, ...states,
  ...detailContent, ...catalogLayout, ...monitor, ...review, ...canonicalReview, ...humanSubmitFocus, ...launch,
  ...requests, ...search, ...palette, ...branches, ...release, ...workbench,
];

const ids = new Set<string>();
for (const item of declared) {
  if (!/^[a-z0-9][a-z0-9-]*$/.test(item.id) || ids.has(item.id)) throw new Error(`invalid or duplicate desktop scenario id: ${item.id}`);
  ids.add(item.id);
}

/** The scenarios this system can run: a case that needs what it does not offer is left out. */
export const scenarios: Scenario[] = declared.filter((item) => supports(item.needs));

export function selectedScenarios(selected: string[]): Scenario[] {
  if (selected.length === 0) return scenarios;
  for (const id of selected) if (!ids.has(id)) throw new Error(`unknown desktop scenario: ${id}`);
  return scenarios.filter((item) => selected.includes(item.id));
}
