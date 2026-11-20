<script lang="ts">
  // A declared Artifact in the Artifactize Viewer (D-VIEW-5): its name and its type as a small
  // tag (`custom` for a Custom Artifact), no status. Selected, it carries the selected outline; while pointing, the accent.
  import { Handle, Position, type NodeProps } from "@xyflow/svelte";

  type Data = { name: string; kind: string; selected: boolean; pointing: boolean };
  let { data }: NodeProps & { data: Data } = $props();
</script>

<div class="node" class:selected={data.selected} class:pointing={data.pointing}>
  <Handle type="target" position={Position.Left} isConnectable={false} />
  <span class="name">{data.name}</span>
  <span class="tag">{data.kind || "custom"}</span>
  <Handle type="source" position={Position.Right} isConnectable={false} />
</div>

<style>
  .node {
    display: flex;
    align-items: center;
    gap: 6px;
    box-sizing: border-box;
    width: 168px;
    height: 36px;
    padding: 0 8px;
    border: 1px solid var(--line-node);
    border-radius: var(--radius-control);
    background: var(--surface-panel);
    color: var(--ink);
    font-size: var(--text-small);
    cursor: pointer;
  }
  .node.selected {
    border-color: var(--outline-selected);
    box-shadow: 0 0 0 1px var(--outline-selected);
  }
  .node.pointing:hover {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
    cursor: crosshair;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tag {
    padding: 0 4px;
    border: 1px solid var(--line-group);
    border-radius: var(--radius-chip);
    color: var(--ink-2);
    font: var(--text-label) var(--mono);
  }
  .node :global(.svelte-flow__handle) {
    opacity: 0;
    pointer-events: none;
  }
</style>
