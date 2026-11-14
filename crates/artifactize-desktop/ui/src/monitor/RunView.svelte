<script lang="ts">
  // The shown Run's graph: a click on an Artifact opens its Detail. Its headline is the Main
  // area's header row's (RunHeadline).
  import type { BranchKey, RunPanel } from "../ipc/client";

  let {
    run,
    branch,
    revision,
    opened = null,
    onopen,
    onclose,
  }: {
    run: RunPanel;
    /** The branch the Run is of. */
    branch: BranchKey;
    /** The browse revision: a new snapshot relights the graph. */
    revision: number;
    /** The Artifact whose Detail is open. */
    opened?: string | null;
    onopen: (id: string, tab?: string | null) => void;
    onclose: () => void;
  } = $props();
</script>

<div class="run">
  <!-- Loaded on first use: the graph's libraries stay out of the window's start-up. -->
  {#await import("./SourceGraph.svelte")}
    <p class="empty">Loading the Run…</p>
  {:then { default: SourceGraph }}
    <SourceGraph source={{ kind: "run", run: run.id }} {branch} {revision} {opened} {onopen} {onclose} />
  {/await}
</div>

<style>
  .run {
    height: 100%;
    display: grid;
    grid-template-rows: minmax(0, 1fr);
    grid-template-columns: minmax(0, 1fr);
    min-height: 0;
  }
  .empty {
    margin: 0;
    padding: 6px var(--pad);
    color: var(--ink-2);
  }
</style>
