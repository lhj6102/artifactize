<script lang="ts">
  // One Artifact of the Run graph: its name and how it is lit, nothing about its evals.
  import { Handle, Position, type NodeProps } from "@xyflow/svelte";
  import type { Light } from "../ipc/client";
  import { WORDS } from "./graph";

  let { data, selected }: NodeProps & { data: { name: string; light: Light; zoomed: boolean; structural?: boolean } } = $props();
</script>

<div class="artifact light-{data.light}" class:selected class:zoomed={data.zoomed} title={data.structural ? `${data.name}: current structure` : `${data.name}: ${WORDS[data.light]}`}>
  <Handle type="target" position={Position.Left} isConnectable={false} />
  <span class="dot" aria-hidden="true"></span>
  <span class="name">{data.name}</span>
  <Handle type="source" position={Position.Right} isConnectable={false} />
</div>

<style>
  .artifact {
    width: 168px;
    height: 44px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    border: 1.5px solid var(--light);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--light) 12%, var(--surface));
    color: var(--text);
    font-weight: 500;
  }
  .light-waiting {
    background: var(--surface);
  }
  .selected,
  .zoomed {
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 45%, transparent);
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
  .artifact :global(.svelte-flow__handle) {
    opacity: 0;
    pointer-events: none;
  }
</style>
