<script lang="ts">
  // The laid-out graph. It takes the pointer: a click on an Artifact asks for its Detail, a
  // click on the background closes it. The keyboard reaches the nodes with Tab, and Enter on a
  // focused node does what a click does. Its first view shows the whole graph in its canvas; the
  // Detail floats over the canvas and takes no part in it. From then on only the person moves the
  // camera, by panning, zooming, Fit View or opening what its find field (Ctrl+F) found: that
  // brings the Artifact to the middle of the graph left of the Detail.
  //
  // A graph of more Artifacts than the window keeps as elements (a Run of tens of thousands)
  // draws its edges on a canvas below them, and keeps as elements only the Artifacts in view
  // while at most that many are; with more in view, they are boxes on the canvas, which a click
  // opens as it opens an element.
  //
  // Each space's Artifacts sit in its region, drawn behind them, which a click passes to the
  // background.
  import { GraphViewState, setGraphViewState } from "./graph-view-state.svelte";
  import { tick } from "svelte";
  import { SvelteFlow, Controls, ControlButton, MiniMap, useSvelteFlow, useViewport, type Edge, type Node } from "@xyflow/svelte";
  import type { Attachment } from "svelte/attachments";
  import type { Found, Shape, Source } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import ArtifactNode from "./ArtifactNode.svelte";
  import GraphFind from "./GraphFind.svelte";
  import SpaceNode from "./SpaceNode.svelte";
  import { CURRENT_LEGEND, LEGEND, WORDS, type Point } from "./graph";
  import { NODE } from "./layout";
  import { regions } from "./spaces";
  import { area, centred, whole, type Area, type Fit } from "./camera";
  import { Picture, hit, scene, view, within } from "./scene";
  import { plain } from "../shell/keys";

  let {
    source,
    graph,
    positions,
    opened = null,
    current = false,
    onopen,
    onclose,
  }: {
    /** The Run or Current the graph is of: its find field searches it. */
    source: Source;
    graph: Shape;
    /** A Current: lit by what verify would do, with its structure text below. */
    current?: boolean;
    positions: Map<string, Point>;
    /** The Artifact whose Detail is open. */
    opened?: string | null;
    /** Open the Detail of the Artifact `id`, on the tab of its eval `tab` or its own. */
    onopen: (id: string, tab?: string | null) => void;
    /** Close the open Detail. */
    onclose: () => void;
  } = $props();
  const v = new GraphViewState();
  setGraphViewState(v);
  const { monitor: m } = getAppState();

  const flow = useSvelteFlow();
  const camera = useViewport();
  const nodeTypes = { artifact: ArtifactNode, space: SpaceNode };
  /** A positive zoom floor clips sufficiently long graphs in the split canvas. Zero lets
   * Fit View choose the positive scale needed to fit any layout, also on a narrow window. */
  const MIN_ZOOM = 0;
  const MAX_ZOOM = 2;
  /** The zoom from which an Artifact's name reads: bringing a found one into view zooms in to
   * it, never out. */
  const READABLE = 0.75;
  /** The most Artifacts kept as elements: the window stays quick to pan and zoom with this
   * many, and a camera holding about this many shows their names readably. */
  const DRAWN = 500;
  /** How the whole graph fits: its first view and Fit View. */
  const FIT: Fit = { minZoom: MIN_ZOOM, maxZoom: MAX_ZOOM, padding: 0.1 };
  /** Fit View, the first fit included, fits the whole canvas. */
  const fitViewOptions = { minZoom: FIT.minZoom, maxZoom: FIT.maxZoom, padding: FIT.padding };

  /** Where every Artifact is, indexed by area. */
  const laid = $derived(scene(positions));
  /** Each space's region around its Artifacts; few, so always kept as elements. */
  const spaces = $derived(regions(graph, positions));
  /** The whole graph, the spaces' regions included, as Fit View fits it. */
  const bounds = $derived.by(() => {
    const all = [laid.bounds, ...spaces];
    if (spaces.length === 0) return laid.bounds;
    const [left, top] = [Math.min(...all.map((box) => box.x)), Math.min(...all.map((box) => box.y))];
    const right = Math.max(...all.map((box) => box.x + box.width));
    const bottom = Math.max(...all.map((box) => box.y + box.height));
    return { x: left, y: top, width: right - left, height: bottom - top };
  });
  /** More Artifacts than the window keeps as elements: the canvas draws what they do not. */
  const large = $derived(graph.nodes.length > DRAWN);
  /** In a large graph, the Artifacts in view by number while at most `DRAWN` are, else `null`;
   * none before the canvas has a size. */
  const inView = $derived(large && v.area ? within(laid, view(camera.current, v.area.width, v.area.height), DRAWN) : []);
  /** The Artifacts kept as elements: every one of a small graph (`null`); of a large one,
   * those in view while at most `DRAWN` are. A text key, so that panning over the same ones
   * keeps the same elements. */
  const drawn = $derived(large ? (inView?.join() ?? "") : null);
  /** The Artifacts in view are painted as boxes, while too many are in view to keep as elements. */
  const painted = $derived(inView === null);

  // Lights change in place; positions only come with a new layout.
  let nodes = $derived.by<Node[]>(() => {
    const keep = drawn === null ? null : new Set(drawn === "" ? [] : drawn.split(",").map((number) => laid.ids[Number(number)]));
    const behind = spaces.map((region): Node => ({
      id: `space:${region.space}`,
      type: "space",
      position: { x: region.x, y: region.y },
      width: region.width,
      height: region.height,
      measured: v.initialized ? { width: region.width, height: region.height } : undefined,
      data: { space: region.space, outline: region.outline, label: region.label },
      draggable: false,
      connectable: false,
      selectable: false,
      focusable: false,
      zIndex: -1,
      ariaLabel: `Space ${region.space}`,
      domAttributes: { "data-space": region.space },
    }));
    const artifacts = graph.nodes
      .filter((node) => positions.has(node.id) && (keep === null || keep.has(node.id)))
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
        data: { label: node.label, light: node.light, open: node.id === opened },
        draggable: false,
        connectable: false,
        ariaLabel: `${node.name}: ${node.note ?? WORDS[node.light]}`,
        domAttributes: { "data-light": node.light },
      }));
    return [...behind, ...artifacts];
  });
  // A large graph's edges are all on the canvas.
  let edges = $derived.by<Edge[]>(() =>
    large
      ? []
      : graph.edges.map((edge) => ({
          id: `${edge.source}>${edge.target}`,
          source: edge.source,
          target: edge.target,
          class: `light-${edge.light}`,
          selectable: false,
          focusable: false,
          domAttributes: { "data-light": edge.light },
        })),
  );

  /** The canvas, measured whenever it changes size. */
  const measure: Attachment<HTMLElement> = (canvas) => {
    v.canvas = canvas;
    const resized = new ResizeObserver(() => {
      const next = area(canvas.clientWidth, canvas.clientHeight);
      // A hidden canvas has no size; it keeps the camera for when it shows again.
      if (next) follow(next);
    });
    resized.observe(canvas);
    return () => {
      resized.disconnect();
      if (v.canvas === canvas) v.canvas = null;
    };
  };

  /** The canvas's size changed: Fit View fits the new size from now on, and a graph first drawn
   * in a hidden screen gets its first view once its canvas has a size. Nothing else moves the
   * camera. */
  function follow(next: Area) {
    v.area = next;
    first();
  }

  /** The first view, once Svelte Flow drew the graph into a canvas with a size: the whole graph
   * in its canvas. */
  function first() {
    if (v.fitted || !v.initialized || !v.area) return;
    v.fitted = true;
    fitAll();
  }

  /** Fit View: every Artifact, kept as an element or not, in the canvas as it is now. It
   * measures the canvas itself rather than trust the last size change reported, which may
   * not have arrived yet. */
  function fitAll() {
    const now = v.canvas ? area(v.canvas.clientWidth, v.canvas.clientHeight) : null;
    if (now) v.area = now;
    if (v.area) void flow.setViewport(whole(bounds, v.area, FIT), { duration: 0 });
  }

  /** The large graph's picture, painted around the view and moved as the camera pans. */
  const picture = new Picture();
  /** How long the zoom holds still before a scaled picture is painted sharp at the new zoom:
   * a zoom gesture or animation changes it every frame. */
  const SETTLE = 150;

  /** Show a large graph's canvas in the next frame whenever the camera, the canvas's size, the
   * lights or the painted boxes change; once a zoom settles, paint it sharp. */
  const draw: Attachment<HTMLCanvasElement> = (canvas) => {
    const at = v.area;
    if (!at) return;
    const what = { graph, viewport: camera.current, width: at.width, height: at.height, ratio: window.devicePixelRatio || 1, boxes: painted, opened };
    const where = laid;
    let settle: ReturnType<typeof setTimeout> | undefined;
    const show = (sharp: boolean) => {
      const [width, height] = [Math.round(what.width * what.ratio), Math.round(what.height * what.ratio)];
      if (canvas.width !== width || canvas.height !== height) [canvas.width, canvas.height] = [width, height];
      const context = canvas.getContext("2d");
      if (context && !picture.show(context, where, what, sharp)) settle = setTimeout(() => (frame = requestAnimationFrame(() => show(true))), SETTLE);
    };
    let frame = requestAnimationFrame(() => show(false));
    return () => {
      cancelAnimationFrame(frame);
      clearTimeout(settle);
    };
  };

  /** A click on the background: an Artifact painted there opens, as a click on an element
   * does; anywhere else closes the Detail. */
  function background(event: MouseEvent | TouchEvent) {
    const box = v.canvas?.getBoundingClientRect();
    // A touch has no pointer position to find a painted Artifact at.
    if (!(event instanceof MouseEvent)) {
      onclose();
      return;
    }
    const camera = flow.getViewport();
    const point = box && { x: (event.clientX - box.left - camera.x) / camera.zoom, y: (event.clientY - box.top - camera.y) / camera.zoom };
    const id = painted && point ? hit(laid, point) : null;
    if (id) onopen(id);
    else onclose();
  }

  /** Open what the find field found, close the field, and bring the Artifact to the middle of
   * the graph left of the Detail once the Detail is there; the keyboard goes to the Detail. */
  async function choose(found: Found) {
    onopen(found.node, found.tab);
    m.finding = false;
    await tick();
    const [at, canvas] = [positions.get(found.node), v.canvas];
    if (at && canvas) {
      const point = { x: at.x + NODE.width / 2, y: at.y + NODE.height / 2 };
      const cover = m.panes.detail?.getBoundingClientRect() ?? null;
      void flow.setViewport(centred(point, canvas.getBoundingClientRect(), cover, flow.getViewport().zoom, READABLE, MAX_ZOOM), { duration: 0 });
    }
    m.panes.detail?.focus();
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
  <div class="canvas" {@attach measure} onkeydown={keydown} tabindex="-1">
    {#if large}<canvas class="paint" aria-hidden="true" {@attach draw}></canvas>{/if}
    <SvelteFlow
      bind:nodes
      bind:edges
      {nodeTypes}
      fitView={!large}
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
        // The first draw shows whole; a canvas without a size yet (a hidden screen) gets its
        // first view once it has one.
        first();
      }}
      proOptions={{ hideAttribution: true }}
      onnodeclick={({ node, event }) => (node.type === "space" ? background(event) : onopen(node.id))}
      onpaneclick={({ event }) => background(event)}
    >
      <Controls showLock={false} showFitView={false}>
        {#snippet after()}
          <ControlButton class="svelte-flow__controls-fitview" onclick={fitAll} title="Fit View" aria-label="Fit View">
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 30" aria-hidden="true">
              <path
                d="M3.692 4.63c0-.53.4-.938.939-.938h5.215V0H4.708C2.13 0 0 2.054 0 4.63v5.216h3.692V4.631zM27.354 0h-5.2v3.692h5.17c.53 0 .984.4.984.939v5.215H32V4.631A4.624 4.624 0 0027.354 0zm.954 24.83c0 .532-.4.94-.939.94h-5.215v3.768h5.215c2.577 0 4.631-2.13 4.631-4.707v-5.139h-3.692v5.139zm-23.677.94c-.531 0-.939-.4-.939-.94v-5.138H0v5.139c0 2.577 2.13 4.707 4.708 4.707h5.138V25.77H4.631z"
              />
            </svg>
          </ControlButton>
        {/snippet}
      </Controls>
      {#if graph.nodes.length > 40 && !large}<MiniMap pannable zoomable />{/if}
    </SvelteFlow>
    {#if m.finding}<GraphFind {source} onchoose={(found) => void choose(found)} onclose={() => (m.finding = false)} />{/if}
    <ul class="legend" aria-label="Legend">
      {#each current ? CURRENT_LEGEND : LEGEND as light (light)}
        <li class="light-{light}"><span class="swatch"></span>{WORDS[light]}</li>
      {/each}
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
    background: var(--bg);
  }
  /* Under the graph's elements, filling the same box. */
  .paint {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }
  .canvas :global(.svelte-flow) {
    --xy-background-color: transparent;
    --xy-edge-stroke-width: 1.5;
  }
  .canvas :global(.svelte-flow__edge path) {
    stroke: var(--light, var(--faint));
  }
  .canvas :global(.svelte-flow__node) {
    cursor: pointer;
  }
  /* A space's region takes the pointer only within its outline and on its label. */
  .canvas :global(.svelte-flow__node-space) {
    pointer-events: none;
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
