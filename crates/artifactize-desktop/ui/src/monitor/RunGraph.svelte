<script lang="ts">
  // The Run graph: Artifacts and their dependency edges, lit by the backend's `run_graph` from
  // the same snapshot as the tree. Every accepted snapshot recolours the graph in place; only a
  // change of Artifacts or edges lays it out again, so nodes never jump on a state change.
  import { SvelteFlowProvider } from "@xyflow/svelte";
  import "@xyflow/svelte/dist/style.css";
  import { call, commands, message, type RunGraph, type TreeNode } from "../ipc/client";
  import GraphView from "./GraphView.svelte";
  import { LIMIT, shape, type Point } from "./graph";
  import { layout } from "./layout";

  let {
    run,
    revision,
    tree,
    selected,
    zoomed = $bindable(null),
    onselect,
    onopen,
  }: {
    run: string;
    /** The browse revision: a new one means a new snapshot to read the graph from. */
    revision: number;
    tree: TreeNode[];
    selected: string[];
    zoomed?: string | null;
    onselect: (path: string[]) => void;
    onopen: () => void;
  } = $props();

  let shown = $state.raw<{ graph: RunGraph; positions: Map<string, Point>; shape: string } | null>(null);
  let pending = $state.raw<RunGraph | null>(null);
  let error = $state<string | null>(null);
  /** Lay out a graph above the limit anyway, for this Run. */
  let forced = $state(false);

  async function place(graph: RunGraph) {
    const key = shape(graph);
    if (shown && shown.shape === key && shown.graph.run === graph.run) {
      shown = { ...shown, graph };
      return;
    }
    if (graph.nodes.length > LIMIT && !forced) {
      pending = graph;
      return;
    }
    pending = null;
    const positions = await layout(graph);
    shown = { graph, positions, shape: key };
  }

  $effect(() => {
    void revision;
    const at = run;
    let current = true;
    call(commands.runGraph(at)).then(
      (graph) => {
        if (!current) return;
        error = null;
        void place(graph).catch((failure) => current && (error = message(failure)));
      },
      (failure) => {
        if (current) error = message(failure);
      },
    );
    return () => {
      current = false;
    };
  });
</script>

{#if error}
  <p class="note error">{error}</p>
{:else if pending}
  <div class="note">
    <p>
      This Run has {pending.nodes.length} Artifacts; the graph lays out at most {LIMIT} at once, which can take a while.
      The tree shows every Run at once (<kbd>v</kbd>).
    </p>
    <button
      class="button"
      onclick={() => {
        forced = true;
        if (pending) void place(pending);
      }}>Lay out all {pending.nodes.length}</button
    >
  </div>
{:else if !shown}
  <p class="note">Laying out…</p>
{:else if shown.graph.nodes.length === 0}
  <p class="note">This Run has no Artifacts.</p>
{:else}
  {#key shown.graph.run}
    <SvelteFlowProvider>
      <GraphView graph={shown.graph} positions={shown.positions} {tree} {selected} bind:zoomed {onselect} {onopen} />
    </SvelteFlowProvider>
  {/key}
{/if}

<style>
  .note {
    color: var(--muted);
    padding: 0 var(--pad);
  }
  .note.error {
    color: var(--red);
  }
</style>
