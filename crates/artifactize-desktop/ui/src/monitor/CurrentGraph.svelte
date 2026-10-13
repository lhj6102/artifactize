<script lang="ts">
  // The graph tool uses the monitor's renderer and layout. The text view exposes all
  // extracted definitions, including evaluation criteria and individual relation labels.
  import { onMount } from "svelte";
  import { SvelteFlowProvider } from "@xyflow/svelte";
  import "@xyflow/svelte/dist/style.css";
  import GraphView from "./GraphView.svelte";
  import { CurrentGraphState, setCurrentGraphState } from "./currentGraph.svelte";
  import { chord } from "../shell/keys";

  const graph = new CurrentGraphState();
  setCurrentGraphState(graph);
  onMount(() => void graph.refresh());

  function keydown(event: KeyboardEvent) {
    if (event.defaultPrevented) return;
    if (chord(event, "r") || event.key === "F5") {
      event.preventDefault();
      event.stopPropagation();
      void graph.refresh();
    }
  }
</script>

<svelte:window onkeydown={keydown} />

<section class="current" aria-label="Current Artifact graph">
  <header>
    <strong>Current Artifact graph</strong>
    <button class="button" disabled={graph.reading} onclick={() => void graph.refresh()}>Refresh</button>
    <span class="muted">Declarations only · no Run verdicts</span>
  </header>
  {#if graph.error}<p class="failure">{graph.error}</p>{/if}
  {#if graph.shown}
    <div class="canvas">
      <SvelteFlowProvider>
        <GraphView
          graph={graph.shown.view.graph}
          positions={graph.shown.positions}
          tree={graph.shown.view.tree}
          selected={graph.selected}
          bind:zoomed={graph.zoomed}
          structural
          onselect={(path) => (graph.selected = path)}
          onopen={() => (graph.details = true)}
        />
      </SvelteFlowProvider>
    </div>
    <details bind:open={graph.details}>
      <summary>Structure and evaluation criteria · {graph.shown.view.fingerprint}</summary>
      <pre>{graph.shown.view.structure}</pre>
    </details>
  {:else if !graph.error}<p>Reading declarations and laying out…</p>{/if}
</section>

<style>
  .current { display: flex; flex-direction: column; height: 100%; min-height: 0; width: 100%; }
  header { display: flex; align-items: center; flex-wrap: wrap; gap: 12px; padding: var(--pad); }
  .canvas { flex: 1; min-height: 200px; }
  details { padding: var(--pad); overflow: auto; max-height: 40vh; }
  summary { overflow-wrap: anywhere; cursor: pointer; }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
