<script lang="ts" module>
  /** What the tools do on a graph. */
  export type GraphActions = {
    find?: () => void;
    zoomBy: (factor: number) => void;
    fit: () => void;
  };
</script>

<script lang="ts">
  // A graph's tools (D-GRAPH-4): Find, the zoom with its percent, and Fit, then what the caller
  // adds. They stand among the stage header's tools (D-MAIN-1), borderless; outside a stage (a
  // viewer window) they stay where they are drawn. Without `act` there is nothing to act on yet
  // (a Run loading or unreadable): the tools stay in their place, unavailable, not hidden
  // (D-MAIN-3). While Find is open its field takes the find icon's place.
  import type { Snippet } from "svelte";
  import { ZOOM_STEP } from "./graph";
  import Icon from "../shell/Icon.svelte";
  import { label } from "../shell/keys";
  import { stageSlot, stageTools } from "../shell/stage-state.svelte";

  let { zoom = 1, act = null, showFind = true, finder, children }: {
    /** The camera's zoom, 1 for 100%. */
    zoom?: number;
    act?: GraphActions | null;
    showFind?: boolean;
    /** The open find field, drawn in the find icon's place. */
    finder?: Snippet;
    /** Further tools after Fit. */
    children?: Snippet;
  } = $props();
  const slot = stageSlot();
</script>

<div class="graph-tools" class:standalone={!slot} role={slot ? "group" : "toolbar"} aria-label="Graph tools" {@attach stageTools(slot)}>
  {#if finder}
    {@render finder()}
  {:else if showFind}
    <button class="tool" aria-label="Find ({label('Ctrl+F')})" title="Find" aria-keyshortcuts="Control+F" disabled={!act?.find} onclick={() => act?.find?.()}><Icon name="find" /></button>
  {/if}
  {#if finder || showFind}<span class="tool-sep"></span>{/if}
  <button class="tool" aria-label="Zoom out" title="Zoom out" disabled={!act} onclick={() => act?.zoomBy(1 / ZOOM_STEP)}><Icon name="minus" /></button>
  <span class="percent" aria-label="Zoom">{Math.round(zoom * 100)}%</span>
  <button class="tool" aria-label="Zoom in" title="Zoom in" disabled={!act} onclick={() => act?.zoomBy(ZOOM_STEP)}><Icon name="add" /></button>
  <span class="tool-sep"></span>
  <button class="tool" disabled={!act} onclick={() => act?.fit()}><Icon name="fit" />Fit</button>
  {#if children}<span class="tool-sep"></span>{@render children()}{/if}
</div>

<style>
  .graph-tools {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
    font-size: var(--text-small);
    white-space: nowrap;
  }
  /* Outside a stage the tools make their own row over the canvas, as the header draws them. */
  .graph-tools.standalone {
    height: 32px;
    padding: 0 var(--pad);
    border-bottom: 1px solid var(--line-panel);
    background: var(--surface-panel);
  }
  .percent {
    flex: none;
    width: 44px;
    text-align: center;
    color: var(--ink);
    font-variant-numeric: tabular-nums;
  }
  /* The stage header styles its tools; standalone, the same grammar. */
  .standalone :global(.tool) {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    min-width: var(--control);
    height: var(--control);
    padding: 0 6px;
    border: 0;
    border-radius: var(--radius-control);
    background: none;
    color: var(--ink-2);
    font: inherit;
    cursor: pointer;
  }
  .standalone :global(.tool:hover:not(:disabled)) {
    color: var(--ink);
  }
  .standalone :global(.tool:disabled) {
    color: var(--ink-3);
  }
  .standalone :global(.tool-sep) {
    width: 1px;
    height: 16px;
    margin: 0 6px;
    background: var(--line-group);
  }
</style>
