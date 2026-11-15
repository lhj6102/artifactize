<script lang="ts">
  // A branch's Current: its worktree as it is now, scanned as `artifactize status` does. The
  // graph comes first, every Artifact still being checked, and marks each Artifact reused, would
  // run, waiting or blocked as the scan learns it. Its scan's state is the Main area's header
  // row's (ScanState).
  import type { BranchKey } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";

  let {
    branch,
    opened = null,
    onopen,
    onclose,
  }: {
    branch: BranchKey;
    /** The Artifact whose Detail is open. */
    opened?: string | null;
    onopen: (id: string, tab?: string | null) => void;
    onclose: () => void;
  } = $props();
  const { current } = getAppState();

  const view = $derived(current.of(branch) ? current.view : null);
</script>

<section class="current" aria-label="Current">
  {#if view}
    {#await import("./SourceGraph.svelte")}
      <p class="note">Loading the graph…</p>
    {:then { default: SourceGraph }}
      <SourceGraph source={{ kind: "current", branch }} {branch} revision={view.revision} {opened} {onopen} {onclose} />
    {/await}
  {:else if !current.error}<p class="note">Scanning the worktree…</p>{/if}
</section>

<style>
  .current {
    height: 100%;
    display: grid;
    grid-template-rows: minmax(0, 1fr);
    grid-template-columns: minmax(0, 1fr);
    min-height: 0;
    min-width: 0;
  }
  .note {
    margin: 0;
    padding: 6px var(--pad);
    color: var(--ink-2);
  }
</style>
