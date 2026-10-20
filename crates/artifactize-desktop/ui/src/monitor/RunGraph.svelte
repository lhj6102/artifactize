<script lang="ts">
  // The Run graph: Artifacts and their dependency edges, lit by the backend's `run_graph` from
  // the same snapshot as the Run's tree of Artifacts and evals. Every accepted snapshot recolours
  // the graph in place: while its Artifacts and edges keep their shape, only their lights come
  // again, and only a new shape is laid out, so nodes never jump on a state change.
  import { SvelteFlowProvider } from "@xyflow/svelte";
  import { untrack } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import "@xyflow/svelte/dist/style.css";
  import { call, useIpc, message } from "../ipc/client";
  import { RunGraphState, setRunGraphState } from "./run-graph-state.svelte";
  import GraphView from "./GraphView.svelte";
  import { relight } from "./graph";
  import { layout } from "./layout";

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

  /** Read the graph again for every accepted snapshot and for another Run: the lights of the
   * shape shown, or a new shape to lay out. Only the newest read shows what it read. */
  const reread: Attachment<HTMLElement> = () => {
    void revision;
    const at = run;
    // What it holds and its count of reads are its own: showing a read must not read again.
    const [read, held] = untrack(() => [++g.reads, g.shown?.run === at ? g.shown : null] as const);
    call(commands.runGraph(at, held?.shape ?? null)).then(
      (next) => {
        if (read !== g.reads) return;
        g.error = null;
        if (next.update.kind === "lights") {
          const graph = held?.shape === next.shape ? relight(held.graph, next.update) : null;
          if (held && graph && graph !== held.graph) g.shown = { ...held, graph };
          return;
        }
        const graph = { nodes: next.update.nodes, edges: next.update.edges };
        g.shown = { run: next.run, shape: next.shape, graph, positions: layout(graph) };
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
{:else if g.shown?.run !== run}
  <p class="note">Laying out…</p>
{:else if g.shown.graph.nodes.length === 0}
  <p class="note">This Run has no Artifacts.</p>
{:else}
  {#key g.shown.run}
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
