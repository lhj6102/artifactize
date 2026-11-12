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
    border: 1px solid var(--light);
    border-radius: var(--radius-control);
    background: var(--surface-panel);
    color: var(--ink);
    font-weight: 500;
  }
  /* The pointer only highlights the Artifact under it: a thin neutral outline. */
  .artifact:hover {
    outline: 1px solid var(--ink-3);
    outline-offset: 1px;
  }
  /* The opened Artifact: a neutral outline outside its border, its face unchanged. */
  .artifact.selected,
  .artifact.open {
    outline: 2px solid var(--outline-selected);
    outline-offset: 1px;
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
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
