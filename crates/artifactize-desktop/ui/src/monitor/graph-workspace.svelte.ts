// The controls around a graph belong to its workspace, not to the live graph's source.
import type { Opened } from "./monitor-state.svelte";

export type Focus = "pane" | "main" | "detail";

export class GraphWorkspace {
  focus = $state<Focus>("pane");
  finding = $state(false);
  opened = $state.raw<Opened | null>(null);
  graphError = $state<string | null>(null);
  readonly panes: { pane?: HTMLElement; main?: HTMLElement; detail?: HTMLElement; search?: HTMLInputElement } = $state({});
}
