<script lang="ts">
  // A Current's scan state, the headline of the Main area's header row, said without a
  // sentence: while it runs, the checking icon and how many Artifacts are still being checked,
  // a count that falls; once it ends, the time it was scanned at; only a failure in words.
  import type { BranchKey } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import Icon from "../shell/Icon.svelte";

  let { branch }: { branch: BranchKey } = $props();
  const { current } = getAppState();

  const view = $derived(current.of(branch) ? current.view : null);
  /** When it was scanned: the instant, and its local time. */
  const scanned = $derived(view?.scannedAt ? { at: new Date(view.scannedAt).toISOString(), local: new Date(view.scannedAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23" }) } : null);
  /** How many Artifacts are still being checked while the scan runs. */
  const left = $derived(current.scanning ? (view?.scanning ? view.checking : null) : null);
</script>

<span class="scan" role="status">
  {#if current.scanning}
    <span class="checking" aria-label="Checking, {left ?? 'all'} Artifacts left"><Icon name="checking" /><span aria-hidden="true">{left ?? "…"} left</span></span>
  {:else if scanned}
    <time datetime={scanned.at} aria-label="Scanned at {scanned.local}">{scanned.local}</time>
  {/if}
  {#if current.error}<span class="failure">{current.error}</span>{/if}
</span>

<style>
  .scan {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    color: var(--ink-2);
    font-variant-numeric: tabular-nums;
  }
  .checking {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .checking :global(.icon) {
    color: var(--checking);
  }
  .failure {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--red);
  }
</style>
