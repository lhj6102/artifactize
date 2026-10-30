<script lang="ts">
  // One Artifact of a graph: its name and how it is lit, nothing about its evals.
  import { Handle, Position, type NodeProps } from "@xyflow/svelte";
  import type { Light } from "../ipc/client";

  let { data, selected }: NodeProps & { data: { name: string; light: Light; open: boolean } } = $props();
</script>

<div class="artifact light-{data.light}" class:selected class:open={data.open}>
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
  /* The pointer only highlights the Artifact under it. */
  .artifact:hover {
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 30%, transparent);
  }
  .selected,
  .open {
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
