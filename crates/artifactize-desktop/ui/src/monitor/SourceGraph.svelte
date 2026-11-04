<script lang="ts">
  // The graph of a Run or a Current: Artifacts and their dependency edges, lit by the backend's
  // `graph` from the same snapshot as the Run's tree (or the Current's scan). Every new snapshot
  // or scan recolours the graph in place: while its Artifacts and edges keep their shape, only
  // their lights come again (a Current's notes with them), and only a new shape is laid out, so
  // nodes never jump on a state change. While a new shape lays out, the graph shown before stays
  // until its layout is ready. Minimizing or expanding a whole lays the shape out again with the
  // wholes minimized on its branch.
  import { SvelteFlowProvider } from "@xyflow/svelte";
  import { untrack } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import "@xyflow/svelte/dist/style.css";
  import { call, useIpc, message, type BranchKey, type Shape, type ShapeKey, type Source } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import { SourceGraphState, setSourceGraphState } from "./source-graph-state.svelte";
  import GraphView from "./GraphView.svelte";
  import { relight } from "./graph";
  import { keyOf } from "./monitor-state.svelte";

  let {
    source,
    branch,
    revision,
    opened = null,
    onopen,
    onclose,
  }: {
    source: Source;
    /** The branch the Run or Current is of: its graphs share their minimized wholes. */
    branch: BranchKey;
    /** The snapshot's or scan's revision: a new one means a new graph to read. */
    revision: number;
    /** The Artifact whose Detail is open. */
    opened?: string | null;
    /** Open the Detail of an Artifact, on the tab of its eval `tab` or its own. */
    onopen: (id: string, tab?: string | null) => void;
    /** Close the open Detail. */
    onclose: () => void;
  } = $props();
  const commands = useIpc();
  const { monitor: m } = getAppState();
  const g = new SourceGraphState();
  setSourceGraphState(g);
  const current = $derived(source.kind === "current");
  const key = $derived(keyOf(source));

  /** Read the graph again for every new snapshot or scan and for another source: the lights of
   * the shape shown, or a new shape to lay out. Only the newest read shows what it read. */
  const reread: Attachment<HTMLElement> = () => {
    void revision;
    const [at, wanted] = [key, source];
    // What it holds and its count of reads are its own: showing a read must not read again.
    const [read, held] = untrack(() => [++g.reads, g.shown && keyOf(g.shown.source) === at ? g.shown : null] as const);
    call(commands.graph(wanted, held?.shape ?? null)).then(
      (next) => {
        if (read !== g.reads) return;
        g.error = null;
        if (next.update.kind === "lights") {
          const graph = held?.shape === next.shape ? relight(held.graph, next.update) : null;
          if (held && graph && graph !== held.graph) g.shown = { ...held, graph };
          return;
        }
        const graph = { nodes: next.update.nodes, edges: next.update.edges };
        lay(next.source, next.shape, graph, () => read === g.reads).catch((failure) => {
          if (read === g.reads) g.error = message(failure);
        });
      },
      (failure) => {
        if (read === g.reads) g.error = message(failure);
      },
    );
  };

  /** Lay `graph` out with the wholes minimized on the branch now and show it, while `still`
   * holds; again if they changed meanwhile. */
  function lay(at: Source, shape: ShapeKey, graph: Shape, still: () => boolean): Promise<void> {
    const minimized = m.minimizedOn(branch);
    return g.positions(at, shape, graph, minimized).then((positions) => {
      if (!still()) return;
      if (m.minimizedOn(branch) !== minimized) return lay(at, shape, graph, still);
      // The same shape may have been lit again meanwhile: its newest lights stay.
      const same = g.shown !== null && keyOf(g.shown.source) === keyOf(at) && g.shown.shape === shape;
      g.shown = { source: at, shape, graph: same ? g.shown!.graph : graph, minimized, positions };
    });
  }

  /** Minimize the whole `id` on the branch's graphs, or expand it; done once the graph shown is
   * laid out again for it. */
  function minimize(id: string, minimized: boolean): Promise<void> {
    m.minimize(branch, id, minimized);
    const held = g.shown;
    if (!held) return Promise.resolve();
    const still = () => g.shown !== null && keyOf(g.shown.source) === keyOf(held.source) && g.shown.shape === held.shape;
    return lay(held.source, held.shape, held.graph, still).catch((failure) => {
      if (still()) g.error = message(failure);
    });
  }
</script>

<div class="host" {@attach reread}>
{#if g.error}
  <p class="note error">{g.error}</p>
{:else if !g.shown}
  <p class="note">Laying out…</p>
{:else if g.shown.graph.nodes.length === 0}
  <p class="note">{current ? "This worktree declares no Artifacts." : "This Run has no Artifacts."}</p>
{:else}
  {#key keyOf(g.shown.source)}
    <SvelteFlowProvider>
      <GraphView
        source={g.shown.source}
        graph={g.shown.graph}
        minimized={g.shown.minimized}
        positions={g.shown.positions}
        {current}
        {opened}
        {onopen}
        {onclose}
        onminimize={minimize}
      />
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
