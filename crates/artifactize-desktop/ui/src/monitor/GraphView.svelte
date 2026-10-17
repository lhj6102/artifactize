<script lang="ts">
  // The laid-out graph. It takes the pointer: a click on an Artifact asks for its Detail, a
  // click on the background closes it. The keyboard reaches the nodes with Tab, and Enter on a
  // focused node does what a click does.
  import { GraphViewState, setGraphViewState } from "./graph-view-state.svelte";
  import { SvelteFlow, Controls, MiniMap, useSvelteFlow, getViewportForBounds, type Edge, type Node } from "@xyflow/svelte";
  import type { Attachment } from "svelte/attachments";
  import type { RunGraph } from "../ipc/client";
  import ArtifactNode from "./ArtifactNode.svelte";
  import { LEGEND, WORDS, type Point } from "./graph";
  import { NODE } from "./layout";
  import { plain } from "../shell/keys";

  let {
    graph,
    positions,
    opened = null,
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
    /** Open the Detail of the Artifact `id`. */
    onopen: (id: string) => void;
    /** Close the open Detail. */
    onclose: () => void;
  } = $props();
  const v = new GraphViewState();
  setGraphViewState(v);

  const flow = useSvelteFlow();
  const nodeTypes = { artifact: ArtifactNode };
  /** The camera and its controls share the same zoom limits. */
  const MIN_ZOOM = 0.05;
  const MAX_ZOOM = 2;
  // Lights change in place; positions only come with a new layout.
  let nodes = $derived.by<Node[]>(() =>
    graph.nodes
      .filter((node) => positions.has(node.id))
      .map((node) => ({
        id: node.id,
        type: "artifact",
        position: positions.get(node.id)!,
        // A fixed size: recoloured nodes keep their dimensions and never re-measure.
        width: NODE.width,
        height: NODE.height,
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

  /** Expanding the structural text changes the canvas height. Keep the whole graph in view
   * rather than clipping it below the new canvas; the Run view keeps its own camera. */
  const structuralSize: Attachment<HTMLElement> = (canvas) => {
    if (!structural) return;
    let previous = "";
    const resized = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      if (width === 0 || height === 0) return;
      const size = `${width}:${height}`;
      if (previous && previous !== size) {
        void fit({ duration: 0, maxZoom: 1.6, padding: 0.6 });
      }
      previous = size;
    });
    resized.observe(canvas);
    return () => resized.disconnect();
  };

  /** fitView shares a pending resolver across calls. Use the public bounds calculation and
   * a distinct viewport transition, so an old fit cannot finish a newer one early. */
  function fit(options: { duration: number; maxZoom?: number; padding?: number }) {
    if (!v.canvas) return Promise.resolve(false);
    const bounds = flow.getNodesBounds(flow.getNodes());
    const viewport = getViewportForBounds(
      bounds, v.canvas.clientWidth, v.canvas.clientHeight,
      MIN_ZOOM, options.maxZoom ?? MAX_ZOOM, options.padding ?? 0.1,
    );
    return flow.setViewport(viewport, { duration: options.duration });
  }

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
  <div class="canvas" bind:this={v.canvas} {@attach structuralSize} onkeydown={keydown} tabindex="-1">
    <SvelteFlow
      bind:nodes
      bind:edges
      {nodeTypes}
      fitView
      minZoom={MIN_ZOOM}
      maxZoom={MAX_ZOOM}
      nodesDraggable={false}
      nodesConnectable={false}
      elementsSelectable={false}
      autoPanOnNodeFocus={false}
      elevateNodesOnSelect={false}
      onlyRenderVisibleElements={graph.nodes.length > 80}
      proOptions={{ hideAttribution: true }}
      onnodeclick={({ node }) => onopen(node.id)}
      onpaneclick={onclose}
    >
      <Controls showLock={false} />
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
