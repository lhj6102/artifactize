<script lang="ts">
  // A branch's Current: its worktree as it is now, scanned as `artifactize status` does. The
  // graph comes first, every Artifact still being checked, and marks each Artifact reused, would
  // run (with how many evals), waiting or blocked as the scan learns it. The header counts the
  // Artifacts still being checked, then says when it was scanned; Refresh scans again. The
  // structure text below lists every criterion and labeled relation, read when expanded.
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
    onopen: (id: string) => void;
    onclose: () => void;
  } = $props();
  const { current } = getAppState();

  const view = $derived(current.of(branch) ? current.view : null);
  /** When it was scanned: the instant, and its local time. */
  const scanned = $derived(view?.scannedAt ? { at: new Date(view.scannedAt).toISOString(), local: new Date(view.scannedAt).toLocaleTimeString() } : null);
</script>

<section class="current" aria-label="Current">
  <header>
    <span class="when" role="status">
      {#if current.scanning}{#if view?.scanning && view.checking > 0}Checking {view.checking} of {view.artifacts} Artifacts…{:else}Scanning…{/if}{:else if scanned}Scanned at <time datetime={scanned.at}>{scanned.local}</time>{/if}
    </span>
    <button class="button small" disabled={current.scanning} onclick={() => void current.scan(branch, true)}>Refresh</button>
  </header>
  {#if current.error}<p class="failure">{current.error}</p>{/if}
  {#if view}
    <div class="canvas">
      {#await import("./SourceGraph.svelte")}
        <p class="note">Loading the graph…</p>
      {:then { default: SourceGraph }}
        <SourceGraph source={{ kind: "current", branch }} revision={view.revision} {opened} {onopen} {onclose} />
      {/await}
    </div>
    <details open={current.structure} ontoggle={(event) => void current.expand(event.currentTarget.open)}>
      <summary>Structure and evaluation criteria · {view.fingerprint}</summary>
      {#if current.structure}<pre>{current.text?.text ?? "Reading…"}</pre>{/if}
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
