<script lang="ts">
  // Attention counters such as `!1 ✗2 ◐1 ?1`.
  import type { Count } from "../ipc/client";
  import { tone } from "./tone";

  let { counts, limit }: { counts: Count[]; limit?: number } = $props();
  const shown = $derived(counts.slice(0, limit ?? counts.length));
</script>

<span class="badge">
  {#each shown as count (count.mark.status)}
    <span class={tone(count.mark.status)} aria-label="{count.mark.status} {count.count}">{count.mark.glyph}{count.count}</span>
  {/each}
</span>

<style>
  .badge {
    display: inline-flex;
    gap: 6px;
    font-variant-numeric: tabular-nums;
  }
</style>
