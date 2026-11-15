<script lang="ts">
  // One Artifact of a graph: a neutral face in a neutral outline. Its status shows in two places
  // and nowhere else, both in the status's one colour: its icon before the name and a strip along
  // its bottom. After the name, how many of its evals are GREEN of how many (on a Current, how
  // many would be reused). Where edges reach it or leave it, a port says which by its shape and
  // side (app.css).
  import { Handle, Position, type NodeProps } from "@xyflow/svelte";
  import type { Light, Tally } from "../ipc/client";
  import Icon from "../shell/Icon.svelte";
  import { LIGHTS } from "../shell/icons";

  let {
    data,
    selected,
  }: NodeProps & { data: { name: string; light: Light; tally: Tally | null; open: boolean; inward: boolean; outward: boolean } } = $props();
</script>

<div class="artifact ported light-{data.light}" class:selected class:open={data.open} class:inward={data.inward} class:outward={data.outward}>
  <Handle type="target" position={Position.Left} isConnectable={false} />
  <span class="light"><Icon name={LIGHTS[data.light].icon} /></span>
  <span class="name">{data.name}</span>
  {#if data.tally}<span class="tally" aria-hidden="true">{data.tally.passed}/{data.tally.of}</span>{/if}
  <Handle type="source" position={Position.Right} isConnectable={false} />
</div>

<style>
  .artifact {
    width: 168px;
    height: 44px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px 0 12px;
    border: 1px solid var(--line-node);
    border-radius: var(--radius-control);
    background: var(--surface-panel);
    box-shadow: inset 0 calc(-1 * var(--strip)) 0 var(--light);
    color: var(--ink);
    font-size: var(--text-body);
    font-weight: 500;
  }
  /* The pointer only steps the outline up. */
  .artifact:hover {
    border-color: var(--ink-3);
  }
  /* The opened Artifact: its own outline turned neutral-bright and 2px, with no gap; its face
     and strip unchanged. */
  .artifact.selected,
  .artifact.open {
    border-color: var(--outline-selected);
    box-shadow: 0 0 0 1px var(--outline-selected), inset 0 calc(-1 * var(--strip)) 0 var(--light);
  }
  .light {
    display: inline-flex;
    color: var(--light);
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tally {
    flex: none;
    color: var(--ink-2);
    font-size: var(--text-label);
    font-weight: 400;
    font-variant-numeric: tabular-nums;
  }
</style>
