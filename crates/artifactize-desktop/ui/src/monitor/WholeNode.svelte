<script lang="ts">
  // A whole of a graph: a box around its parts' nodes, its name and how it is lit at its top. It
  // is an Artifact of its own: the pointer and the keyboard reach it as they reach a node, and
  // its parts, drawn above it, take the pointer within their own boxes.
  import { Handle, Position, type NodeProps } from "@xyflow/svelte";
  import type { Light } from "../ipc/client";
  import { BOX } from "./parts";

  let { data, selected }: NodeProps & { data: { name: string; light: Light; open: boolean } } = $props();
</script>

<div class="whole light-{data.light}" class:selected class:open={data.open} style:--header="{BOX.header}px" style:--pad="{BOX.pad}px">
  <Handle type="target" position={Position.Left} isConnectable={false} />
  <div class="title">
    <span class="dot" aria-hidden="true"></span>
    <span class="name">{data.name}</span>
  </div>
  <Handle type="source" position={Position.Right} isConnectable={false} />
</div>

<style>
  .whole {
    width: 100%;
    height: 100%;
    box-sizing: border-box;
    border: 1.5px solid var(--light);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--light) 5%, transparent);
    color: var(--text);
  }
  .light-waiting {
    background: color-mix(in srgb, var(--surface) 40%, transparent);
  }
  /* The pointer only highlights the whole under it. */
  .whole:hover {
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 30%, transparent);
  }
  .selected,
  .open {
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 45%, transparent);
  }
  .title {
    height: var(--header);
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 calc(var(--pad) - 2px);
    font-weight: 600;
  }
  .dot {
    flex: none;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--light);
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .whole :global(.svelte-flow__handle) {
    opacity: 0;
    pointer-events: none;
  }
</style>
