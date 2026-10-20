<script lang="ts">
  // The Run graph: Artifacts and their dependency edges, lit by the backend's `run_graph` from
  // the same snapshot as the Run's tree of Artifacts and evals. Every accepted snapshot recolours the graph in place; only a
  // change of Artifacts or edges lays it out again, so nodes never jump on a state change.
  import { SvelteFlowProvider } from "@xyflow/svelte";
  import type { Attachment } from "svelte/attachments";
  import "@xyflow/svelte/dist/style.css";
  import { call, useIpc, message, type RunGraph } from "../ipc/client";
  import { RunGraphState, setRunGraphState } from "./run-graph-state.svelte";
  import GraphView from "./GraphView.svelte";
  import { LIMIT, shape } from "./graph";

  let {
    run,
    revision,
    opened = null,
    cover = null,
    onopen,
    onclose,
  }: {
    run: string;
    /** The browse revision: a new one means a new snapshot to read the graph from. */
    revision: number;
    /** The Artifact whose Detail is open. */
    opened?: string | null;
    /** The open Detail, docked over the graph. */
    cover?: HTMLElement | null;
    /** Open the Detail of an Artifact. */
    onopen: (id: string) => void;
    /** Close the open Detail. */
    onclose: () => void;
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
      <GraphView graph={g.shown.graph} positions={g.shown.positions} {opened} {cover} {onopen} {onclose} />
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
