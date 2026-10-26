<script lang="ts">
  // A space's region behind its Artifacts, its rounded outline following them, labelled with the
  // space's name above the topmost. It is no Artifact: the keyboard passes it by, and a click on
  // it does what a click on the graph's background does. Only the region itself takes the
  // pointer: outside its outline, within its box, the pointer reaches the background.
  import type { NodeProps } from "@xyflow/svelte";
  import { SPACE, type Region } from "./spaces";

  let { data }: NodeProps & { data: Pick<Region, "space" | "outline" | "label"> } = $props();
</script>

<svg class="region" aria-hidden="true"><path d={data.outline} /></svg>
<span class="label" style:left="{data.label.x}px" style:width="{data.label.width}px" style:--header="{SPACE.header}px">{data.space}</span>

<style>
  .region {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    overflow: visible;
    pointer-events: none;
  }
  .region path {
    fill: color-mix(in srgb, var(--surface) 55%, transparent);
    stroke: var(--border);
    stroke-width: 1;
    pointer-events: visiblePainted;
  }
  .label {
    position: absolute;
    top: 0;
    height: var(--header);
    line-height: var(--header);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
    font-size: 12px;
    font-weight: 600;
    pointer-events: auto;
  }
</style>
