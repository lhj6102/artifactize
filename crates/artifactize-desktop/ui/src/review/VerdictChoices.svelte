<script lang="ts">
  import type { Verdict } from "../ipc/client";
  import type { Review } from "./review.svelte";

  let { review, selected = null, shortcuts = false }: { review: Review; selected?: Verdict | null; shortcuts?: boolean } = $props();
  const verdicts = ["GREEN", "RED"] as const;
  const key = (verdict: Verdict) => verdict === "GREEN" ? "g" : "r";
</script>

<div class="verdicts" role={selected === null ? undefined : "radiogroup"} aria-label={selected === null ? undefined : "Verdict"}>
  {#each verdicts as verdict (verdict)}
    <button
      class="button verdict {verdict.toLowerCase()}"
      role={selected === null ? undefined : "radio"}
      aria-checked={selected === null ? undefined : verdict === selected}
      aria-label={shortcuts ? `${verdict} (${key(verdict)})` : undefined}
      aria-keyshortcuts={shortcuts ? key(verdict) : undefined}
      onclick={() => void review.choose(verdict)}
    >{verdict}</button>
  {/each}
</div>

<style>
  .verdicts { display: flex; flex-wrap: wrap; gap: 6px; }
  .verdict { color: var(--ink-2); }
  .verdict::before {
    content: "";
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }
  .verdict.green::before { background: var(--green); }
  .verdict.red::before { background: var(--red); }
  .verdict[aria-checked="true"],
  .verdict[aria-checked="true"]:hover:not(:disabled) {
    color: var(--ink);
    border-color: var(--outline-selected);
  }
</style>
