<script lang="ts">
  // The laid-out graph under its tool strip (Find, the zoom group with the zoom in percent, Fit),
  // on a canvas with a faint grid that moves with the camera. It takes the pointer: a click on an
  // Artifact asks for its Detail, a click on the background closes it. The keyboard reaches the
  // nodes with Tab, and Enter on a focused node does what a click does. Its first view shows the
  // whole graph in its canvas, never enlarged beyond its nodes' own size; the Detail is docked
  // over the canvas and takes no part in it. From then on only the person moves the camera, by
  // panning, zooming, Fit View, the Navigator (shown while part of the graph is out of view) or
  // opening what its find field (Ctrl+F) found: that brings the Artifact to the middle of the
  // graph left of the Detail. Edges are neutral, but for those that leave a RED, ERROR or
  // blocked Artifact, drawn in its colour.
  //
  // A graph of more Artifacts than the window keeps as elements (a Run of tens of thousands)
  // draws its edges on a canvas below them, and keeps as elements only the Artifacts in view
  // while at most that many are and their names read; with more in view, or zoomed out too far
  // for names, they are boxes on the canvas, which a click opens as it opens an element.
  //
  // A whole is a box around its parts (parts.ts), a parent node in Svelte Flow, and a part of
  // several wholes is drawn in each: every copy opens its Artifact, and the opened Artifact's
  // copies all show it. A whole's Minimize button draws it as a node, which shows the opened
  // Artifact too while it hides it, and its Expand button draws its box again; both keep the
  // whole's top left corner where it was on the screen. `m` does what the button does, for the
  // whole focused on the graph or else the whole whose Detail is open.
  //
  // A Current is where the branch's Artifacts are defined. There a node, a minimized whole or a
  // whole's title row is dragged: released in the box it was in it is placed, and the places of
  // that box (or the top level) are saved in the definitions as shown; over another Artifact,
  // highlighted while the library would accept it, it becomes its part; out of every box it
  // leaves its whole. Esc during a drag puts it back. A comment is dragged by its text onto an
  // Artifact without one; a click on it opens its field. A drag on a whole's face pans, as one on
  // the background does. A Run's graph is a record: nothing on it moves.
  import { GraphViewState, setGraphViewState } from "./graph-view-state.svelte";
  import { tick } from "svelte";
  import { SvelteFlow, useSvelteFlow, useViewport, type Edge, type Node } from "@xyflow/svelte";
  import type { Attachment } from "svelte/attachments";
  import { PRECEDENCE, type Found, type Light, type Shape, type Source } from "../ipc/client";
  import type { GraphWorkspace } from "./graph-workspace.svelte";
  import { getAppState } from "../shell/app.svelte";
  import Icon from "../shell/Icon.svelte";
  import GraphTools from "./GraphTools.svelte";
  import CommentField from "./CommentField.svelte";
  import ArtifactNode from "./ArtifactNode.svelte";
  import GraphFind from "./GraphFind.svelte";
  import Navigator from "./Navigator.svelte";
  import WholeNode from "./WholeNode.svelte";
  import { boxes, copies, copyEdges, drawings, partsCount, type Copy } from "./parts";
  import { BLOCKING, NODE, WORDS, ZOOM_STEP, currentWords, tallyWords, type Point } from "./graph";
  import { area, centred, whole, type Area, type Fit } from "./camera";
  import { Picture, commentRect, hit, legible, scene, view, within } from "./scene";
  import { plain } from "../shell/keys";
  import { useCommands } from "../shell/commands.svelte";
  import { NO_ANNOTATIONS, roomOf, type Annotations } from "./annotations";
  import { anchor, boxPlaces, canJoin, dropAt, inside, placesOf, topLevel, under, type Drop } from "./placing";
  import type { BranchKey, EditOperation } from "../ipc/client";

  let {
    source,
    graph,
    minimized,
    positions,
    annotations = NO_ANNOTATIONS,
    branch = null,
    workspace,
    active,
    opened = null,
    current = false,
    empty = "",
    onopen,
    onclose,
    onminimize,
  }: {
    /** The Run or Current the graph is of: its find field searches it. */
    source: Source;
    graph: Shape;
    /** A Current: lit by what verify would do. */
    current?: boolean;
    /** Said under the tool strip when the graph has no Artifacts. */
    empty?: string;
    /** The wholes drawn as nodes, by tree id: those `positions` was laid out for. */
    minimized: readonly string[];
    /** Where the layout put each copy that is no box, by key. */
    positions: Map<string, Point>;
    /** The places and comments `positions` was laid out with. */
    annotations?: Annotations;
    /** The branch the graph is of: a Current's edits write its definitions. */
    branch?: BranchKey | null;
    workspace?: GraphWorkspace;
    active?: boolean;
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
  const m = $derived(workspace ?? app.monitor);
  const shown = $derived(active ?? (app.mode === "verify" && !app.monitor.settings));

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
  /** A graph of more drawings than this keeps as elements only those in view: the window stays
   * quick to pan and zoom. */
  const DRAWN = 500;
  /** How the whole graph fits, its first view and Fit View: never enlarged beyond its nodes'
   * own size, only the person's own zooming goes further. */
  const FIT: Fit = { minZoom: MIN_ZOOM, maxZoom: 1, padding: 0.1 };
  /** The smallest grid cell, in screen pixels, still drawn: further out the grid is left out. */
  const GRID_SHOWN = 8;
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
  /** Where every copy is, wholes around their parts and their comments. */
  const rects = $derived(boxes(drawn, positions, roomOf(annotations)));
  const defs = $derived(branch ? app.definitionsFor(branch) : app.definitions);
  /** A Current of a branch whose definitions are read: what is on it can be edited. */
  const editable = $derived(current && branch !== null && defs.of(branch));
  /** The graph's edges between copies, lit as their edges. */
  const lines = $derived(copyEdges(graph, drawn, current ? PRECEDENCE.current : PRECEDENCE.run).map(({ source, target, edge }) => ({ ...edge, source, target })));
  /** The copies edges reach, and those they leave: each shows its port on that side. */
  const ported = $derived({ inward: new Set(lines.map((edge) => edge.target)), outward: new Set(lines.map((edge) => edge.source)) });
  /** Where every copy is, indexed by area. */
  const laid = $derived(
    scene(
      rects,
      new Set(drawn.filter((copy) => copy.parts.length > 0).map((copy) => copy.key)),
      new Map(drawn.map((copy) => [copy.key, copy.node.id])),
    ),
  );
  /** What the canvas paints from: the layout, with a dragged node and the parts inside it where
   * the drag has taken them. */
  let paintedAt = $derived(laid);
  function dragScene(moved: { key: string; x: number; y: number }) {
    const inside = (copy: Copy | undefined): boolean => !!copy && (copy.key === moved.key || inside(copy.parent === null ? undefined : byKey.get(copy.parent)));
    const shifted = new Map([...rects].map(([key, rect]) => [key, inside(byKey.get(key)) ? { ...rect, x: rect.x + moved.x, y: rect.y + moved.y } : rect]));
    return scene(shifted, new Set(drawn.filter((copy) => copy.parts.length > 0).map((copy) => copy.key)), new Map(drawn.map((copy) => [copy.key, copy.node.id])));
  }
  /** The whole graph, as Fit View fits it. */
  const bounds = $derived(laid.bounds);
  /** More Artifacts than the window keeps as elements: the canvas draws what they do not. */
  const large = $derived(drawn.length > DRAWN);
  /** The Artifacts in view are painted as blocks while the zoom alone makes their names too
   * small to read (D-GRAPH-2), never for how many are in view. A zoom of 0 shows nothing yet: the
   * canvas has no size its first fit could use. */
  const painted = $derived(v.area !== null && camera.current.zoom > 0 && !legible(v.font, camera.current.zoom));
  /** In a large graph whose names read, the Artifacts in view by number; none before the canvas
   * has a size. */
  const inView = $derived(large && v.area && !painted ? (within(laid, view(camera.current, v.area.width, v.area.height)) ?? []) : []);
  /** The canvas paints: every edge of a large graph, and whatever is painted as blocks. */
  const canvased = $derived(large || painted);
  /** The copies kept as elements: none while painted; every one of a small graph (`null`); of a
   * large one, those in view. A text key, so that panning over the same ones keeps the same
   * elements. */
  const kept = $derived(painted ? "" : large ? inView.join() : null);

  // Lights change in place; positions only come with a new layout, or, after a drag that wrote
  // nothing, as they were.
  let nodes = $derived.by<Node[]>(() => {
    void v.epoch;
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
        const box = parts.length > 0 && !folded;
        return {
          id: key,
          type: folded ? "minimized" : box ? "whole" : "artifact",
          // A whole's box is dragged by its title row; its face pans.
          dragHandle: box ? ".title" : undefined,
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
            tally: node.tally,
            open: showing.has(key),
            inward: ported.inward.has(key),
            outward: ported.outward.has(key),
            artifact: node.id,
            count,
            onminimize: () => void minimize(key, !folded),
            comment: annotations.comments.get(node.name) ?? null,
            editing: editable && defs.field?.name === node.name && keys.get(node.id)?.[0] === key,
            reset: editable && box && annotations.places.has(node.name),
            onreset: () => void edit({ kind: "resetPlaces", whole: node.name }),
            oncomment: editable ? (event: PointerEvent) => grabComment(event, key) : undefined,
            onfield: (written: string | null) => void closeField(written),
            onpan: editable ? (event: PointerEvent) => pan(event) : undefined,
          },
          draggable: editable && !painted,
          connectable: false,
          ariaLabel: `${node.name}: ${node.note ?? (current ? currentWords(node.light) : `${WORDS[node.light]}${tallyWords(node.tally)}`)}${folded ? `, minimized, ${partsCount(count)}` : ""}`,
          domAttributes: { "data-light": node.light, "data-artifact": node.id },
        };
      });
  });
  // A large graph's edges, and a painted one's, are all on the canvas.
  let edges = $derived.by<Edge[]>(() =>
    canvased
      ? []
      : lines.map((edge) => ({
          id: `${edge.source}>${edge.target}`,
          source: edge.source,
          target: edge.target,
          class: BLOCKING.has(edge.light) ? `light-${edge.light} blocking` : undefined,
          selectable: false,
          focusable: false,
          domAttributes: { "data-light": edge.light },
        })),
  );

  /** How much of the canvas the docked Detail covers, measured as the Detail opens, closes or
   * changes width: the Navigator shows for, and frames, only what shows. */
  const docked: Attachment<HTMLElement> = () => {
    const pane = opened !== null ? m.panes.detail : undefined;
    if (!pane) {
      v.covered = 0;
      return;
    }
    const sized = new ResizeObserver(() => (v.covered = pane.getBoundingClientRect().width));
    sized.observe(pane);
    return () => sized.disconnect();
  };

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

  /** Each Artifact's light by its tree id, for the Navigator. */
  const lights = $derived(new Map<string, Light>(graph.nodes.map((node) => [node.id, node.light])));
  /** How wide the canvas shows left of a docked Detail. */
  const visible = $derived(v.area ? Math.max(0, v.area.width - v.covered) : 0);
  /** The whole graph is in view, left of a docked Detail: no Navigator. */
  const allInView = $derived.by(() => {
    if (!v.area || graph.nodes.length === 0) return true;
    const { x, y, zoom } = camera.current;
    const [left, top] = [-x / zoom, -y / zoom];
    const [right, bottom] = [left + visible / zoom, top + v.area.height / zoom];
    // Half a pixel of slack: a fit view's rounding never shows the Navigator.
    const slack = 0.5 / zoom;
    return bounds.x >= left - slack && bounds.y >= top - slack && bounds.x + bounds.width <= right + slack && bounds.y + bounds.height <= bottom + slack;
  });
  /** Read the grid's cell and the names' font from the --grid and --text-body tokens once the
   * canvas is there. */
  const grid: Attachment<HTMLElement> = (canvas) => {
    const style = getComputedStyle(canvas);
    v.cell = Number.parseFloat(style.getPropertyValue("--grid")) || v.cell;
    v.font = Number.parseFloat(style.getPropertyValue("--text-body")) || v.font;
  };
  const gridSize = $derived(v.cell * camera.current.zoom);

  /** Zoom in or out by one step, as the tool strip's buttons do. */
  function zoomBy(factor: number) {
    void flow.setZoom(Math.min(Math.max(flow.getZoom() * factor, MIN_ZOOM), MAX_ZOOM));
  }

  /** The Navigator asked for `point` in the middle of the view, the zoom kept. */
  function navigate(point: Point) {
    if (!v.area) return;
    const zoom = flow.getViewport().zoom;
    void flow.setViewport({ x: visible / 2 - point.x * zoom, y: v.area.height / 2 - point.y * zoom, zoom }, { duration: 0 });
  }

  /** The minimized wholes by key, with how many parts each holds. */
  const hidden = $derived(new Map(drawn.filter((copy) => copy.minimized).map((copy) => [copy.key, copy.count])));
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
    const what = {
      graph: picturing,
      viewport: camera.current,
      width: at.width,
      height: at.height,
      ratio: window.devicePixelRatio || 1,
      boxes: painted,
      opened: showing,
      minimized: hidden,
      comments: annotations.comments,
    };
    const where = paintedAt;
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

  /** How far the pointer moves before a press is a drag rather than a click, in screen pixels. */
  const DRAG = 4;

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
    if (event.defaultPrevented || !plain(event, "m") || m.focus === "pane" || !shown || app.palette || app.help || app.asking) return;
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

  /** The whole `m` would minimize or expand now: the focused one, else the one whose Detail is
   * open (its first copy). */
  function wholeAtHand(): string | undefined {
    const focused = v.focused ?? undefined;
    const open = opened === null ? undefined : keys.get(opened)?.find((key) => byKey.get(key)?.node.id === opened);
    return isWhole(focused) ? focused : isWhole(open) ? open : undefined;
  }

  /** The tree id the graph gives the Artifact `name`. */
  const idOf = (name: string) => graph.nodes.find((node) => node.name === name)?.id;

  /** The drawn copy focus is on, kept as focus moves in and out of the canvas. */
  function focusIn(event: FocusEvent) {
    const held = event.target instanceof Element ? event.target.closest<HTMLElement>(".svelte-flow__node") : null;
    v.focused = held?.dataset.id ?? null;
  }


  /** Write `operation` to the branch's definitions, made on the graph: refused, what was moved
   * goes back where it was and the Main area's notice says why. Answers whether it was written. */
  async function edit(operation: EditOperation): Promise<boolean> {
    if (!branch) return false;
    const refused = await defs.edit(branch, operation, "graph");
    // What was dragged goes back where it was drawn.
    if (refused !== null) v.epoch += 1;
    return refused === null;
  }

  /** The point of the graph under the pointer of `event`. */
  function pointOf(event: MouseEvent | TouchEvent | PointerEvent): Point | null {
    const at = "clientX" in event ? event : (event.changedTouches[0] ?? null);
    return at ? flow.screenToFlowPosition({ x: at.clientX, y: at.clientY }) : null;
  }



  function dropOf(key: string, event: MouseEvent | TouchEvent): Drop | null {
    const [copy, at] = [byKey.get(key), pointOf(event)];
    return copy && at ? dropAt(drawn, rects, copy, at) : null;
  }

  function dragStart(key: string) {
    v.dragging = { key, cancelled: false };
    v.target = null;
  }

  /** While a node is dragged, the Artifact a release would make its whole is highlighted, but
   * only while the library would accept it. */
  function dragMove(key: string, event: MouseEvent | TouchEvent) {
    if (!v.dragging || v.dragging.cancelled) return;
    const [from, now] = [v.pressed, pointOf(event)];
    if (canvased && from && now) paintedAt = dragScene({ key, x: now.x - from.x, y: now.y - from.y });
    const [copy, drop] = [byKey.get(key), dropOf(key, event)];
    v.target = branch && defs.of(branch) && copy && drop?.kind === "join"
      && canJoin((name) => defs.node(name)?.parts, copy.node.name, drop.to) ? drop.key : null;
  }

  /** A released node: placed, made a part of the Artifact under it, or out of its whole. It is
   * drawn where it was let go until what was written is laid out, or back where it was when the
   * edit is refused. */
  async function dragStop(node: Node, event: MouseEvent | TouchEvent) {
    try {
      await release(node, event);
    } finally {
      paintedAt = laid;
    }
  }

  async function release(node: Node, event: MouseEvent | TouchEvent) {
    const held = v.dragging;
    v.dragging = null;
    v.target = null;
    const copy = byKey.get(node.id);
    const drop = held && !held.cancelled ? dropOf(node.id, event) : null;
    if (!copy || !drop) {
      v.epoch += 1;
      return;
    }
    const [from, now] = [v.pressed, pointOf(event)];
    const was = rects.get(copy.key)!;
    const around = copy.parent === null ? null : rects.get(copy.parent)!;
    const rect = from && now
      ? { ...was, x: was.x + now.x - from.x, y: was.y + now.y - from.y }
      : { ...was, x: node.position.x + (around?.x ?? 0), y: node.position.y + (around?.y ?? 0) };
    const to = anchor(copy, rect, annotations);
    const whole = copy.parent === null ? null : byKey.get(copy.parent)!.node.name;
    if (drop.kind === "place") {
      await edit({ kind: "savePlaces", places: placesOf(drawn, rects, annotations, copy, to) });
      return;
    }
    if (drop.kind === "leave") {
      // A top level laid out automatically is placed as shown, with the Artifact where it was dropped.
      const shown = annotations.at.size === 0 ? topLevel(drawn, rects, annotations, copy.node.name) : {};
      if (await edit({ kind: "movePart", part: copy.node.name, from: whole, to: null, at: [Math.round(to.x), Math.round(to.y)] })) {
        if (Object.keys(shown).length > 0) await edit({ kind: "savePlaces", places: shown });
      }
      return;
    }
    const target = byKey.get(drop.key)!;
    const box = target.parts.length > 0 && !target.minimized;
    const targetRect = rects.get(target.key)!;
    const release = now ?? to;
    const at: [number, number] = box ? freeIn(target, inside(to, target, targetRect, annotations), was)
      : [Math.max(0, Math.round(release.x - targetRect.x)), Math.max(0, Math.round(release.y - targetRect.y))];
    // A box laid out automatically is placed as shown, with the new part where it was dropped.
    const shown = box && !annotations.places.has(target.node.name) ? boxPlaces(drawn, rects, annotations, target, copy.node.name) : {};
    if (await edit({ kind: "movePart", part: copy.node.name, from: whole, to: target.node.name, at })) {
      if (Object.keys(shown).length > 0) await edit({ kind: "savePlaces", places: shown });
    }
  }

  /** `at` in the box of `whole` for a dragged Artifact of `size`, or, where it would cover one of
   * its parts, under them all. */
  function freeIn(whole: Copy, at: [number, number], size: { width: number; height: number } = NODE): [number, number] {
    const box = rects.get(whole.key)!;
    const parts = whole.parts.flatMap((key) => {
      const rect = rects.get(key);
      const copy = byKey.get(key);
      return rect && copy ? [{ at: inside(anchor(copy, rect, annotations), whole, box, annotations), rect }] : [];
    });
    const covers = parts.some(({ at: [x, y], rect }) => at[0] < x + rect.width && x < at[0] + size.width && at[1] < y + rect.height && y < at[1] + size.height);
    if (!covers) return at;
    return [at[0], Math.max(...parts.map(({ at: [, y], rect }) => y + rect.height)) + 20];
  }

  /** A press on a comment: moved, the comment is dragged onto an Artifact without one; released
   * without moving, its field opens (on a Run, its node's Detail does). Esc puts it back. */
  function grabComment(event: PointerEvent, key: string) {
    const copy = byKey.get(key);
    if (event.button !== 0 || !copy) return;
    event.stopPropagation();
    event.preventDefault();
    const start = { x: event.clientX, y: event.clientY };
    // The text as it is drawn under its node, so it keeps its width and lines while dragged.
    const zoom = flow.getViewport().zoom;
    const grabbed = event.currentTarget instanceof HTMLElement ? event.currentTarget : null;
    const shape = {
      width: (grabbed?.getBoundingClientRect().width ?? NODE.width * zoom) / zoom,
      padding: grabbed ? getComputedStyle(grabbed).padding : "0px",
      zoom,
    };
    let [moved, cancelled] = [false, false];
    let intended: Copy | null = null;
    let checked = 0;
    const move = (next: PointerEvent) => {
      if (cancelled || (!moved && Math.hypot(next.clientX - start.x, next.clientY - start.y) < DRAG)) return;
      moved = true;
      const box = v.canvas?.getBoundingClientRect();
      if (box) v.comment = { key, x: next.clientX - box.left, y: next.clientY - box.top, shape };
      const at = pointOf(next);
      const target = at ? under(drawn, rects, at) : null;
      if (intended?.key === target?.key) return;
      intended = target;
      const request = ++checked;
      v.target = null;
      const owner = branch;
      if (owner && target) void defs.check(owner, { kind: "moveComment", from: copy.node.name, to: target.node.name }).then((result) => {
        if (request === checked && result.kind === "checked") v.target = target.key;
      }, () => {});
    };
    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("keydown", escape, true);
    };
    const up = () => {
      stop();
      ++checked;
      const target = intended;
      [v.comment, v.target] = [null, null];
      if (cancelled) return;
      if (!moved) defs.openField(copy.node.name, "graph");
      else if (target) void edit({ kind: "moveComment", from: copy.node.name, to: target.node.name });
    };
    const escape = (next: KeyboardEvent) => {
      if (!plain(next, "Escape")) return;
      next.preventDefault();
      next.stopPropagation();
      cancelled = true;
      ++checked;
      [v.comment, v.target] = [null, null];
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("keydown", escape, true);
  }

  /** A drag on a whole's face pans the camera, as one on the background does. */
  function pan(event: PointerEvent) {
    if (event.button !== 0) return;
    const start = { x: event.clientX, y: event.clientY };
    const from = flow.getViewport();
    let moved = false;
    const move = (next: PointerEvent) => {
      const [dx, dy] = [next.clientX - start.x, next.clientY - start.y];
      if (!moved && Math.hypot(dx, dy) < DRAG) return;
      moved = true;
      void flow.setViewport({ x: from.x + dx, y: from.y + dy, zoom: from.zoom }, { duration: 0 });
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      // The click that follows a pan opens nothing.
      if (moved) v.panned = true;
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  /** The comment's font measured as the canvas draws it, for the room comments take. */
  const measuring: Attachment<HTMLElement> = (element) => {
    if (defs.measure) return;
    const context = document.createElement("canvas").getContext("2d");
    if (!context) return;
    const style = getComputedStyle(element);
    context.font = `400 ${style.getPropertyValue("--text-small").trim()} ${style.getPropertyValue("--font").trim()}`;
    defs.measure = (text) => context.measureText(text).width;
  };

  /** Esc reaches a drag before anything else that takes it, the Detail's close included. */
  const escaping: Attachment<HTMLElement> = () => {
    window.addEventListener("keydown", cancelDrag, true);
    return () => window.removeEventListener("keydown", cancelDrag, true);
  };

  /** Esc during a node's drag puts it back: the drag ends where it is, writing nothing. */
  function cancelDrag(event: KeyboardEvent) {
    if (!v.dragging || !plain(event, "Escape")) return;
    event.preventDefault();
    event.stopPropagation();
    v.dragging = { ...v.dragging, cancelled: true };
    paintedAt = laid;
    v.target = null;
    v.epoch += 1;
    // Svelte Flow's drag ends on the button's release; this release ends it now.
    window.dispatchEvent(new MouseEvent("mouseup"));
  }

  /** The comment field of `name`'s first copy closes: written, removed or put back. Focus goes
   * back where it was opened from. */
  async function closeField(text: string | null) {
    const field = defs.field;
    if (!field) return;
    if (text !== null && text !== (annotations.comments.get(field.name) ?? "")) {
      if (!(await edit({ kind: "setComment", name: field.name, text }))) return;
    }
    defs.field = null;
    await tick();
    if (field.from === "button") m.panes.detail?.querySelector<HTMLElement>("[data-comment-button]")?.focus();
    else {
      const first = keys.get(idOf(field.name) ?? "")?.[0];
      v.canvas?.querySelector<HTMLElement>(`.svelte-flow__node[data-id="${CSS.escape(first ?? "")}"]`)?.focus({ preventScroll: true });
    }
  }

  /** At block zoom render only the editor over its Artifact, without moving the camera. */
  const blockField = $derived.by(() => {
    const field = editable && painted ? defs.field : null;
    if (!field) return null;
    const key = keys.get(idOf(field.name) ?? "")?.[0];
    const place = key === undefined ? undefined : rects.get(key);
    const copy = key === undefined ? undefined : byKey.get(key);
    if (!place || !copy) return null;
    const { x, y, zoom } = camera.current;
    return { name: field.name, ...commentRect({
      x: x + place.x * zoom, y: y + place.y * zoom, width: place.width * zoom, height: place.height * zoom,
    }, copy.parts.length > 0, zoom) };
  });

  /** An Artifact just created or renamed: once the scan draws it, its Detail opens; a created
   * one is brought to the middle of the graph left of the Detail, as a found Artifact is. */
  const created: Attachment<HTMLElement> = () => {
    const made = defs.created;
    const id = made === null ? undefined : idOf(made.name);
    if (made === null || id === undefined || !keys.has(id)) return;
    defs.created = null;
    if (made.centre) void choose({ node: id, tab: null } as Found);
    else onopen(id);
  };

  const newArtifact = () => (defs.dialog = { kind: "new" });
  const resetTop = () => void edit({ kind: "resetPlaces", whole: null });
  const canResetTop = $derived(editable && annotations.at.size > 0);

  useCommands(() => [
    { id: "graph.find", group: "Graph", label: "Find", keys: "Ctrl+F", run: () => (m.finding = true) },
    { id: "graph.zoom-out", group: "Graph", label: "Zoom out", run: () => zoomBy(1 / ZOOM_STEP) },
    { id: "graph.zoom-in", group: "Graph", label: "Zoom in", run: () => zoomBy(ZOOM_STEP) },
    { id: "graph.fit", group: "Graph", label: "Fit", run: fitAll },
    { id: "graph.new", group: "Graph", label: "New Artifact", enabled: () => editable, run: newArtifact },
    { id: "graph.reset-layout", group: "Graph", label: "Reset layout", enabled: () => canResetTop, run: resetTop },
    {
      id: "graph.minimize",
      group: "Graph",
      label: "Minimize or expand the whole",
      keys: "m",
      enabled: () => wholeAtHand() !== undefined,
      run: () => {
        const key = wholeAtHand();
        if (key !== undefined) void minimize(key, !byKey.get(key)!.minimized);
      },
    },
  ], () => shown);

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
  <!-- What acts on the graph: its tools, among the stage header's (D-GRAPH-4). -->
  {#snippet finder()}
    <GraphFind {source} {branch} onchoose={(found) => void choose(found)} onclose={() => (m.finding = false)} />
  {/snippet}
  {#snippet edits()}
    <button class="tool" onclick={newArtifact}><Icon name="add" />New Artifact</button>
    {#if canResetTop}
      <button class="tool" onclick={resetTop}><Icon name="reset" />Reset layout</button>
    {/if}
  {/snippet}
  <GraphTools zoom={camera.current.zoom} act={{ find: () => (m.finding = true), zoomBy, fit: fitAll }} finder={m.finding ? finder : undefined} children={editable ? edits : undefined} />
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="canvas"
    class:far={gridSize < GRID_SHOWN}
    class:empty-run={graph.nodes.length === 0 && !editable}
    data-graph-area
    style:background-size="{gridSize}px {gridSize}px"
    style:background-position="{camera.current.x}px {camera.current.y}px"
    {@attach measure}
    {@attach docked}
    {@attach grid}
    {@attach created}
    {@attach escaping}
    {@attach measuring}
    onkeydown={keydown}
    onpointerdowncapture={(event) => (v.pressed = pointOf(event))}
    onfocusin={focusIn}
    onfocusout={() => (v.focused = null)}
    tabindex="-1"
  >
    {#if graph.nodes.length === 0}<p class="empty">{empty}</p>{/if}
    {#if blockField}
      <div class="block-field" style:left="{blockField.x}px" style:top="{blockField.y}px" style:width="{blockField.width}px">
        <CommentField name={blockField.name} onclose={(written) => void closeField(written)} />
      </div>
    {/if}
    {#if canvased}<canvas class="paint" aria-hidden="true" {@attach draw}></canvas>{/if}
    <SvelteFlow
      bind:nodes
      bind:edges
      {nodeTypes}
      fitView={!large}
      {fitViewOptions}
      minZoom={MIN_ZOOM}
      maxZoom={MAX_ZOOM}
      nodesDraggable={editable && !painted}
      nodeDragThreshold={DRAG}
      onnodedragstart={({ targetNode }) => targetNode && dragStart(targetNode.id)}
      onnodedrag={({ targetNode, event }) => targetNode && dragMove(targetNode.id, event)}
      onnodedragstop={({ targetNode, event }) => targetNode && void dragStop(targetNode, event)}
      nodesConnectable={false}
      elementsSelectable={false}
      autoPanOnNodeFocus={false}
      autoPanOnNodeDrag={false}
      elevateNodesOnSelect={false}
      onlyRenderVisibleElements={drawn.length > 80}
      oninit={() => {
        v.initialized = true;
        // The first draw shows whole; a canvas without a size yet (a hidden screen) gets its
        // first view once it has one.
        first();
      }}
      proOptions={{ hideAttribution: true }}
      onnodeclick={({ node }) => {
        if (v.panned) v.panned = false;
        else onopen(byKey.get(node.id)?.node.id ?? node.id);
      }}
      onpaneclick={({ event }) => background(event)}
    />
    {#if v.comment}
      {@const text = annotations.comments.get(byKey.get(v.comment.key)?.node.name ?? "")}
      <p class="lifted" aria-hidden="true" style:left="{v.comment.x}px" style:top="{v.comment.y}px" style:width="{v.comment.shape.width}px" style:padding={v.comment.shape.padding} style:transform="scale({v.comment.shape.zoom})">{text}</p>
    {/if}
    {#if !allInView && v.area}
      <Navigator {laid} {lights} viewport={camera.current} width={visible} height={v.area.height} onmove={navigate} />
    {/if}
  </div>
</div>

<style>
  .block-field { position: absolute; z-index: 2; }
  /* No Artifacts: the sentence stands at the top of the canvas, under the tool strip, which
     acts on the empty canvas as on any. */
  .empty {
    position: absolute;
    inset: 0 0 auto 0;
    z-index: 1;
    margin: 0;
    padding: 6px var(--pad);
    color: var(--ink-2);
    pointer-events: none;
  }
  .graph {
    position: relative;
    height: 100%;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    grid-template-columns: minmax(0, 1fr);
    min-height: 0;
  }
  /* The working surface, opened by a group line under the strip, with a faint grid that moves
     and scales with the camera, left out where its cells grow too small. */
  .canvas {
    position: relative;
    min-height: 0;
    border-top: 1px solid var(--line-group);
    background-color: var(--surface-app);
    background-image:
      linear-gradient(to right, color-mix(in srgb, var(--line-group) 40%, transparent) 1px, transparent 1px),
      linear-gradient(to bottom, color-mix(in srgb, var(--line-group) 40%, transparent) 1px, transparent 1px);
  }
  .canvas.far {
    background-image: none;
  }
  .canvas.empty-run {
    background: var(--surface-panel);
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
  }
  /* Edges are neutral; one that leaves a RED, ERROR or blocked Artifact takes its colour. */
  .canvas :global(.svelte-flow__edge path) {
    stroke: var(--edge);
  }
  .canvas :global(.svelte-flow__edge.blocking path) {
    stroke: var(--light);
  }
  .canvas :global(.svelte-flow__node) {
    cursor: pointer;
  }
  /* A node being dragged floats: raised one step with the faint shadow, its face and strip as
     they were; a whole's box keeps its face and takes the shadow. */
  /* Lift the face without replacing its status strip, stack, or current ring. */
  .canvas :global(.svelte-flow__node.dragging .graph-face) {
    background: var(--surface-raised);
    --node-lift: 0 2px 8px var(--scrim);
  }
  /* A comment being dragged: its text alone follows the pointer, in the ink, width and lines it
     has under its node, lifted by a faint shadow on its letters, with no surface or box. */
  .lifted {
    position: absolute;
    z-index: 5;
    box-sizing: border-box;
    margin: 0;
    transform-origin: 0 0;
    color: var(--ink-2);
    font-size: var(--text-small);
    line-height: 1.4;
    text-shadow: 0 1px 3px var(--scrim);
    pointer-events: none;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    overflow: hidden;
  }
  .canvas :global(.svelte-flow__node:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: 5px;
    border-radius: var(--radius-control);
  }
  .canvas :global(.svelte-flow__node:focus-visible:has(.minimized)) {
    outline-offset: 9px;
  }
</style>
