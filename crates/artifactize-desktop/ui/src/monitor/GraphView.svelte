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
  // A whole is a box around its parts (parts.ts), a parent node in Svelte Flow, and a part of
  // several wholes is drawn in each: every copy opens its Artifact, and the opened Artifact's
  // copies all show it. A whole's Minimize button draws it as a node, which shows the opened
  // Artifact too while it hides it, and its Expand button draws its box again; both keep the
  // whole's top left corner where it was on the screen. `m` does what the button does, for the
  // whole focused on the graph or else the whole whose Detail is open.
  import { GraphViewState, setGraphViewState } from "./graph-view-state.svelte";
  import { tick } from "svelte";
  import { SvelteFlow, Controls, ControlButton, MiniMap, useSvelteFlow, useViewport, type Edge, type Node } from "@xyflow/svelte";
  import type { Attachment } from "svelte/attachments";
  import { PRECEDENCE, type Found, type Shape, type Source } from "../ipc/client";
  import { getAppState } from "../shell/app.svelte";
  import ArtifactNode from "./ArtifactNode.svelte";
  import GraphFind from "./GraphFind.svelte";
  import WholeNode from "./WholeNode.svelte";
  import { boxes, copies, copyEdges, drawings, partsCount } from "./parts";
  import { CURRENT_LEGEND, LEGEND, WORDS, type Point } from "./graph";
  import { area, centred, whole, type Area, type Fit } from "./camera";
  import { Picture, hit, scene, view, within } from "./scene";
  import { plain } from "../shell/keys";

  let {
    source,
    graph,
    minimized,
    positions,
    opened = null,
    current = false,
    onopen,
    onclose,
    onminimize,
  }: {
    /** The Run or Current the graph is of: its find field searches it. */
    source: Source;
    graph: Shape;
    /** A Current: lit by what verify would do, with its structure text below. */
    current?: boolean;
    /** The wholes drawn as nodes, by tree id: those `positions` was laid out for. */
    minimized: readonly string[];
    /** Where the layout put each copy that is no box, by key. */
    positions: Map<string, Point>;
    /** The Artifact whose Detail is open. */
    opened?: string | null;
    /** Open the Detail of the Artifact `id`, on the tab of its eval `tab` or its own. */
    onopen: (id: string, tab?: string | null) => void;
    /** Close the open Detail. */
    onclose: () => void;
    /** Minimize the whole `id`, or expand it; done once the graph is laid out again for it. */
    onminimize: (id: string, minimized: boolean) => Promise<void>;
  } = $props();
  const v = new GraphViewState();
  setGraphViewState(v);
  const app = getAppState();
  const m = app.monitor;
  /** The branch whose Run or Current is shown: its find field lists its project's favorites. */
  const branch = $derived(m.shown.kind === "run" || m.shown.kind === "current" ? m.shown.branch : null);

  const flow = useSvelteFlow();
  const camera = useViewport();
  const nodeTypes = { artifact: ArtifactNode, whole: WholeNode, minimized: WholeNode };
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

  /** Every drawing of an Artifact, each whole before its parts. */
  const drawn = $derived(copies(graph, minimized));
  /** Each copy by key. */
  const byKey = $derived(new Map(drawn.map((copy) => [copy.key, copy])));
  /** Each Artifact's copies by its tree id. */
  const keys = $derived(drawings(drawn));
  /** The copies that show the opened Artifact: its own, and the minimized wholes hiding it. */
  const showing = $derived(new Set(opened === null ? [] : (keys.get(opened) ?? [])));
  /** Where every copy is, wholes around their parts. */
  const rects = $derived(boxes(drawn, positions));
  /** The graph's edges between copies, lit as their edges. */
  const lines = $derived(copyEdges(graph, drawn, current ? PRECEDENCE.current : PRECEDENCE.run).map(({ source, target, edge }) => ({ ...edge, source, target })));
  /** Where every copy is, indexed by area. */
  const laid = $derived(
    scene(
      rects,
      new Set(drawn.filter((copy) => copy.parts.length > 0).map((copy) => copy.key)),
      new Map(drawn.map((copy) => [copy.key, copy.node.id])),
    ),
  );
  /** The whole graph, as Fit View fits it. */
  const bounds = $derived(laid.bounds);
  /** More Artifacts than the window keeps as elements: the canvas draws what they do not. */
  const large = $derived(drawn.length > DRAWN);
  /** In a large graph, the Artifacts in view by number while at most `DRAWN` are, else `null`;
   * none before the canvas has a size. */
  const inView = $derived(large && v.area ? within(laid, view(camera.current, v.area.width, v.area.height), DRAWN) : []);
  /** The copies kept as elements: every one of a small graph (`null`); of a large one, those
   * in view while at most `DRAWN` are. A text key, so that panning over the same ones keeps
   * the same elements. */
  const kept = $derived(large ? (inView?.join() ?? "") : null);
  /** The Artifacts in view are painted as boxes, while too many are in view to keep as elements. */
  const painted = $derived(inView === null);

  // Lights change in place; positions only come with a new layout.
  let nodes = $derived.by<Node[]>(() => {
    // A part kept as an element keeps the wholes around it, which hold it.
    const holding = (key: string): string[] => {
      const parent = byKey.get(key)?.parent ?? null;
      return parent === null ? [key] : [key, ...holding(parent)];
    };
    const keep = kept === null ? null : new Set(kept === "" ? [] : kept.split(",").flatMap((number) => holding(laid.ids[Number(number)])));
    return drawn
      .filter((copy) => rects.has(copy.key) && (keep === null || keep.has(copy.key)))
      .map(({ key, node, parent, parts, minimized: folded, count }) => {
        const [at, around] = [rects.get(key)!, parent === null ? null : rects.get(parent)!];
        return {
          id: key,
          type: folded ? "minimized" : parts.length > 0 ? "whole" : "artifact",
          // A part's place is from its whole's corner.
          position: around ? { x: at.x - around.x, y: at.y - around.y } : { x: at.x, y: at.y },
          parentId: parent ?? undefined,
          // A fixed size: recoloured nodes keep their dimensions and never re-measure. A node
          // object without `measured` makes Svelte Flow drop its handle positions, which hides
          // its edges until it is measured again; every recolour would make them flicker. The
          // first draw goes without it, so the initial fit waits for the laid-out canvas.
          width: at.width,
          height: at.height,
          measured: v.initialized ? { width: at.width, height: at.height } : undefined,
          data: {
            name: node.name,
            light: node.light,
            open: showing.has(key),
            artifact: node.id,
            count,
            onminimize: () => void minimize(key, !folded),
          },
          draggable: false,
          connectable: false,
          ariaLabel: `${node.name}: ${node.note ?? WORDS[node.light]}${folded ? `, minimized, ${partsCount(count)}` : ""}`,
          domAttributes: { "data-light": node.light, "data-artifact": node.id },
        };
      });
  });
  // A large graph's edges are all on the canvas.
  let edges = $derived.by<Edge[]>(() =>
    large
      ? []
      : lines.map((edge) => ({
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

  /** The graph the large graph's picture paints: its edges between copies. */
  const picturing = $derived({ nodes: graph.nodes, edges: lines });
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
    const what = { graph: picturing, viewport: camera.current, width: at.width, height: at.height, ratio: window.devicePixelRatio || 1, boxes: painted, opened: showing };
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
    const key = painted && point ? hit(laid, point) : null;
    const copy = key === null ? undefined : byKey.get(key);
    if (copy) onopen(copy.node.id);
    else onclose();
  }

  /** Open what the find field found, close the field, and bring the Artifact (its first copy)
   * to the middle of the graph left of the Detail once the Detail is there; the keyboard goes
   * to the Detail. */
  async function choose(found: Found) {
    onopen(found.node, found.tab);
    m.finding = false;
    await tick();
    const first = keys.get(found.node)?.[0];
    const [at, canvas] = [first === undefined ? undefined : rects.get(first), v.canvas];
    if (at && canvas) {
      const point = { x: at.x + at.width / 2, y: at.y + at.height / 2 };
      const cover = m.panes.detail?.getBoundingClientRect() ?? null;
      void flow.setViewport(centred(point, canvas.getBoundingClientRect(), cover, flow.getViewport().zoom, READABLE, MAX_ZOOM), { duration: 0 });
    }
    m.panes.detail?.focus();
  }

  /** Minimize the whole drawn as `key`, or expand it, keeping its top left corner where it is
   * on the screen once the graph is laid out again. */
  async function minimize(key: string, folded: boolean) {
    const [copy, before] = [byKey.get(key), rects.get(key)];
    if (!copy || !before) return;
    const { x, y, zoom } = flow.getViewport();
    const corner = { x: before.x * zoom + x, y: before.y * zoom + y };
    await onminimize(copy.node.id, folded);
    await tick();
    const after = rects.get(key);
    const now = flow.getViewport();
    if (after) void flow.setViewport({ x: corner.x - after.x * now.zoom, y: corner.y - after.y * now.zoom, zoom: now.zoom }, { duration: 0 });
  }

  /** A whole's copy, drawn as a box or minimized: one `m` minimizes or expands. */
  const isWhole = (key: string | undefined) => {
    const copy = key === undefined ? undefined : byKey.get(key);
    return copy !== undefined && (copy.parts.length > 0 || copy.minimized);
  };

  /** `m` minimizes or expands a whole as its button does: the whole focused on the graph, or
   * whose button is focused, else the whole whose Detail is open (its first copy), while focus
   * is outside the Pane and no dialog is open. Focus stays on the whole or its button. */
  async function minimizeKey(event: KeyboardEvent) {
    if (event.defaultPrevented || !plain(event, "m") || m.focus === "pane" || m.settings || app.palette || app.help || app.asking) return;
    const target = event.target instanceof Element ? event.target : null;
    if (target?.closest('[role="dialog"], [role="alertdialog"]')) return;
    const held = target?.closest<HTMLElement>(".svelte-flow__node");
    const focused = held && v.canvas?.contains(held) ? held.dataset.id : undefined;
    const open = opened === null ? undefined : keys.get(opened)?.find((key) => byKey.get(key)?.node.id === opened);
    const key = isWhole(focused) ? focused : isWhole(open) ? open : undefined;
    if (key === undefined) return;
    event.preventDefault();
    const onButton = key === focused && target?.closest("button") !== null;
    await minimize(key, !byKey.get(key)!.minimized);
    if (key !== focused) return;
    const again = v.canvas?.querySelector<HTMLElement>(`.svelte-flow__node[data-id="${CSS.escape(key)}"]`);
    (onButton ? again?.querySelector<HTMLElement>("button") : again)?.focus({ preventScroll: true });
  }

  /** Enter on a focused Artifact node opens its Detail, as a click does; on a button in it,
   * the button acts. */
  function keydown(event: KeyboardEvent) {
    const target = event.target as HTMLElement | null;
    if (!target?.classList.contains("svelte-flow__node")) return;
    const key = target.dataset.id;
    const copy = key === undefined ? undefined : byKey.get(key);
    if (!copy || !plain(event, "Enter")) return;
    event.preventDefault();
    onopen(copy.node.id);
  }
</script>

<svelte:window onkeydown={(event) => void minimizeKey(event)} />

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
      onlyRenderVisibleElements={drawn.length > 80}
      oninit={() => {
        v.initialized = true;
        // The first draw shows whole; a canvas without a size yet (a hidden screen) gets its
        // first view once it has one.
        first();
      }}
      proOptions={{ hideAttribution: true }}
      onnodeclick={({ node }) => onopen(byKey.get(node.id)?.node.id ?? node.id)}
      onpaneclick={({ event }) => background(event)}
    >
      <!-- The zoom buttons are the graph's own: the built-in ones carry a native tooltip, and
           hovering opens nothing here. -->
      <Controls showLock={false} showFitView={false} showZoom={false}>
        {#snippet before()}
          <ControlButton class="svelte-flow__controls-zoomin" onclick={() => void flow.zoomIn()} aria-label="Zoom In">
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" aria-hidden="true"><path d="M32 18.133H18.133V32h-4.266V18.133H0v-4.266h13.867V0h4.266v13.867H32z" /></svg>
          </ControlButton>
          <ControlButton class="svelte-flow__controls-zoomout" onclick={() => void flow.zoomOut()} aria-label="Zoom Out">
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 5" aria-hidden="true"><path d="M0 0h32v4.2H0z" /></svg>
          </ControlButton>
        {/snippet}
        {#snippet after()}
          <ControlButton class="svelte-flow__controls-fitview" onclick={fitAll} aria-label="Fit View">
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 30" aria-hidden="true">
              <path
                d="M3.692 4.63c0-.53.4-.938.939-.938h5.215V0H4.708C2.13 0 0 2.054 0 4.63v5.216h3.692V4.631zM27.354 0h-5.2v3.692h5.17c.53 0 .984.4.984.939v5.215H32V4.631A4.624 4.624 0 0027.354 0zm.954 24.83c0 .532-.4.94-.939.94h-5.215v3.768h5.215c2.577 0 4.631-2.13 4.631-4.707v-5.139h-3.692v5.139zm-23.677.94c-.531 0-.939-.4-.939-.94v-5.138H0v5.139c0 2.577 2.13 4.707 4.708 4.707h5.138V25.77H4.631z"
              />
            </svg>
          </ControlButton>
        {/snippet}
      </Controls>
      {#if drawn.length > 40 && !large}<MiniMap pannable zoomable ariaLabel="" aria-label="Mini map" />{/if}
    </SvelteFlow>
    {#if m.finding}<GraphFind {source} {branch} onchoose={(found) => void choose(found)} onclose={() => (m.finding = false)} />{/if}
    <p class="hint"><kbd>Ctrl+F</kbd> finds, or lists the favorites · <kbd>m</kbd> minimizes or expands a whole</p>
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
    border-top: 1px solid var(--line-group);
    background: var(--surface-app);
  }
  /* Under the graph's elements, filling the same box. */
  .paint {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }
  /* Svelte Flow's container hides its overflow, which still lets a focused node scroll it:
   * WebKitGTK then shifts the whole graph off the camera. A clipped container cannot scroll. */
  .canvas :global(.svelte-flow) {
    overflow: clip;
    --xy-background-color: transparent;
    --xy-edge-stroke-width: 1.25;
    --xy-controls-button-background-color: var(--surface-panel);
    --xy-controls-button-background-color-hover: var(--surface-group);
    --xy-controls-button-color: var(--ink-2);
    --xy-controls-button-color-hover: var(--ink);
    --xy-controls-button-border-color: var(--line-group);
    --xy-controls-box-shadow: none;
    --xy-minimap-background-color: var(--surface-panel);
    --xy-minimap-mask-background-color: var(--scrim);
    --xy-minimap-node-background-color: var(--surface-group);
  }
  /* The minimap's nodes take a control's corner. */
  .canvas :global(.svelte-flow__minimap-node) {
    rx: var(--radius-control);
    ry: var(--radius-control);
  }
  /* The zoom controls: one bordered group of 24px squares at the canvas's bottom left. */
  .canvas :global(.svelte-flow__controls) {
    border: 1px solid var(--line-group);
    border-radius: var(--radius-control);
    overflow: hidden;
  }
  .canvas :global(.svelte-flow__controls-button) {
    width: 24px;
    height: 24px;
    border-bottom-color: var(--line-group);
  }
  .canvas :global(.svelte-flow__edge path) {
    stroke: var(--light, var(--ink-3));
  }
  .canvas :global(.svelte-flow__node) {
    cursor: pointer;
  }
  .canvas :global(.svelte-flow__node:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
    border-radius: var(--radius-control);
  }
  .legend {
    position: absolute;
    left: 8px;
    top: 8px;
    display: flex;
    flex-wrap: wrap;
    gap: 4px 10px;
    margin: 0;
    padding: 3px 8px;
    list-style: none;
    font-size: var(--text-label);
    color: var(--ink-2);
    background: color-mix(in srgb, var(--surface-panel) 85%, transparent);
    border-radius: var(--radius-control);
    pointer-events: none;
    z-index: 5;
  }
  /* At the graph's bottom, right of the zoom controls and clear of the Detail on the right. */
  .hint {
    position: absolute;
    left: 52px;
    bottom: 15px;
    margin: 0;
    padding: 3px 8px;
    font-size: var(--text-label);
    color: var(--ink-2);
    background: color-mix(in srgb, var(--surface-panel) 85%, transparent);
    border-radius: var(--radius-control);
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
