<script lang="ts">
  // The Run graph: Artifacts and their dependency edges, lit by the backend's `run_graph` from
  // the same snapshot as the tree. Every accepted snapshot recolours the graph in place; only a
  // change of Artifacts or edges lays it out again, so nodes never jump on a state change.
  import { SvelteFlowProvider } from "@xyflow/svelte";
  import type { Attachment } from "svelte/attachments";
  import "@xyflow/svelte/dist/style.css";
  import { call, useIpc, message, type RunGraph, type TreeNode } from "../ipc/client";
  import { RunGraphState, setRunGraphState } from "./runGraph.svelte";
  import GraphView from "./GraphView.svelte";
  import { LIMIT, shape } from "./graph";

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
  const commands = useIpc();
  const g = new RunGraphState();
  setRunGraphState(g);

  /** Show `graph` as read `read`: its lights at once when its shape is laid out, else once
   * its layout finishes, unless a newer read came meanwhile. */
  async function place(graph: RunGraph, read: number) {
    const key = shape(graph);
    if (g.shown && g.shownShape === key && g.shown.graph.run === graph.run) {
      g.shown = { ...g.shown, graph };
      g.pending = null;
      return;
    }
    if (graph.nodes.length > LIMIT && !g.forced) {
      g.pending = graph;
      return;
    }
    g.pending = null;
    const laid = await g.positions(graph, key);
    if (read !== g.reads) return;
    g.shown = { graph, positions: laid };
  }

  /** Read the graph again for every accepted snapshot and for another Run. */
  const reread: Attachment<HTMLElement> = () => {
    void revision;
    const at = run;
    const read = ++g.reads;
    if (at !== g.last) {
      g.last = at;
      g.forced = false;
    }
    call(commands.runGraph(at)).then(
      (graph) => {
        if (read !== g.reads) return;
        g.error = null;
        void place(graph, read).catch((failure) => read === g.reads && (g.error = message(failure)));
      },
      (failure) => {
        if (read === g.reads) g.error = message(failure);
      },
    );
  };
</script>

<div class="host" {@attach reread}>
{#if g.error}
  <p class="note error">{g.error}</p>
{:else if g.pending}
  <div class="note">
    <p>
      This Run has {g.pending.nodes.length} Artifacts; the graph lays out at most {LIMIT} at once, which can take a while.
      The tree shows every Run at once (<kbd>v</kbd>).
    </p>
    <button
      class="button"
      onclick={() => {
        g.forced = true;
        if (g.pending) void place(g.pending, g.reads);
      }}>Lay out all {g.pending.nodes.length}</button
    >
  </div>
{:else if !g.shown}
  <p class="note">Laying out…</p>
{:else if g.shown.graph.nodes.length === 0}
  <p class="note">This Run has no Artifacts.</p>
{:else}
  {#key g.shown.graph.run}
    <SvelteFlowProvider>
      <GraphView graph={g.shown.graph} positions={g.shown.positions} {tree} {selected} bind:zoomed {active} {onselect} {onopen} />
    </SvelteFlowProvider>
  {/key}
{/if}
</div>

<style>
  .host {
    display: contents;
  }
  .note {
    color: var(--muted);
    padding: 0 var(--pad);
  }
  .note.error {
    color: var(--red);
  }
</style>
