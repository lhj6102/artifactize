<script lang="ts">
  // The graph tool uses the monitor's renderer and layout. A click on an Artifact shows its
  // declared evals docked over the graph's right side; the text view exposes all extracted
  // definitions, including evaluation criteria and individual relation labels.
  import { onMount } from "svelte";
  import { SvelteFlowProvider } from "@xyflow/svelte";
  import "@xyflow/svelte/dist/style.css";
  import GraphView from "./GraphView.svelte";
  import Panel from "../shell/Panel.svelte";
  import { CurrentGraphState, setCurrentGraphState } from "./current-graph-state.svelte";
  import { chord, plain } from "../shell/keys";

  import { useIpc } from "../ipc/client";

  const graph = new CurrentGraphState(useIpc());
  setCurrentGraphState(graph);
  onMount(() => void graph.refresh());

  const artifact = $derived(graph.opened ? (graph.shown?.view.tree.find((node) => node.id === graph.opened) ?? null) : null);

  function keydown(event: KeyboardEvent) {
    if (event.defaultPrevented) return;
    if (chord(event, "r") || event.key === "F5") {
      event.preventDefault();
      event.stopPropagation();
      void graph.refresh();
    } else if (plain(event, "Escape") && graph.opened) {
      event.preventDefault();
      graph.opened = null;
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
          opened={graph.opened}
          structural
          onopen={(id) => (graph.opened = id)}
          onclose={() => (graph.opened = null)}
        />
      </SvelteFlowProvider>
      {#if artifact}
        <aside class="dock" aria-label="Detail">
          <Panel title="Artifact {artifact.name}" focused={false}>
            {#snippet actions()}
              <button class="button small" onclick={() => (graph.opened = null)}>Close <kbd>Esc</kbd></button>
            {/snippet}
            <ul class="evals" aria-label="Evals of {artifact.name}">
              {#each artifact.children as item (item.id)}
                <li>
                  <button class="eval" data-eval={item.id} onclick={() => (graph.details = true)}>
                    <span class="name">{item.name}{#if item.marks}<span class="muted"> {item.marks}</span>{/if}</span>
                    {#each item.text as segment, part (part)}<span class="muted">{segment.text}</span>{/each}
                  </button>
                </li>
              {:else}
                <li class="muted none">No evals.</li>
              {/each}
            </ul>
          </Panel>
        </aside>
      {/if}
    </div>
    <details bind:open={graph.details}>
      <summary>Structure and evaluation criteria · {graph.shown.view.fingerprint}</summary>
      <pre>{graph.shown.view.structure}</pre>
    </details>
  {:else if !graph.error}<p>Reading declarations and laying out…</p>{/if}
</section>

<style>
  .current { display: flex; flex-direction: column; height: 100%; min-height: 0; width: 100%; min-width: 0; }
  header { display: flex; align-items: center; flex-wrap: wrap; gap: 12px; padding: var(--pad); }
  .canvas { position: relative; isolation: isolate; flex: 1; min-height: 200px; min-width: 0; }
  details { padding: var(--pad); overflow: auto; max-height: 40vh; }
  summary { overflow-wrap: anywhere; cursor: pointer; }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; }
  .dock {
    position: absolute;
    z-index: 20;
    top: 12px;
    right: 12px;
    max-height: calc(100% - 24px);
    width: min(46%, 480px);
    min-width: min(280px, calc(100% - 24px));
    max-width: calc(100% - 24px);
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 10px 32px rgb(0 0 0 / 22%);
  }
  .evals { list-style: none; margin: 0; padding: 0 0 8px; }
  .eval {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    width: 100%;
    margin: 0;
    padding: 4px var(--pad);
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    overflow-wrap: anywhere;
    cursor: pointer;
  }
  .eval:hover { background: var(--surface-2); }
  .muted { color: var(--muted); }
  .none { padding: 0 var(--pad); }
</style>
