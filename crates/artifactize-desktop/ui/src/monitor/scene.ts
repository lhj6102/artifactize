// A large graph on a canvas. The window keeps only a few hundred Artifacts as elements; a Run
// of tens of thousands draws the rest here: every edge, and every Artifact in view while too
// many are in view to keep as elements. Where things are comes from the layout; this only
// finds what is in view or under the pointer and paints it, in graph coordinates under the
// camera's transform. A whole's box is painted as its outline, its parts inside it.
import type { Rect, Viewport } from "@xyflow/svelte";
import type { Light, Shape } from "../ipc/client";
import type { Point } from "./graph";
import { NODE } from "./layout";

/** The side of one cell of the index, in graph coordinates: a few Artifacts across. */
const CELL = 1024;
/** Cells per row of the index's keys: more than any layout's columns of cells. */
const ROW = 1 << 20;
/** The side of a screen cell in which edges that start and end alike are drawn once. */
const GRAIN = 8;
/** Cells counted beyond the canvas on each side, and the cells a key spans: an edge's end
 * further out is drawn as if at the last one. */
const FAR = 1024;
const SPAN = 2 * FAR + 1;
/** The width of the ring around the opened Artifact, as its element draws it. */
const RING = 2;
/** A node's anatomy in graph pixels, as ArtifactNode.svelte draws it: its side padding, its
 * dot and the gap before its name. Its corner radius and its name's font size are the theme's
 * tokens, read when painting. */
const NODE_PAD = 12;
const NODE_DOT = 8;
const NODE_GAP = 8;
/** The smallest font, in device-independent pixels, at which a name is still painted. */
const LEGIBLE = 5;
/** An edge's stroke in graph pixels, as the graph's elements draw it. */
const EDGE_WIDTH = 1.25;
/** How far a picture reaches beyond each side of the canvas, as a share of the canvas: the
 * camera pans this far before the picture is painted again. */
const AROUND = 0.5;
/** How far beyond the canvas an Artifact still counts as in view, as a share of the canvas:
 * panning a little never finds one missing. */
const MARGIN = 0.25;

/** The laid-out Artifacts: where each is, and an index of them by area. */
export type Scene = {
  /** The drawn copies' keys (parts.ts), each whole before its parts. */
  ids: string[];
  places: Rect[];
  /** The numbers of the copies that are wholes, drawn as boxes around their parts. */
  wholes: Set<number>;
  /** The Artifact's tree id of each copy. */
  nodes: string[];
  /** Each Artifact's number by id. */
  numbers: Map<string, number>;
  /** All of them, or an empty box for none. */
  bounds: Rect;
  cells: Map<number, number[]>;
};

const cell = (value: number) => Math.floor(value / CELL);

/** The scene of `positions`, each of `NODE`'s size unless it has its own; `wholes` and
 * `nodes` say which keys are wholes and whose copies they are, each its own Artifact without. */
export function scene(positions: Map<string, Point & Partial<Rect>>, wholes: Set<string> = new Set(), nodes: Map<string, string> = new Map()): Scene {
  const ids = [...positions.keys()];
  const places = [...positions.values()].map((at) => ({ x: at.x, y: at.y, width: at.width ?? NODE.width, height: at.height ?? NODE.height }));
  const cells = new Map<number, number[]>();
  let [left, top, right, bottom] = [Infinity, Infinity, -Infinity, -Infinity];
  places.forEach((at, number) => {
    [left, top] = [Math.min(left, at.x), Math.min(top, at.y)];
    [right, bottom] = [Math.max(right, at.x + at.width), Math.max(bottom, at.y + at.height)];
    for (let x = cell(at.x); x <= cell(at.x + at.width); x += 1) {
      for (let y = cell(at.y); y <= cell(at.y + at.height); y += 1) {
        const key = x * ROW + y;
        const found = cells.get(key);
        if (found) found.push(number);
        else cells.set(key, [number]);
      }
    }
  });
  const bounds = places.length === 0 ? { x: 0, y: 0, width: 0, height: 0 } : { x: left, y: top, width: right - left, height: bottom - top };
  const numbers = new Map(ids.map((id, number) => [id, number]));
  return {
    ids,
    places,
    wholes: new Set([...wholes].flatMap((key) => numbers.get(key) ?? [])),
    nodes: ids.map((id) => nodes.get(id) ?? id),
    numbers,
    bounds,
    cells,
  };
}

/** What a canvas of `width` × `height` shows under `viewport`, in graph coordinates, with a
 * margin around it. */
export function view(viewport: Viewport, width: number, height: number): Rect {
  const [wide, high] = [width / viewport.zoom, height / viewport.zoom];
  return { x: -viewport.x / viewport.zoom - wide * MARGIN, y: -viewport.y / viewport.zoom - high * MARGIN, width: wide * (1 + 2 * MARGIN), height: high * (1 + 2 * MARGIN) };
}

/** The Artifacts whose boxes meet `rect`, in layout order; `null` as soon as more than `most`
 * do, which spares counting a whole catalog in view on every frame. */
export function within(at: Scene, rect: Rect, most = Infinity): number[] | null {
  const found = new Set<number>();
  for (let x = cell(rect.x); x <= cell(rect.x + rect.width); x += 1) {
    for (let y = cell(rect.y); y <= cell(rect.y + rect.height); y += 1) {
      for (const number of at.cells.get(x * ROW + y) ?? []) {
        const place = at.places[number];
        const meets = place.x < rect.x + rect.width && place.x + place.width > rect.x && place.y < rect.y + rect.height && place.y + place.height > rect.y;
        if (meets) found.add(number);
        if (found.size > most) return null;
      }
    }
  }
  return [...found].sort((one, other) => one - other);
}

/** The copy whose box holds `point`, if any: of a whole and its part, the part. */
export function hit(at: Scene, point: Point): string | null {
  const holding = (at.cells.get(cell(point.x) * ROW + cell(point.y)) ?? []).filter((each) => {
    const place = at.places[each];
    return point.x >= place.x && point.x <= place.x + place.width && point.y >= place.y && point.y <= place.y + place.height;
  });
  // A part comes after the wholes around it.
  return holding.length === 0 ? null : at.ids[Math.max(...holding)];
}

/** What to paint: the graph's lights, the camera, the canvas's size and pixel ratio, whether
 * the Artifacts are painted as boxes, and the copies that show the opened Artifact. */
export type Paint = {
  graph: Shape;
  viewport: Viewport;
  width: number;
  height: number;
  ratio: number;
  boxes: boolean;
  opened: ReadonlySet<string>;
};

/** The graph's edges by the numbers of their ends in a scene, with their lights. */
type Ends = { sources: Int32Array; targets: Int32Array; lights: Light[] };

/** Each graph's edges by number, worked out once per scene and set of lights. */
const ended = new WeakMap<Scene, WeakMap<object, Ends>>();

function ends(at: Scene, graph: Shape): Ends {
  const known = ended.get(at)?.get(graph.edges);
  if (known) return known;
  const kept = graph.edges.filter((edge) => at.numbers.has(edge.source) && at.numbers.has(edge.target));
  const found = {
    sources: Int32Array.from(kept, (edge) => at.numbers.get(edge.source)!),
    targets: Int32Array.from(kept, (edge) => at.numbers.get(edge.target)!),
    lights: kept.map((edge) => edge.light),
  };
  if (!ended.has(at)) ended.set(at, new WeakMap());
  ended.get(at)!.set(graph.edges, found);
  return found;
}

/** Paint `what` onto `context`, which is `what.width` × `what.height` at `what.ratio`: the
 * edges in view, then the Artifacts in view as nodes if asked, in the theme's lights. Edges are
 * at least one device pixel wide, the regular stroke once the zoom asks for more; one pixel is
 * what the canvas draws many times faster than wider ones. Of edges
 * whose ends fall into the same `GRAIN` cells of the screen only the first is drawn, and one
 * shorter than a cell not at all: at a whole catalog's scale, thousands of edges from one
 * shared input end in the same few pixels. */
/** `text` as it fits in `width` with `context`'s font: whole, or cut with an ellipsis, as a
 * node's element cuts its name. */
function cut(context: CanvasRenderingContext2D, text: string, width: number): string {
  if (context.measureText(text).width <= width) return text;
  let [fits, over] = [0, text.length];
  while (over - fits > 1) {
    const middle = (fits + over) >> 1;
    if (context.measureText(`${text.slice(0, middle)}…`).width <= width) fits = middle;
    else over = middle;
  }
  return `${text.slice(0, fits)}…`;
}

function paint(context: CanvasRenderingContext2D, at: Scene, what: Paint) {
  const style = getComputedStyle(document.documentElement);
  const colour = (light: Light) => style.getPropertyValue(`--light-${light}`).trim();
  const { viewport, ratio, width, height } = what;
  const { zoom, x: left, y: top } = viewport;
  context.setTransform(1, 0, 0, 1, 0, 0);
  context.clearRect(0, 0, width * ratio, height * ratio);
  context.setTransform(ratio, 0, 0, ratio, 0, 0);
  context.lineWidth = Math.max(1 / ratio, EDGE_WIDTH * zoom);

  const { sources, targets, lights } = ends(at, what.graph);
  const paths = new Map<Light, Path2D>();
  const drawn = new Set<number>();
  const grain = (value: number) => Math.min(Math.max(Math.floor(value / GRAIN), -FAR), FAR) + FAR;
  for (let edge = 0; edge < sources.length; edge += 1) {
    const [source, target] = [at.places[sources[edge]], at.places[targets[edge]]];
    const [x1, y1] = [(source.x + source.width) * zoom + left, (source.y + source.height / 2) * zoom + top];
    const [x2, y2] = [target.x * zoom + left, (target.y + target.height / 2) * zoom + top];
    // Out of view: both ends on one side of the canvas.
    if ((x1 < 0 && x2 < 0) || (x1 > width && x2 > width) || (y1 < 0 && y2 < 0) || (y1 > height && y2 > height)) continue;
    const cells = [grain(x1), grain(y1), grain(x2), grain(y2)];
    if (cells[0] === cells[2] && cells[1] === cells[3]) continue;
    const key = ((cells[0] * SPAN + cells[1]) * SPAN + cells[2]) * SPAN + cells[3];
    if (drawn.has(key)) continue;
    drawn.add(key);
    let path = paths.get(lights[edge]);
    if (!path) paths.set(lights[edge], (path = new Path2D()));
    // Svelte Flow's bezier from the source's right side to the target's left.
    const offset = x2 >= x1 ? 0.5 * (x2 - x1) : 0.25 * 25 * Math.sqrt((x1 - x2) * zoom);
    path.moveTo(x1, y1);
    path.bezierCurveTo(x1 + offset, y1, x2 - offset, y2, x2, y2);
  }
  for (const [light, path] of paths) {
    context.strokeStyle = colour(light);
    context.stroke(path);
  }
  if (!what.boxes) return;

  // The Artifacts in view, painted as their elements draw them: a whole as its box's outline, a
  // node as a neutral face with a border and a dot in its light's colour and its name, cut to
  // the face, once the zoom makes a name legible.
  const named = new Map(what.graph.nodes.map((node) => [node.id, node]));
  const boxes = new Map<Light, Path2D>();
  const outlines = new Map<Light, Path2D>();
  const dots = new Map<Light, Path2D>();
  const names: { x: number; y: number; width: number; text: string }[] = [];
  const shown = { x: -left / zoom, y: -top / zoom, width: width / zoom, height: height / zoom };
  const pixels = (token: string) => Number.parseFloat(style.getPropertyValue(token)) || 0;
  const [corner, font] = [pixels("--radius-control"), pixels("--text-body")];
  const radius = Math.min(corner * zoom, corner);
  const legible = font * zoom >= LEGIBLE;
  for (const number of within(at, shown) ?? []) {
    const node = named.get(at.nodes[number]);
    const light = node?.light ?? "waiting";
    const whole = at.wholes.has(number);
    const paths = whole ? outlines : boxes;
    let path = paths.get(light);
    if (!path) paths.set(light, (path = new Path2D()));
    const place = at.places[number];
    const [x, y, w, h] = [place.x * zoom + left, place.y * zoom + top, Math.max(place.width * zoom, 1), Math.max(place.height * zoom, 1)];
    path.roundRect(x, y, w, h, radius);
    if (whole) continue;
    let dot = dots.get(light);
    if (!dot) dots.set(light, (dot = new Path2D()));
    const [cx, cy] = [x + NODE_PAD * zoom + (NODE_DOT / 2) * zoom, y + h / 2];
    dot.moveTo(cx + (NODE_DOT / 2) * zoom, cy);
    dot.arc(cx, cy, Math.max((NODE_DOT / 2) * zoom, 0.5), 0, 2 * Math.PI);
    if (legible && node) names.push({ x: x + (NODE_PAD + NODE_DOT + NODE_GAP) * zoom, y: cy, width: w - (2 * NODE_PAD + NODE_DOT + NODE_GAP) * zoom, text: node.name });
  }
  // A node's border is its own: one pixel at the node's scale, never an edge's stroke.
  context.lineWidth = Math.max(1 / ratio, zoom);
  for (const [light, path] of outlines) {
    context.strokeStyle = colour(light);
    context.stroke(path);
  }
  context.fillStyle = style.getPropertyValue("--surface-panel").trim();
  for (const path of boxes.values()) context.fill(path);
  for (const [light, path] of boxes) {
    context.strokeStyle = colour(light);
    context.stroke(path);
  }
  for (const [light, path] of dots) {
    context.fillStyle = colour(light);
    context.fill(path);
  }
  if (names.length === 0) return;
  context.fillStyle = style.getPropertyValue("--ink").trim();
  context.font = `500 ${font * zoom}px ${style.getPropertyValue("--font").trim()}`;
  context.textBaseline = "middle";
  for (const name of names) {
    context.save();
    context.beginPath();
    context.rect(name.x, name.y - font * zoom, name.width, 2 * font * zoom);
    context.clip();
    context.fillText(cut(context, name.text, name.width), name.x, name.y);
    context.restore();
  }
}

/** A large graph's picture: painted once around the view, a canvas wider and taller than it,
 * and then moved as the camera pans, so that panning copies one image a frame instead of
 * painting tens of thousands of edges. A change of zoom scales the picture until it is painted
 * again at the new zoom; anything else (lights, size, boxes) paints it again at once. */
export class Picture {
  #image: HTMLCanvasElement | null = null;
  /** What the image holds: the scene and paint it was painted from, and its camera. */
  #held: { at: Scene; what: Paint; viewport: Viewport } | null = null;

  /** Show `what` on `context`. False when the picture is scaled rather than painted for the
   * camera's zoom: the caller paints it again with `sharp` once the zoom settles. */
  show(context: CanvasRenderingContext2D, at: Scene, what: Paint, sharp = false): boolean {
    const { viewport, ratio, width, height } = what;
    const held = this.#held;
    const same = held && held.at === at && held.what.graph === what.graph && held.what.boxes === what.boxes
      && held.what.width === width && held.what.height === height && held.what.ratio === ratio;
    const scale = held ? viewport.zoom / held.viewport.zoom : 1;
    // Where the image's top-left corner lands on the canvas, in canvas pixels.
    const [x, y] = held
      ? [viewport.x - held.viewport.x * scale, viewport.y - held.viewport.y * scale]
      : [0, 0];
    const [wide, high] = [width * (1 + 2 * AROUND), height * (1 + 2 * AROUND)];
    const covers = x <= 0 && y <= 0 && x + wide * scale >= width && y + high * scale >= height;
    let painted = true;
    if (!same || !this.#image || (scale === 1 ? !covers : sharp)) {
      this.#image ??= document.createElement("canvas");
      const image = this.#image;
      [image.width, image.height] = [Math.round(wide * ratio), Math.round(high * ratio)];
      const around = { x: viewport.x + width * AROUND, y: viewport.y + height * AROUND, zoom: viewport.zoom };
      const drawing = image.getContext("2d");
      if (drawing) paint(drawing, at, { ...what, viewport: around, width: wide, height: high });
      this.#held = { at, what, viewport: around };
    } else painted = scale === 1;
    const { viewport: from } = this.#held!;
    const factor = viewport.zoom / from.zoom;
    context.setTransform(1, 0, 0, 1, 0, 0);
    context.clearRect(0, 0, width * ratio, height * ratio);
    context.setTransform(ratio * factor, 0, 0, ratio * factor, (viewport.x - from.x * factor) * ratio, (viewport.y - from.y * factor) * ratio);
    context.drawImage(this.#image!, 0, 0, this.#image!.width / ratio, this.#image!.height / ratio);
    this.#ring(context, at, what);
    return painted;
  }

  /** A ring around every copy that shows the opened Artifact, as its elements draw it. */
  #ring(context: CanvasRenderingContext2D, at: Scene, what: Paint) {
    if (what.opened.size === 0) return;
    const { viewport, ratio } = what;
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.strokeStyle = getComputedStyle(document.documentElement).getPropertyValue("--outline-selected").trim();
    context.lineWidth = RING;
    at.ids.forEach((key, number) => {
      if (!what.opened.has(key)) return;
      const place = at.places[number];
      const [x, y] = [place.x * viewport.zoom + viewport.x, place.y * viewport.zoom + viewport.y];
      context.strokeRect(x - RING, y - RING, place.width * viewport.zoom + 2 * RING, place.height * viewport.zoom + 2 * RING);
    });
  }
}
