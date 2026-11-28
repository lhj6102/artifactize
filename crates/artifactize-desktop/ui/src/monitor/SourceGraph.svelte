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
  import { setSourceGraphState } from "./source-graph-state.svelte";
  import GraphView from "./GraphView.svelte";
  import GraphTools from "./GraphTools.svelte";
  import { relight } from "./graph";
  import { keyOf } from "./monitor-state.svelte";
  import type { GraphWorkspace } from "./graph-workspace.svelte";

  let {
    source,
    branch,
    revision,
    opened = null,
    workspace,
    active,
    onopen,
    onclose,
    bottom = 0,
  }: {
    source: Source;
    workspace?: GraphWorkspace;
    active?: boolean;
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
    /** The band a floating chat covers at the canvas's bottom, kept free by the camera. */
    bottom?: number;
  } = $props();
  const commands = useIpc();
  const app = getAppState();
  const m = $derived(workspace ?? app.monitor);
  const g = untrack(() => m.graph);
  setSourceGraphState(g);
  const current = $derived(source.kind === "current");
  const key = $derived(keyOf(source));

  /** Read the graph again for every new snapshot or scan and for another source: the lights of
   * the shape shown, or a new shape to lay out. Only the newest read shows what it read. */
  const reread: Attachment<HTMLElement> = () => {
    const [at, wanted, atRevision] = [key, source, revision];
    // What it holds and its count of reads are its own: showing a read must not read again.
    const [read, held] = untrack(() => [++g.reads, g.shown && keyOf(g.shown.source) === at ? g.shown : null] as const);
    call(commands.graph(wanted, held?.shape ?? null)).then(
      (next) => {
        if (read !== g.reads) return;
        g.error = null;
        if (next.update.kind === "lights") {
          const graph = held?.shape === next.shape ? relight(held.graph, next.update) : null;
          if (held && graph && graph !== held.graph) g.shown = { ...held, graph };
          if (held && graph) g.drawn = atRevision;
          return;
        }
        const graph = { nodes: next.update.nodes, edges: next.update.edges };
        lay(next.source, next.shape, graph, () => read === g.reads, () => graph)
          .then(() => {
            if (read === g.reads) g.drawn = atRevision;
          })
          .catch((failure) => {
          if (read === g.reads) g.error = message(failure);
        });
      },
      (failure) => {
        if (read === g.reads) g.error = message(failure);
      },
    );
  };

  /** Lay `graph` out with the wholes minimized on the branch now, and show it lit as `lit`
   * says once laid out, while `still` holds; again if they changed meanwhile. */
  function lay(at: Source, shape: ShapeKey, graph: Shape, still: () => boolean, lit: () => Shape): Promise<void> {
    const minimized = untrack(() => app.monitor.minimizedOn(branch));
    return g.positions(at, shape, graph, minimized).then((positions) => {
      if (!still()) return;
      if (untrack(() => app.monitor.minimizedOn(branch) !== minimized)) return lay(at, shape, graph, still, lit);
      g.shown = { source: at, shape, graph: lit(), minimized, positions };
    });
  }

  /** Minimize the whole `id` on the branch's graphs, or expand it; done once the graph shown is
   * laid out again for it. */
  function minimize(id: string, minimized: boolean): Promise<void> {
    app.monitor.minimize(branch, id, minimized);
    const held = g.shown;
    if (!held) return Promise.resolve();
    const still = () => g.shown !== null && keyOf(g.shown.source) === keyOf(held.source) && g.shown.shape === held.shape;
    // The graph may have been lit again meanwhile: its newest lights stay.
    return lay(held.source, held.shape, held.graph, still, () => g.shown!.graph).catch((failure) => {
      if (still()) g.error = message(failure);
    });
  }

</script>

<!-- The revision the graph was read at, and the one drawn: equal once the newest read shows. -->
<div class="host" data-read={revision} data-drawn={g.drawn} {@attach reread}>
{#if g.error}
  <!-- Why stands in the header's notice; the graph's place says only that it is not drawn. -->
  <div class="pending"><GraphTools /><p class="note">The graph could not be read.</p></div>
{:else if !g.shown}
  <div class="pending"><GraphTools /><p class="note">Laying out…</p></div>
{:else}
  {#key keyOf(g.shown.source)}
    <SvelteFlowProvider>
      <GraphView
        source={g.shown.source}
        graph={g.shown.graph}
        minimized={g.shown.minimized}
        positions={g.shown.positions}
        {branch}
        {current}
        {workspace}
        {active}
        empty={current ? "This worktree defines no Artifacts." : "This Run has no Artifacts."}
        {opened}
        {onopen}
        {onclose}
        onminimize={minimize}
        {bottom}
      />
    </SvelteFlowProvider>
  {/key}
{/if}
</div>

<style>
  .host {
    display: contents;
  }
  /* Nothing to act on yet: the tool strip in its place, the line below it. */
  .pending {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    min-height: 0;
  }
  .note {
    margin: 0;
    padding: 6px var(--pad);
    color: var(--ink-2);
  }
</style>
