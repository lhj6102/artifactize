<script lang="ts">
  // The screen switch: Monitor and Review (after Graph when launched on one). Review shows the
  // number of Human reviews waiting beside its name while any wait, and nothing more.
  import { Tabs } from "bits-ui";
  import { getAppState } from "./app.svelte";
  import { tone } from "./tone";

  const app = getAppState();
  const waiting = $derived(app.waiting.count);
</script>

<Tabs.Root bind:value={app.screen}>
  <Tabs.List class="tabs" aria-label="Screens">
    {#if app.launch?.mode === "graph"}<Tabs.Trigger class="tab" value="graph">Graph</Tabs.Trigger>{/if}
    <Tabs.Trigger class="tab" value="monitor">Monitor</Tabs.Trigger>
    <Tabs.Trigger class="tab" value="review"
      >Review{#if waiting > 0}<span class="count {tone('WAITING_HUMAN')}">{` ${waiting}`}</span>{/if}</Tabs.Trigger
    >
  </Tabs.List>
</Tabs.Root>

<style>
  :global(.tabs) {
    display: flex;
    gap: 4px;
    background: var(--surface-2);
    padding: 3px;
    border-radius: 8px;
  }
  :global(.tab) {
    font: inherit;
    border: 0;
    background: transparent;
    color: var(--muted);
    padding: 3px 12px;
    border-radius: 6px;
    cursor: pointer;
  }
  :global(.tab[data-state="active"]) {
    background: var(--surface);
    color: var(--text);
    box-shadow: 0 1px 2px rgb(0 0 0 / 10%);
  }
  .count {
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
</style>
