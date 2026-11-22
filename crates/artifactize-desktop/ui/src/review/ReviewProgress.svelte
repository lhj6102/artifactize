<script lang="ts">
  import { getAppState } from "../shell/app.svelte";
  import Icon from "../shell/Icon.svelte";
  import type { Review } from "./review.svelte";

  let { review }: { review: Review } = $props();
  const app = getAppState();
  const seconds = $derived(review.busy ? Math.max(0, Math.floor((app.now - review.busy.since) / 1000)) : 0);
</script>

{#if review.busy}
  <span class="busy" role="status">
    <span class="tone-running"><Icon name="running" label={review.busy.label} /></span>
    <span class="seconds">{seconds}s</span>
    <button class="button small" aria-label="Cancel (Esc)" aria-keyshortcuts="Escape" onclick={() => review.cancel()}>Cancel</button>
  </span>
{:else if review.notice?.error}
  <span class="error" role="status">{review.notice.text}</span>
{/if}

<style>
  .busy {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
  }
  .tone-running { display: inline-flex; }
  .seconds { color: var(--ink-2); }
  .error {
    min-width: 0;
    color: var(--red);
    overflow-wrap: anywhere;
  }
</style>
