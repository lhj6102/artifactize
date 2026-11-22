// The controls around a graph belong to its workspace, not to the live graph's source.
import { SourceGraphState } from "./source-graph-state.svelte";
import type { Opened } from "./monitor-state.svelte";

export type Focus = "pane" | "main" | "detail";

export class GraphWorkspace {
  focus = $state<Focus>("pane");
  finding = $state(false);
  opened = $state.raw<Opened | null>(null);
  readonly graph = new SourceGraphState();
  readonly panes: { pane?: HTMLElement; main?: HTMLElement; detail?: HTMLElement; search?: HTMLInputElement } = $state({});
}
