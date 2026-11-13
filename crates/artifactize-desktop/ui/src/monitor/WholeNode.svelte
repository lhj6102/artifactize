<script lang="ts">
  // A whole of a graph. Drawn as a box (`whole`), it is a box around its parts' nodes, its name,
  // how it is lit and its Minimize button at its top. Minimized (`minimized`), it is a node of a
  // node's size, its parts hidden: its name, how it is lit, how many parts it has and its Expand
  // button. It is an Artifact of its own: the pointer and the keyboard reach it as they reach a
  // node, its button after it, and the parts of a box, drawn above it, take the pointer within
  // their own boxes. A click on its button only minimizes or expands it, as `m` does (GraphView).
  import { Handle, Position, type NodeProps } from "@xyflow/svelte";
  import type { Light } from "../ipc/client";
  import { BOX, partsCount } from "./parts";

  let {
    data,
    selected,
    type,
  }: NodeProps & { data: { name: string; light: Light; open: boolean; inward: boolean; outward: boolean; count: number; onminimize: () => void } } = $props();
  const minimized = $derived(type === "minimized");
  const action = $derived(minimized ? "Expand" : "Minimize");

  function toggle(event: MouseEvent) {
    // The click is the button's, not the node's: it opens no Detail.
    event.stopPropagation();
    data.onminimize();
  }
</script>

<div
  class="whole ported light-{data.light}"
  class:inward={data.inward}
  class:outward={data.outward}
  class:minimized
  class:selected
  class:open={data.open}
  style:--header="{BOX.header}px"
  style:--pad="{BOX.pad}px"
>
  <Handle type="target" position={Position.Left} isConnectable={false} />
  <div class="title">
    <span class="dot" aria-hidden="true"></span>
    <span class="text">
      <span class="name">{data.name}</span>
      {#if minimized}<span class="count">{partsCount(data.count)}</span>{/if}
    </span>
    <button class="toggle nodrag nopan" type="button" title="{action} (m)" aria-label="{action} {data.name}" aria-keyshortcuts="m" onclick={toggle}>
      <svg viewBox="0 0 12 12" aria-hidden="true">
        {#if minimized}
          <path d="M2 6h8M6 2v8" />
        {:else}
          <path d="M2 6h8" />
        {/if}
      </svg>
    </button>
  </div>
  <Handle type="source" position={Position.Right} isConnectable={false} />
</div>

<style>
  .whole {
    width: 100%;
    height: 100%;
    box-sizing: border-box;
    border: 1px solid var(--light);
    border-radius: var(--radius-control);
    background: color-mix(in srgb, var(--surface-panel) 40%, transparent);
    color: var(--ink);
  }
  /* Minimized, it is a node's neutral face, with a second edge behind it for the parts it holds. */
  .minimized {
    background: var(--surface-panel);
    box-shadow: 3px 3px 0 -1px var(--surface-app), 3px 3px 0 0 var(--light);
  }
  /* The pointer only highlights the whole under it: a thin neutral outline. */
  .whole:hover {
    outline: 1px solid var(--ink-3);
    outline-offset: 1px;
  }
  /* The opened whole: a neutral outline outside its border, its face unchanged. */
  .whole.selected,
  .whole.open {
    outline: 2px solid var(--outline-selected);
    outline-offset: 1px;
  }
  .title {
    height: var(--header);
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 6px 0 calc(var(--pad) - 2px);
    font-weight: 600;
  }
  .minimized .title {
    height: 100%;
    padding-left: 12px;
    font-weight: 500;
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--light);
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: 1.2;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    font-size: var(--text-label);
    font-weight: 400;
    color: var(--ink-2);
    font-variant-numeric: tabular-nums;
  }
  .toggle {
    flex: none;
    width: 20px;
    height: 20px;
    display: grid;
    place-items: center;
    padding: 0;
    border: 1px solid var(--line-group);
    border-radius: var(--radius-control);
    background: var(--surface-panel);
    color: var(--ink-2);
    cursor: pointer;
  }
  .toggle:hover {
    background: var(--surface-group);
    color: var(--ink);
  }
  .toggle:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: 1px;
  }
  .toggle svg {
    width: 10px;
    height: 10px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-linecap: round;
  }
</style>
