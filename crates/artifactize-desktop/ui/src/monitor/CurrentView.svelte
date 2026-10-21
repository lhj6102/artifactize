<script lang="ts">
  // A branch's Current: its worktree as it is now, scanned as `artifactize status` does. The
  // header says when it was scanned; Refresh scans again. The graph marks each Artifact reused,
  // would run (with how many evals), waiting or blocked; the structure text below it lists
  // every criterion and labeled relation.
  import type { BranchKey } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";

  let {
    branch,
    opened = null,
    cover = null,
    onopen,
    onclose,
  }: {
    branch: BranchKey;
    /** The Artifact whose Detail is open. */
    opened?: string | null;
    /** The open Detail, docked over the graph. */
    cover?: HTMLElement | null;
    onopen: (id: string) => void;
    onclose: () => void;
  } = $props();
  const { current } = getAppState();

  const view = $derived(current.of(branch) ? current.view : null);
  /** When it was scanned, local time. */
  const scanned = $derived(view?.scannedAt ? new Date(view.scannedAt).toLocaleTimeString() : null);
</script>

<section class="current" aria-label="Current">
  <header>
    <span class="when" role="status">
      {#if current.scanning}Scanning…{:else if scanned}Scanned at {scanned}{/if}
    </span>
    <button class="button small" disabled={current.scanning} onclick={() => void current.scan(branch)}>Refresh</button>
  </header>
  {#if current.error}<p class="failure">{current.error}</p>{/if}
  {#if view}
    <div class="canvas">
      {#await import("./SourceGraph.svelte")}
        <p class="note">Loading the graph…</p>
      {:then { default: SourceGraph }}
        <SourceGraph source={{ kind: "current", branch }} revision={view.revision} {opened} {cover} {onopen} {onclose} />
      {/await}
    </div>
    <details bind:open={current.structure}>
      <summary>Structure and evaluation criteria · {view.fingerprint}</summary>
      <pre>{view.structure}</pre>
    </details>
  {:else if !current.error}<p class="note">Scanning the worktree…</p>{/if}
</section>

<style>
  .current {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    min-width: 0;
  }
  header {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 12px;
    padding: 0 var(--pad) 6px;
  }
  .when {
    color: var(--muted);
  }
  .canvas {
    position: relative;
    flex: 1;
    min-height: 200px;
    min-width: 0;
  }
  details {
    padding: var(--pad);
    overflow: auto;
    max-height: 40vh;
  }
  summary {
    overflow-wrap: anywhere;
    cursor: pointer;
  }
  pre {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .note {
    color: var(--muted);
    padding: 0 var(--pad);
  }
  .failure {
    color: var(--red);
    padding: 0 var(--pad);
    overflow-wrap: anywhere;
  }
</style>
