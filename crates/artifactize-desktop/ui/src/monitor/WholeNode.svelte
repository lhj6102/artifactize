<script lang="ts">
  // A whole of a graph, in neutral lines. Drawn as a box (`whole`), it is a box around its parts'
  // nodes under a title row: its status's icon, its name, its tally and its Minimize button,
  // closed below by the strip in its status's colour. Minimized (`minimized`), it is a node of a
  // node's size and anatomy, its parts hidden: its icon, its name with how many parts it has
  // under it, its tally, its Expand button, and a second outline behind it. It is an Artifact of its own: the pointer and the keyboard reach it as they reach a
  // node, its button after it, and the parts of a box, drawn above it, take the pointer within
  // their own boxes. A click on its button only minimizes or expands it, as `m` does (GraphView).
  import { Handle, Position, type NodeProps } from "@xyflow/svelte";
  import type { Light, Tally } from "../ipc/client";
  import Icon from "../shell/Icon.svelte";
  import { LIGHTS } from "../shell/icons";
  import { BOX, partsCount } from "./parts";

  let {
    data,
    selected,
    type,
  }: NodeProps & { data: { name: string; light: Light; tally: Tally | null; open: boolean; inward: boolean; outward: boolean; count: number; onminimize: () => void } } = $props();
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
    <span class="light"><Icon name={LIGHTS[data.light].icon} /></span>
    <span class="text">
      <span class="name">{data.name}</span>
      {#if minimized}<span class="count">{partsCount(data.count)}</span>{/if}
    </span>
    {#if data.tally}<span class="tally" aria-hidden="true">{data.tally.passed}/{data.tally.of}</span>{/if}
    <button class="toggle nodrag nopan" type="button" title="{action} (m)" aria-label="{action} {data.name} (m)" aria-keyshortcuts="m" onclick={toggle}>
      <Icon name={minimized ? "add" : "minus"} />
    </button>
  </div>
  <Handle type="source" position={Position.Right} isConnectable={false} />
</div>

<style>
  .whole {
    width: 100%;
    height: 100%;
    box-sizing: border-box;
    border: 1px solid var(--line-node);
    border-radius: var(--radius-control);
    background: color-mix(in srgb, var(--surface-panel) 40%, transparent);
    color: var(--ink);
  }
  /* Minimized, it is a node's neutral face with its strip, and a second outline behind it for
     the parts it holds. */
  .minimized {
    background: var(--surface-panel);
    box-shadow:
      3px 3px 0 -1px var(--surface-app),
      3px 3px 0 0 var(--line-node),
      inset 0 calc(-1 * var(--strip)) 0 var(--light);
  }
  /* The pointer only steps the outline up. */
  .whole:hover {
    border-color: var(--ink-3);
  }
  /* The opened whole: its own outline turned neutral-bright and 2px, with no gap. */
  .whole.selected,
  .whole.open {
    border-color: var(--outline-selected);
    box-shadow: 0 0 0 1px var(--outline-selected);
  }
  /* Opened, it keeps the second outline behind it for the parts it holds. */
  .minimized.selected,
  .minimized.open {
    box-shadow:
      0 0 0 1px var(--outline-selected),
      3px 3px 0 -1px var(--surface-app),
      3px 3px 0 0 var(--line-node),
      inset 0 calc(-1 * var(--strip)) 0 var(--light);
  }
  /* The title row, closed below by the strip in its status's colour across the box. */
  .title {
    height: var(--header);
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 6px 0 calc(var(--pad) - 6px);
    font-weight: 600;
    box-shadow: inset 0 calc(-1 * var(--strip)) 0 var(--light);
  }
  .minimized .title {
    height: 100%;
    padding-left: 12px;
    font-weight: 500;
    box-shadow: none;
  }
  .light {
    display: inline-flex;
    color: var(--light);
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
  .count,
  .tally {
    font-size: var(--text-label);
    font-weight: 400;
    color: var(--ink-2);
    font-variant-numeric: tabular-nums;
  }
  .tally {
    flex: none;
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
    background: var(--surface-group);
    color: var(--ink-2);
    cursor: pointer;
  }
  .toggle:hover {
    border-color: var(--line-input);
    color: var(--ink);
  }
  .toggle:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: 1px;
  }
</style>
