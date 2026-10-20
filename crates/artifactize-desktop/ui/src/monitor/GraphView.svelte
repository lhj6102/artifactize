<script lang="ts">
  // The laid-out graph. It takes the pointer: a click on an Artifact asks for its Detail, a
  // click on the background closes it. The keyboard reaches the nodes with Tab, and Enter on a
  // focused node does what a click does. A panel docked over its right side (the Detail) hides
  // no Artifact: the camera works in the part of the canvas the panel leaves.
  import { GraphViewState, setGraphViewState } from "./graph-view-state.svelte";
  import { SvelteFlow, Controls, MiniMap, useSvelteFlow, type Edge, type Node } from "@xyflow/svelte";
  import { untrack } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import type { RunGraph } from "../ipc/client";
  import ArtifactNode from "./ArtifactNode.svelte";
  import { LEGEND, WORDS, type Point } from "./graph";
  import { NODE } from "./layout";
  import { area, padding, reveal, same, whole, type Area, type Fit } from "./camera";
  import { plain } from "../shell/keys";

  let {
    graph,
    positions,
    opened = null,
    cover = null,
    structural = false,
    onopen,
    onclose,
  }: {
    graph: Pick<RunGraph, "nodes" | "edges">;
    /** Static structure has no Run verdicts. */
    structural?: boolean;
    positions: Map<string, Point>;
    /** The Artifact whose Detail is open. */
    opened?: string | null;
    /** The panel docked over the graph's right side while it is open, such as the Detail. */
    cover?: HTMLElement | null;
    /** Open the Detail of the Artifact `id`. */
    onopen: (id: string) => void;
    /** Close the open Detail. */
    onclose: () => void;
  } = $props();
  const v = new GraphViewState();
  setGraphViewState(v);

  const flow = useSvelteFlow();
  const nodeTypes = { artifact: ArtifactNode };
  /** A positive zoom floor clips sufficiently long graphs in the split canvas. Zero lets
   * Fit View choose the positive scale needed to fit any layout, also on a narrow window. */
  const MIN_ZOOM = 0;
  const MAX_ZOOM = 2;
  /** How the whole graph fits: the structure view keeps a wider margin and a smaller zoom. */
  const fit = $derived<Fit>(
    structural ? { minZoom: MIN_ZOOM, maxZoom: 1.6, padding: 0.6 } : { minZoom: MIN_ZOOM, maxZoom: MAX_ZOOM, padding: 0.1 },
  );
  /** Fit View, the first fit included, fits into what the docked panel leaves. */
  const fitViewOptions = $derived({ minZoom: fit.minZoom, maxZoom: fit.maxZoom, padding: padding(v.area, fit) });
  // Lights change in place; positions only come with a new layout.
  let nodes = $derived.by<Node[]>(() =>
    graph.nodes
      .filter((node) => positions.has(node.id))
      .map((node) => ({
        id: node.id,
        type: "artifact",
        position: positions.get(node.id)!,
        // A fixed size: recoloured nodes keep their dimensions and never re-measure. A node
        // object without `measured` makes Svelte Flow drop its handle positions, which hides
        // its edges until it is measured again; every recolour would make them flicker. The
        // first draw goes without it, so the initial fit waits for the laid-out canvas.
        width: NODE.width,
        height: NODE.height,
        measured: v.initialized ? { width: NODE.width, height: NODE.height } : undefined,
        data: { name: node.name, light: node.light, open: node.id === opened },
        draggable: false,
        connectable: false,
        ariaLabel: structural ? `${node.name}: current structure` : `${node.name}: ${WORDS[node.light]}`,
        domAttributes: { "data-light": node.light },
      })),
  );
  let edges = $derived.by<Edge[]>(() =>
    graph.edges.map((edge) => ({
      id: `${edge.source}>${edge.target}`,
      source: edge.source,
      target: edge.target,
      class: `light-${edge.light}`,
      selectable: false,
      focusable: false,
      domAttributes: { "data-light": edge.light },
    })),
  );

  /** The canvas and the panel over it, measured whenever either changes size; the panel's
   * opening and closing gives a new one. */
  const measure = (panel: HTMLElement | null): Attachment<HTMLElement> => (canvas) => {
    const resized = new ResizeObserver(() => {
      const next = area(canvas.clientWidth, canvas.clientHeight, canvas.getBoundingClientRect(), panel?.getBoundingClientRect() ?? null);
      // A hidden canvas has no size; it keeps the camera for when it shows again.
      if (next) follow(next);
    });
    resized.observe(canvas);
    if (panel) resized.observe(panel);
    return () => resized.disconnect();
  };

  /** What the panel leaves changed. A camera showing the whole graph fits it into the new
   * part, as does one never fitted into a canvas with a size (a graph first drawn in a hidden
   * screen); the structure view always refits, so expanding its text never clips the graph.
   * A camera the person moved stays, apart from bringing the opened Artifact into view. */
  function follow(next: Area) {
    const before = v.area;
    if (before && before.width === next.width && before.height === next.height && before.covered === next.covered) return;
    v.area = next;
    if (!v.initialized) return;
    const bounds = flow.getNodesBounds(flow.getNodes());
    if (!before || structural || same(flow.getViewport(), whole(bounds, before, fit))) {
      void flow.setViewport(whole(bounds, next, fit), { duration: 0 });
    }
    show(opened);
  }

  /** Bring the Artifact `id` into the part the panel leaves, at the camera's zoom. */
  function show(id: string | null) {
    const at = id === null ? undefined : positions.get(id);
    if (!at || !v.area || !v.initialized) return;
    const moved = reveal(flow.getViewport(), { ...at, width: NODE.width, height: NODE.height }, v.area);
    if (moved) void flow.setViewport(moved, { duration: 0 });
  }

  /** Another opened Artifact, such as a neighbour clicked in the Detail, comes into view. */
  const keepOpened: Attachment = () => {
    const id = opened;
    untrack(() => show(id));
  };

  /** Enter on a focused Artifact node opens its Detail, as a click does. */
  function keydown(event: KeyboardEvent) {
    const id = (event.target as HTMLElement | null)?.closest<HTMLElement>(".svelte-flow__node")?.dataset.id;
    if (!id || !plain(event, "Enter")) return;
    event.preventDefault();
    onopen(id);
  }
</script>

<div class="graph">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="canvas" {@attach measure(cover)} {@attach keepOpened} onkeydown={keydown} tabindex="-1">
    <SvelteFlow
      bind:nodes
      bind:edges
      {nodeTypes}
      fitView
      {fitViewOptions}
      minZoom={MIN_ZOOM}
      maxZoom={MAX_ZOOM}
      nodesDraggable={false}
      nodesConnectable={false}
      elementsSelectable={false}
      autoPanOnNodeFocus={false}
      elevateNodesOnSelect={false}
      onlyRenderVisibleElements={graph.nodes.length > 80}
      oninit={() => {
        v.initialized = true;
        // The first draw shows whole beside an open panel; a canvas without a size yet (a
        // hidden screen) gets its fit once it has one.
        if (!v.area) return;
        void flow.setViewport(whole(flow.getNodesBounds(flow.getNodes()), v.area, fit), { duration: 0 });
        show(opened);
      }}
      proOptions={{ hideAttribution: true }}
      onnodeclick={({ node }) => onopen(node.id)}
      onpaneclick={onclose}
    >
      <Controls showLock={false} {fitViewOptions} />
      {#if graph.nodes.length > 40}<MiniMap pannable zoomable />{/if}
    </SvelteFlow>
    <ul class="legend" aria-label="Legend">
      {#if structural}
        <li>Current structure · arrows: input → consumer</li>
      {:else}
        {#each LEGEND as light (light)}
          <li class="light-{light}"><span class="swatch"></span>{WORDS[light]}</li>
        {/each}
      {/if}
    </ul>
  </div>
</div>

<style>
  .graph {
    position: relative;
    height: 100%;
    display: grid;
    grid-template-rows: minmax(0, 1fr);
    grid-template-columns: minmax(0, 1fr);
    min-height: 0;
  }
  .canvas {
    position: relative;
    min-height: 0;
    border-top: 1px solid var(--border);
  }
  .canvas :global(.svelte-flow) {
    --xy-background-color: var(--bg);
    --xy-edge-stroke-width: 1.5;
  }
  .canvas :global(.svelte-flow__edge path) {
    stroke: var(--light, var(--faint));
  }
  .canvas :global(.svelte-flow__node) {
    cursor: pointer;
  }
  .canvas :global(.svelte-flow__node:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
    border-radius: var(--radius);
  }
  .legend {
    position: absolute;
    left: 8px;
    top: 8px;
    display: flex;
    flex-wrap: wrap;
    gap: 4px 10px;
    margin: 0;
    padding: 4px 8px;
    list-style: none;
    font-size: 11px;
    color: var(--muted);
    background: color-mix(in srgb, var(--surface) 85%, transparent);
    border-radius: 6px;
    pointer-events: none;
    z-index: 5;
  }
  .swatch {
    display: inline-block;
    width: 8px;
    height: 8px;
    margin-right: 4px;
    border-radius: 50%;
    background: var(--light);
  }
</style>
