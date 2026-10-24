<script lang="ts">
  // A space's box behind its Artifacts, labelled with the space's name. It is no Artifact: the
  // keyboard passes it by, and a click on it does what a click on the graph's background does.
  import type { NodeProps } from "@xyflow/svelte";
  import { SPACE } from "./spaces";

  let { data }: NodeProps & { data: { space: string } } = $props();
</script>

<div class="space" style:--header="{SPACE.header}px">
  <span class="label">{data.space}</span>
</div>

<style>
  .space {
    width: 100%;
    height: 100%;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--surface) 55%, transparent);
  }
  .label {
    display: block;
    height: var(--header);
    line-height: var(--header);
    padding: 0 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
    font-size: 12px;
    font-weight: 600;
  }
</style>
