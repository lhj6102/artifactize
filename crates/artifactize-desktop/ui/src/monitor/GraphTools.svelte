<script lang="ts" module>
  /** What the strip's controls do on a graph. */
  export type GraphActions = {
    find?: () => void;
    zoomBy: (factor: number) => void;
    fit: () => void;
  };
</script>

<script lang="ts">
  // The graph's tool strip: Find, the zoom group with the zoom in percent, and Fit, then what
  // the caller adds. Without `act` there is nothing to act on yet (a Run loading or unreadable):
  // the controls stay in their place, unavailable, not hidden (D-MAIN-3).
  import type { Snippet } from "svelte";
  import { ZOOM_STEP } from "./graph";
  import Icon from "../shell/Icon.svelte";
  import { label } from "../shell/keys";

  let { zoom = 1, act = null, showFind = true, children }: {
    /** The camera's zoom, 1 for 100%. */
    zoom?: number;
    act?: GraphActions | null;
    showFind?: boolean;
    /** Further buttons after Fit. */
    children?: Snippet;
  } = $props();
</script>

<div class="tools" role="toolbar" aria-label="Graph tools">
  {#if showFind}<button class="button small" aria-label="Find ({label('Ctrl+F')})" aria-keyshortcuts="Control+F" disabled={!act?.find} onclick={() => act?.find?.()}><Icon name="find" />Find</button>{/if}
  <span class="zoom">
    <button aria-label="Zoom out" disabled={!act} onclick={() => act?.zoomBy(1 / ZOOM_STEP)}><Icon name="minus" /></button>
    <span class="percent" aria-label="Zoom">{Math.round(zoom * 100)}%</span>
    <button aria-label="Zoom in" disabled={!act} onclick={() => act?.zoomBy(ZOOM_STEP)}><Icon name="add" /></button>
  </span>
  <button class="button small" disabled={!act} onclick={() => act?.fit()}><Icon name="fit" />Fit</button>
  {@render children?.()}
</div>

<style>
  /* The tool strip: one row on the panel, opened above by a group line. */
  .tools {
    display: flex;
    align-items: center;
    gap: var(--gap);
    height: var(--row-head);
    padding: 0 var(--pad);
    border-top: 1px solid var(--line-group);
    background: var(--surface-panel);
    font-size: var(--text-small);
    white-space: nowrap;
    overflow: hidden;
  }
  .tools :global(.button) {
    flex: none;
  }
  /* One bordered group: Zoom out, the zoom in percent, Zoom in. */
  .zoom {
    flex: none;
    display: inline-flex;
    align-items: center;
    height: var(--row-dense);
    border: 1px solid var(--line-group);
    border-radius: var(--radius-control);
    background: var(--surface-group);
    overflow: hidden;
  }
  .zoom button {
    display: grid;
    place-items: center;
    width: 22px;
    height: 20px;
    margin: 0;
    padding: 0;
    border: 0;
    background: none;
    color: var(--ink-2);
    cursor: pointer;
  }
  /* The pointer steps the icon's ink up; the face stays the group's. */
  .zoom button:hover:not(:disabled) {
    color: var(--ink);
  }
  .zoom button:disabled {
    color: var(--ink-3);
    cursor: default;
  }
  .zoom button:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
  }
  .percent {
    width: 44px;
    height: 20px;
    line-height: 20px;
    text-align: center;
    border-left: 1px solid var(--line-group);
    border-right: 1px solid var(--line-group);
    color: var(--ink);
    font-variant-numeric: tabular-nums;
  }
</style>
