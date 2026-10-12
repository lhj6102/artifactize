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
    active = true,
    onselect,
    onopen,
  }: {
    run: string;
    /** The browse revision: a new one means a new snapshot to read the graph from. */
    revision: number;
    tree: TreeNode[];
    selected: string[];
    zoomed?: string | null;
    /** The graph's screen is shown; a hidden graph holds no keyboard focus. */
    active?: boolean;
    onselect: (path: string[]) => void;
    onopen: () => void;
  } = $props();

  let shown = $state.raw<{ graph: RunGraph; positions: Map<string, Point>; shape: string } | null>(null);
  let pending = $state.raw<RunGraph | null>(null);
  let error = $state<string | null>(null);
  /** Lay out a graph above the limit anyway, for this Run. */
  let forced = $state(false);
  /** Each read of the graph; only the newest one may show what it read. */
  let reads = 0;
  /** Layouts by Run and shape, finished or in flight, so returning to a Run reuses its own. */
  const layouts = new Map<string, Promise<Map<string, Point>>>();
  const KEPT = 8;

  function positions(graph: RunGraph, key: string): Promise<Map<string, Point>> {
    const id = `${graph.run}\n${key}`;
    let laid = layouts.get(id);
    if (!laid) {
      laid = layout(graph);
      laid.catch(() => layouts.delete(id));
      layouts.set(id, laid);
      // Forget the oldest beyond a few Runs.
      for (const old of [...layouts.keys()].slice(0, Math.max(layouts.size - KEPT, 0))) layouts.delete(old);
    }
    return laid;
  }

  /** Show `graph` as read `read`: its lights at once when its shape is laid out, else once
   * its layout finishes, unless a newer read came meanwhile. */
  async function place(graph: RunGraph, read: number) {
    const key = shape(graph);
    if (shown && shown.shape === key && shown.graph.run === graph.run) {
      shown = { ...shown, graph };
      pending = null;
      return;
    }
    if (graph.nodes.length > LIMIT && !forced) {
      pending = graph;
      return;
    }
    pending = null;
    const laid = await positions(graph, key);
    if (read !== reads) return;
    shown = { graph, positions: laid, shape: key };
  }

  let last: string | null = null;
  $effect(() => {
    void revision;
    const at = run;
    const read = ++reads;
    if (at !== last) {
      last = at;
      forced = false;
    }
    call(commands.runGraph(at)).then(
      (graph) => {
        if (read !== reads) return;
        error = null;
        void place(graph, read).catch((failure) => read === reads && (error = message(failure)));
      },
      (failure) => {
        if (read === reads) error = message(failure);
      },
    );
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
        if (pending) void place(pending, reads);
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
      <GraphView graph={shown.graph} positions={shown.positions} {tree} {selected} bind:zoomed {active} {onselect} {onopen} />
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
