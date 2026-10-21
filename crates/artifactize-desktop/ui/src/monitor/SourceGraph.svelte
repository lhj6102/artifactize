<script lang="ts">
  // The graph of a Run or a Current: Artifacts and their dependency edges, lit by the backend's
  // `graph` from the same snapshot as the Run's tree (or the Current's scan). Every new snapshot
  // or scan recolours the graph in place; only a change of Artifacts or edges lays it out again,
  // so nodes never jump on a state change.
  import { SvelteFlowProvider } from "@xyflow/svelte";
  import type { Attachment } from "svelte/attachments";
  import "@xyflow/svelte/dist/style.css";
  import { call, useIpc, message, type Graph, type Source } from "../ipc/client";
  import { SourceGraphState, setSourceGraphState } from "./source-graph-state.svelte";
  import GraphView from "./GraphView.svelte";
  import { LIMIT, shape } from "./graph";
  import { keyOf } from "./monitor-state.svelte";

  let {
    source,
    revision,
    opened = null,
    cover = null,
    onopen,
    onclose,
  }: {
    source: Source;
    /** The snapshot's or scan's revision: a new one means a new graph to read. */
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
  const g = new SourceGraphState();
  setSourceGraphState(g);
  const current = $derived(source.kind === "current");
  const key = $derived(keyOf(source));

  /** Show `graph` as read `read`: its lights at once when its shape is laid out, else once
   * its layout finishes, unless a newer read came meanwhile. */
  async function place(graph: Graph, read: number) {
    const laid = shape(graph);
    if (g.shown && g.shownShape === laid && keyOf(g.shown.graph.source) === keyOf(graph.source)) {
      g.shown = { ...g.shown, graph };
      g.pending = null;
      return;
    }
    if (graph.nodes.length > LIMIT && !g.forced) {
      g.pending = graph;
      return;
    }
    g.pending = null;
    const positions = await g.positions(graph, laid);
    if (read !== g.reads) return;
    g.shown = { graph, positions };
  }

  /** Read the graph again for every new snapshot or scan, and for another source. */
  const reread: Attachment<HTMLElement> = () => {
    void revision;
    const [at, wanted] = [key, source];
    const read = ++g.reads;
    if (at !== g.last) {
      g.last = at;
      g.forced = false;
    }
    call(commands.graph(wanted)).then(
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
      This graph has {g.pending.nodes.length} Artifacts; it lays out at most {LIMIT} at once, which can take a while.
    </p>
    <button
      class="button"
      onclick={() => {
        g.forced = true;
        if (g.pending) void place(g.pending, g.reads);
      }}>Lay out all {g.pending.nodes.length}</button
    >
  </div>
{:else if !g.shown || keyOf(g.shown.graph.source) !== key}
  <p class="note">Laying out…</p>
{:else if g.shown.graph.nodes.length === 0}
  <p class="note">{current ? "This worktree declares no Artifacts." : "This Run has no Artifacts."}</p>
{:else}
  {#key keyOf(g.shown.graph.source)}
    <SvelteFlowProvider>
      <GraphView graph={g.shown.graph} positions={g.shown.positions} {current} {opened} {cover} {onopen} {onclose} />
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
