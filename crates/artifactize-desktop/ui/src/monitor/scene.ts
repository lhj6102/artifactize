// A large graph on a canvas. The window keeps only a few hundred Artifacts as elements; a Run
// of tens of thousands draws the rest here: every edge, and every Artifact in view while too
// many are in view to keep as elements. Where things are comes from the layout; this only
// finds what is in view or under the pointer and paints it, in graph coordinates under the
// camera's transform.
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
const RING = 3;
/** How far a picture reaches beyond each side of the canvas, as a share of the canvas: the
 * camera pans this far before the picture is painted again. */
const AROUND = 0.5;
/** How far beyond the canvas an Artifact still counts as in view, as a share of the canvas:
 * panning a little never finds one missing. */
const MARGIN = 0.25;

/** The laid-out Artifacts: where each is, and an index of them by area. */
export type Scene = {
  ids: string[];
  places: Point[];
  /** Each Artifact's number by id. */
  numbers: Map<string, number>;
  /** All of them, or an empty box for none. */
  bounds: Rect;
  cells: Map<number, number[]>;
};

const cell = (value: number) => Math.floor(value / CELL);

/** The scene of `positions`. */
export function scene(positions: Map<string, Point>): Scene {
  const ids = [...positions.keys()];
  const places = [...positions.values()];
  const cells = new Map<number, number[]>();
  let [left, top, right, bottom] = [Infinity, Infinity, -Infinity, -Infinity];
  places.forEach((at, number) => {
    [left, top] = [Math.min(left, at.x), Math.min(top, at.y)];
    [right, bottom] = [Math.max(right, at.x + NODE.width), Math.max(bottom, at.y + NODE.height)];
    for (let x = cell(at.x); x <= cell(at.x + NODE.width); x += 1) {
      for (let y = cell(at.y); y <= cell(at.y + NODE.height); y += 1) {
        const key = x * ROW + y;
        const found = cells.get(key);
        if (found) found.push(number);
        else cells.set(key, [number]);
      }
    }
  });
  const bounds = places.length === 0 ? { x: 0, y: 0, width: 0, height: 0 } : { x: left, y: top, width: right - left, height: bottom - top };
  return { ids, places, numbers: new Map(ids.map((id, number) => [id, number])), bounds, cells };
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
        const meets = place.x < rect.x + rect.width && place.x + NODE.width > rect.x && place.y < rect.y + rect.height && place.y + NODE.height > rect.y;
        if (meets) found.add(number);
        if (found.size > most) return null;
      }
    }
  }
  return [...found].sort((one, other) => one - other);
}

/** The Artifact whose box holds `point`, if any. */
export function hit(at: Scene, point: Point): string | null {
  const number = (at.cells.get(cell(point.x) * ROW + cell(point.y)) ?? []).find((each) => {
    const place = at.places[each];
    return point.x >= place.x && point.x <= place.x + NODE.width && point.y >= place.y && point.y <= place.y + NODE.height;
  });
  return number === undefined ? null : at.ids[number];
}

/** What to paint: the graph's lights, the camera, the canvas's size and pixel ratio, whether
 * the Artifacts are painted as boxes, and the opened one. */
export type Paint = {
  graph: Shape;
  viewport: Viewport;
  width: number;
  height: number;
  ratio: number;
  boxes: boolean;
  opened: string | null;
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
 * edges in view, then the Artifacts in view as boxes if asked, in the theme's lights. Edges are
 * one device pixel wide, which the canvas draws many times faster than wider ones. Of edges
 * whose ends fall into the same `GRAIN` cells of the screen only the first is drawn, and one
 * shorter than a cell not at all: at a whole catalog's scale, thousands of edges from one
 * shared input end in the same few pixels. */
function paint(context: CanvasRenderingContext2D, at: Scene, what: Paint) {
  const style = getComputedStyle(document.documentElement);
  const colour = (light: Light) => style.getPropertyValue(`--light-${light}`).trim();
  const { viewport, ratio, width, height } = what;
  const { zoom, x: left, y: top } = viewport;
  context.setTransform(1, 0, 0, 1, 0, 0);
  context.clearRect(0, 0, width * ratio, height * ratio);
  context.setTransform(ratio, 0, 0, ratio, 0, 0);
  context.lineWidth = 1 / ratio;

  const { sources, targets, lights } = ends(at, what.graph);
  const paths = new Map<Light, Path2D>();
  const drawn = new Set<number>();
  const grain = (value: number) => Math.min(Math.max(Math.floor(value / GRAIN), -FAR), FAR) + FAR;
  for (let edge = 0; edge < sources.length; edge += 1) {
    const [source, target] = [at.places[sources[edge]], at.places[targets[edge]]];
    const [x1, y1] = [(source.x + NODE.width) * zoom + left, (source.y + NODE.height / 2) * zoom + top];
    const [x2, y2] = [target.x * zoom + left, (target.y + NODE.height / 2) * zoom + top];
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

  const lit = new Map(what.graph.nodes.map((node) => [node.id, node.light]));
  const boxes = new Map<Light, Path2D>();
  const shown = { x: -left / zoom, y: -top / zoom, width: width / zoom, height: height / zoom };
  for (const number of within(at, shown) ?? []) {
    const light = lit.get(at.ids[number]) ?? "waiting";
    let path = boxes.get(light);
    if (!path) boxes.set(light, (path = new Path2D()));
    const place = at.places[number];
    path.rect(place.x * zoom + left, place.y * zoom + top, Math.max(NODE.width * zoom, 1), Math.max(NODE.height * zoom, 1));
  }
  for (const [light, path] of boxes) {
    context.fillStyle = colour(light);
    context.fill(path);
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

  /** A ring around the opened Artifact, as its element draws it. */
  #ring(context: CanvasRenderingContext2D, at: Scene, what: Paint) {
    const opened = what.opened === null ? undefined : at.numbers.get(what.opened);
    if (opened === undefined) return;
    const { viewport, ratio } = what;
    const place = at.places[opened];
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.strokeStyle = getComputedStyle(document.documentElement).getPropertyValue("--accent").trim();
    context.lineWidth = RING;
    const [x, y] = [place.x * viewport.zoom + viewport.x, place.y * viewport.zoom + viewport.y];
    context.strokeRect(x - RING, y - RING, NODE.width * viewport.zoom + 2 * RING, NODE.height * viewport.zoom + 2 * RING);
  }
}
