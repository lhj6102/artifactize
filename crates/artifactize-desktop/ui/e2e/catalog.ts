// Every scenario of the suite, in the order the runner batches them.
import type { Scenario } from "./scenario.ts";
import { scenarios as editing } from "./cases/editing.ts";
import { scenarios as graph } from "./cases/graph.ts";
import { scenarios as pane } from "./cases/pane.ts";
import { scenarios as review } from "./cases/review.ts";
import { scenarios as settings } from "./cases/settings.ts";
import { scenarios as viewers } from "./cases/viewers.ts";
import { scenarios as window } from "./cases/window.ts";

import { scenarios as modes } from "./cases/modes.ts";

export const scenarios: Scenario[] = [...window, ...pane, ...graph, ...editing, ...review, ...settings, ...modes, ...viewers];

const ids = new Set<string>();
for (const item of scenarios) {
  if (!/^[a-z0-9][a-z0-9-]*$/.test(item.id) || ids.has(item.id)) throw new Error(`invalid or duplicate scenario id: ${item.id}`);
  ids.add(item.id);
}

/** The scenarios `selected` names, all of them when it names none. */
export function selectedScenarios(selected: string[]): Scenario[] {
  for (const id of selected) if (!ids.has(id)) throw new Error(`unknown desktop scenario: ${id}`);
  return selected.length === 0 ? scenarios : scenarios.filter((item) => selected.includes(item.id));
}
