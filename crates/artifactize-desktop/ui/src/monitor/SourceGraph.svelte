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
  import GraphTools from "./GraphTools.svelte";
  import { relight } from "./graph";
  import { keyOf } from "./monitor-state.svelte";
import { NO_ANNOTATIONS } from "./annotations";

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
  const { monitor: m, definitions } = getAppState();
  /** The places and comments of the branch's definitions as they are now; none until read. */
  const annotations = $derived(definitions.of(branch) ? definitions.annotations : NO_ANNOTATIONS);
  const g = new SourceGraphState();
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
        // The first layout waits for the branch's places, so that it shows where they are.
        // Places that cannot be read leave the graph laid out automatically.
        Promise.resolve()
          .then(() => definitions.ready(branch))
          .catch(() => {})
          .then(() => lay(next.source, next.shape, graph, () => read === g.reads, () => graph))
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

  /** Lay `graph` out with the wholes minimized on the branch now and the branch's places and
   * comments, and show it lit as `lit` says once laid out, while `still` holds; again if they
   * changed meanwhile. */
  function lay(at: Source, shape: ShapeKey, graph: Shape, still: () => boolean, lit: () => Shape): Promise<void> {
    const [minimized, placed] = untrack(() => [m.minimizedOn(branch), annotations] as const);
    return g.positions(at, shape, graph, minimized, placed).then((positions) => {
      if (!still()) return;
      if (untrack(() => m.minimizedOn(branch) !== minimized || annotations !== placed)) return lay(at, shape, graph, still, lit);
      g.shown = { source: at, shape, graph: lit(), minimized, positions, annotations: placed };
    });
  }

  /** The graph shown is laid out again whenever the branch's places or comments change: an edit
   * wrote them, or they were read again. */
  const replace: Attachment<HTMLElement> = () => {
    const placed = annotations;
    const held = untrack(() => g.shown);
    if (!held || held.annotations === placed) return;
    const still = () => g.shown !== null && keyOf(g.shown.source) === keyOf(held.source) && g.shown.shape === held.shape;
    lay(held.source, held.shape, held.graph, still, () => g.shown!.graph).catch((failure) => {
      if (still()) g.error = message(failure);
    });
  };

  /** Minimize the whole `id` on the branch's graphs, or expand it; done once the graph shown is
   * laid out again for it. */
  function minimize(id: string, minimized: boolean): Promise<void> {
    m.minimize(branch, id, minimized);
    const held = g.shown;
    if (!held) return Promise.resolve();
    const still = () => g.shown !== null && keyOf(g.shown.source) === keyOf(held.source) && g.shown.shape === held.shape;
    // The graph may have been lit again meanwhile: its newest lights stay.
    return lay(held.source, held.shape, held.graph, still, () => g.shown!.graph).catch((failure) => {
      if (still()) g.error = message(failure);
    });
  }

  /** The graph's read failure, told to the Main area's header notice while it lasts (D-MAIN-3). */
  const report: Attachment = () => {
    const error = g.error;
    m.graphError = error;
    return () => {
      if (m.graphError === error) m.graphError = null;
    };
  };
</script>

<!-- The revision the graph was read at, and the one drawn: equal once the newest read shows. -->
<div class="host" data-read={revision} data-drawn={g.drawn} {@attach reread} {@attach replace} {@attach report}>
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
        annotations={g.shown.annotations}
        {branch}
        {current}
        empty={current ? "This worktree defines no Artifacts." : "This Run has no Artifacts."}
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
