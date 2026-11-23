// A large graph on a canvas. The window keeps only a few hundred Artifacts as elements; a Run
// of tens of thousands draws the rest here: every edge, and every Artifact in view while too
// many are in view to keep as elements. Where things are comes from the layout; this only
// finds what is in view or under the pointer and paints it, in graph coordinates under the
// camera's transform. A whole's box is painted as its outline, its parts inside it.
import type { Rect, Viewport } from "@xyflow/svelte";
import type { Light, Shape } from "../ipc/client";
import { ICONS, LIGHTS } from "../shell/icons";
import { BLOCKING, type Point } from "./graph";
import { NODE } from "./layout";
import { BOX, partsCount } from "./parts";

/** A comment's gap under its node, its inset inside a box, its line height and its lines, as
 * its element draws it. */
const [COMMENT_GAP, COMMENT_INSET, COMMENT_LEADING, COMMENT_LINES] = [4, 10, 1.4, 3];

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
const RING = 1;
const RING_GAP = 2;
/** A node's anatomy in graph pixels, as ArtifactNode.svelte draws it: its padding on the left
 * and the right, and the gap after its icon, before its tally. Its corner radius, its icon's
 * box, its strip, its port and its fonts are the theme's tokens, read when painting. */
const NODE_PAD = 12;
const NODE_PAD_END = 10;
const NODE_GAP = 8;
/** A whole's Minimize or Expand button: its side, and its gap to the title row's end, as its
 * element draws it. */
const TOGGLE = 20;
const TOGGLE_END = 6;
/** The line height of a minimized whole's name and its parts count, as its element sets it. */
const LINE = 1.2;
/** How far a minimized whole's second outline sits behind it, down and right. */
const STACK = 3;
/** A whole's box face: the panel's colour at this opacity. */
const WHOLE_FACE = 0.4;
/** The icon set's own box, in which its shapes are drawn (icons.ts). */
const ICON_BOX = 12;
/** The smallest font, in device-independent pixels, at which a name still reads. */
const LEGIBLE = 8;
/** An edge's stroke in graph pixels, as the graph's elements draw it. */
const EDGE_WIDTH = 1.25;
/** How far a picture reaches beyond each side of the canvas, as a share of the canvas: the
 * camera pans this far before the picture is painted again. */
const AROUND = 0.5;
/** How far beyond the canvas an Artifact still counts as in view, as a share of the canvas:
 * panning a little never finds one missing. */
const MARGIN = 0.25;

/** Whether names in a font of `font` graph pixels read at `zoom`: further out, an Artifact is a
 * block of its status's colour and a whole its box's outline. */
export function legible(font: number, zoom: number): boolean {
  return font * zoom >= LEGIBLE;
}

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
 * the Artifacts are painted as boxes, the copies that show the opened Artifact, and the
 * minimized wholes by key with how many parts each holds. */
export type Paint = {
  graph: Shape;
  viewport: Viewport;
  width: number;
  height: number;
  ratio: number;
  boxes: boolean;
  opened: ReadonlySet<string>;
  minimized: ReadonlyMap<string, number>;
  /** Each Artifact's comment by name, painted under its node or a box's title row. */
  comments?: ReadonlyMap<string, string>;
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

/** `text` in at most `most` lines of `width` with `context`'s font, wrapped at spaces and its
 * own line breaks, the last one cut with an ellipsis when more is left, as a comment's element
 * clamps it. */
function wrap(context: CanvasRenderingContext2D, text: string, width: number, most: number): string[] {
  const found: string[] = [];
  for (const paragraph of text.split("\n")) {
    let line = "";
    for (const word of paragraph.split(" ")) {
      const next = line ? `${line} ${word}` : word;
      if (line && context.measureText(next).width > width) {
        found.push(line);
        line = word;
      } else line = next;
      // A word wider than the line, unbroken text such as Korean included, breaks inside it.
      while (context.measureText(line).width > width && line.length > 1) {
        let fits = 1;
        while (fits < line.length && context.measureText(line.slice(0, fits + 1)).width <= width) fits += 1;
        found.push(line.slice(0, fits));
        line = line.slice(fits);
      }
    }
    found.push(line);
  }
  if (found.length <= most) return found;
  const kept = found.slice(0, most);
  kept[most - 1] = cut(context, `${kept[most - 1]} ${found[most]}`, width);
  if (!kept[most - 1].endsWith("…")) kept[most - 1] = cut(context, `${kept[most - 1]}…`, width);
  return kept;
}

function paint(context: CanvasRenderingContext2D, at: Scene, what: Paint) {
  const style = getComputedStyle(document.documentElement);
  const token = (name: string) => style.getPropertyValue(name).trim();
  const colour = (light: Light) => token(LIGHTS[light].colour);
  const { viewport, ratio, width, height } = what;
  const { zoom, x: left, y: top } = viewport;
  context.setTransform(1, 0, 0, 1, 0, 0);
  context.clearRect(0, 0, width * ratio, height * ratio);
  context.setTransform(ratio, 0, 0, ratio, 0, 0);
  const readable = legible(Number.parseFloat(token("--text-body")), zoom);
  context.lineWidth = readable ? Math.max(1 / ratio, EDGE_WIDTH * zoom) : 1 / ratio;

  const { sources, targets, lights } = ends(at, what.graph);
  // Edges are neutral; those that leave a RED, ERROR or blocked Artifact take its colour.
  const paths = new Map<Light | null, Path2D>();
  const drawn = new Set<number>();
  const grain = (value: number) => Math.min(Math.max(Math.floor(value / GRAIN), -FAR), FAR) + FAR;
  // The Artifacts edges reach, and those they leave: each shows its port on that side.
  const [inward, outward] = [new Uint8Array(at.places.length), new Uint8Array(at.places.length)];
  for (let edge = 0; edge < sources.length; edge += 1) {
    inward[targets[edge]] = 1;
    outward[sources[edge]] = 1;
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
    const light = BLOCKING.has(lights[edge]) ? lights[edge] : null;
    let path = paths.get(light);
    if (!path) paths.set(light, (path = new Path2D()));
    // Svelte Flow's bezier from the source's right side to the target's left.
    const offset = x2 >= x1 ? 0.5 * (x2 - x1) : 0.25 * 25 * Math.sqrt((x1 - x2) * zoom);
    path.moveTo(x1, y1);
    path.bezierCurveTo(x1 + offset, y1, x2 - offset, y2, x2, y2);
  }
  // The neutral edges first, so a coloured one shows over them.
  for (const [light, path] of [...paths].sort(([one], [other]) => Number(one !== null) - Number(other !== null))) {
    context.strokeStyle = light === null ? token("--edge") : colour(light);
    context.stroke(path);
  }
  if (!what.boxes) return;

  // The Artifacts in view. Where a name would be too small to read, a node is one block of its
  // status's colour and a whole its box's outline in that colour. From the zoom at which names
  // read, each is painted as its element draws it: a node's neutral face and outline, its icon
  // and strip in its status's colour, its name, cut to the face, and its tally; a whole's
  // neutral box with its title row; and their ports, a triangle pointing in on the left side
  // and a square on the right, in the edges' neutral colour.
  const pixels = (name: string) => Number.parseFloat(token(name)) || 0;
  const [corner, font, label, icon, strip, port] = [pixels("--radius-control"), pixels("--text-body"), pixels("--text-label"), pixels("--icon"), pixels("--strip"), pixels("--port") * zoom];
  const named = new Map(what.graph.nodes.map((node) => [node.id, node]));
  const shown = { x: -left / zoom, y: -top / zoom, width: width / zoom, height: height / zoom };
  const radius = Math.min(corner * zoom, corner);
  const blocks = new Map<Light, Path2D>();
  const outlines = new Map<Light, Path2D>();
  const faces = new Path2D();
  /** A whole's box: the panel's face at 40%, behind its parts. */
  const boxFaces = new Path2D();
  /** A minimized whole's second outline behind it, for the parts it holds. */
  const stacked = new Path2D();
  const neutral = new Path2D();
  /** Each whole's Minimize or Expand control at its title row's end, by its button's corner. */
  const toggles: { icon: "minus" | "add"; x: number; y: number }[] = [];
  const strips = new Map<Light, Path2D>();
  const ports = new Path2D();
  const marks: { light: Light; x: number; y: number }[] = [];
  const names: { x: number; y: number; width: number; text: string; count: string; tally: string; whole: boolean }[] = [];
  /** Each comment in view: under a node, or inside a box under its title row. */
  const notes: { x: number; y: number; width: number; text: string }[] = [];
  const add = (into: Map<Light, Path2D>, light: Light) => {
    let path = into.get(light);
    if (!path) into.set(light, (path = new Path2D()));
    return path;
  };
  for (const number of within(at, shown) ?? []) {
    const node = named.get(at.nodes[number]);
    const light = node?.light ?? "waiting";
    const whole = at.wholes.has(number);
    const place = at.places[number];
    const [x, y, w, h] = [place.x * zoom + left, place.y * zoom + top, Math.max(place.width * zoom, 1), Math.max(place.height * zoom, 1)];
    if (!readable) {
      if (whole) add(outlines, light).rect(x, y, w, h);
      else add(blocks, light).rect(x, y, w, h);
      continue;
    }
    // A node's face with the strip in its status's colour along its bottom, or a whole's box
    // with the strip along its top edge and its title row under it; a whole, boxed or minimized,
    // ends its title row with its control.
    const folded = what.minimized.get(at.ids[number]);
    const row = whole ? Math.min(BOX.header * zoom, h) : h;
    if (whole) boxFaces.roundRect(x, y, w, h, radius);
    else faces.roundRect(x, y, w, h, radius);
    if (folded !== undefined) stacked.roundRect(x + STACK * zoom, y + STACK * zoom, w, h, radius);
    neutral.roundRect(x, y, w, h, radius);
    add(strips, light).rect(x, whole ? y : y + row - strip * zoom, w, strip * zoom);
    marks.push({ light, x: x + NODE_PAD * zoom, y: y + row / 2 - (icon * zoom) / 2 });
    const start = (NODE_PAD + icon + NODE_GAP) * zoom;
    let end = NODE_PAD_END * zoom;
    if (whole || folded !== undefined) {
      // The control as its element draws it: a button of TOGGLE pixels with its own outline.
      const side = TOGGLE * zoom;
      toggles.push({ icon: whole ? "minus" : "add", x: x + w - TOGGLE_END * zoom - side, y: y + row / 2 - side / 2 });
      end = (TOGGLE_END + TOGGLE + NODE_GAP) * zoom;
    }
    const tally = node?.tally ? `${node.tally.passed}/${node.tally.of}` : "";
    const count = folded === undefined ? "" : partsCount(folded);
    if (node) names.push({ x: x + start, y: y + row / 2, width: w - start - end, text: node.name, count, tally, whole });
    const comment = node ? what.comments?.get(node.name) : undefined;
    if (comment) {
      notes.push(whole
        ? { x: x + COMMENT_INSET * zoom, y: y + row + COMMENT_GAP * zoom, width: w - 2 * COMMENT_INSET * zoom, text: comment }
        : { x, y: y + h + COMMENT_GAP * zoom, width: w, text: comment });
    }
    // Wherever names are painted, so are the ports, however small.
    if (inward[number] || outward[number]) {
      const [middle, half] = [y + h / 2, port / 2];
      if (inward[number]) {
        ports.moveTo(x - half, middle - half);
        ports.lineTo(x + half, middle);
        ports.lineTo(x - half, middle + half);
        ports.closePath();
      }
      if (outward[number]) ports.rect(x + w - half, middle - half, port, port);
    }
  }
  for (const [light, path] of blocks) {
    context.fillStyle = colour(light);
    context.fill(path);
  }
  // A whole's or a node's outline is its own: one pixel at the graph's scale.
  context.lineWidth = Math.max(1 / ratio, Math.min(zoom, 1));
  for (const [light, path] of outlines) {
    context.strokeStyle = colour(light);
    context.stroke(path);
  }
  if (!readable) return;
  context.fillStyle = token("--surface-panel");
  context.globalAlpha = WHOLE_FACE;
  context.fill(boxFaces);
  context.globalAlpha = 1;
  context.fillStyle = token("--surface-app");
  context.fill(stacked);
  context.strokeStyle = token("--line-node");
  context.stroke(stacked);
  context.fillStyle = token("--surface-panel");
  context.fill(faces);
  context.stroke(neutral);
  for (const [light, path] of strips) {
    context.fillStyle = colour(light);
    context.fill(path);
  }
  context.fillStyle = token("--edge");
  context.fill(ports);
  // Each status's icon, drawn from the icon set's own shapes at the icon's box, and each
  // whole's control in secondary ink.
  const scale = (icon * zoom) / ICON_BOX;
  context.lineWidth = pixels("--icon-stroke");
  context.lineCap = "round";
  context.lineJoin = "round";
  const drawIcon = (name: keyof typeof ICONS, shade: string, x: number, y: number) => {
    context.save();
    context.translate(x, y);
    context.scale(scale, scale);
    for (const shape of ICONS[name]) {
      const path = new Path2D(shape.d);
      context.setLineDash(shape.dash ?? []);
      if (shape.fill) {
        context.fillStyle = shade;
        context.fill(path);
      } else {
        context.strokeStyle = shade;
        context.stroke(path);
      }
    }
    context.restore();
  };
  for (const mark of marks) drawIcon(LIGHTS[mark.light].icon, colour(mark.light), mark.x, mark.y);
  const side = TOGGLE * zoom;
  context.lineWidth = Math.max(1 / ratio, Math.min(zoom, 1));
  context.strokeStyle = token("--line-group");
  context.fillStyle = token("--surface-group");
  for (const toggle of toggles) {
    context.beginPath();
    context.roundRect(toggle.x, toggle.y, side, side, radius);
    context.fill();
    context.stroke();
    drawIcon(toggle.icon, token("--ink-2"), toggle.x + (side - icon * zoom) / 2, toggle.y + (side - icon * zoom) / 2);
  }
  const face = token("--font");
  context.textBaseline = "middle";
  for (const name of names) {
    let room = name.width;
    if (name.tally) {
      context.font = `400 ${label * zoom}px ${face}`;
      const wide = context.measureText(name.tally).width;
      context.fillStyle = token("--ink-2");
      context.fillText(name.tally, name.x + name.width - wide, name.y);
      room -= wide + NODE_GAP * zoom;
    }
    if (room <= 0) continue;
    context.font = `${name.whole ? 600 : 500} ${font * zoom}px ${face}`;
    context.fillStyle = token("--ink");
    context.save();
    context.beginPath();
    context.rect(name.x, name.y - font * zoom, room, 2 * font * zoom);
    context.clip();
    // A minimized whole says how many parts it holds below its name, the two lines centred.
    const lines = name.count ? [name.y - (label * zoom * LINE) / 2, name.y + (font * zoom * LINE) / 2] : [name.y];
    context.fillText(cut(context, name.text, room), name.x, lines[0]);
    if (name.count) {
      context.font = `400 ${label * zoom}px ${face}`;
      context.fillStyle = token("--ink-2");
      context.fillText(cut(context, name.count, room), name.x, lines[1]);
    }
    context.restore();
  }
  // Comments in the quieter ink at the small size, at most three lines.
  const small = pixels("--text-small");
  context.font = `400 ${small * zoom}px ${face}`;
  context.fillStyle = token("--ink-2");
  context.textBaseline = "top";
  for (const note of notes) {
    wrap(context, note.text, note.width, COMMENT_LINES).forEach((line, at) => context.fillText(line, note.x, note.y + at * small * COMMENT_LEADING * zoom));
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
      && held.what.width === width && held.what.height === height && held.what.ratio === ratio && held.what.minimized === what.minimized;
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

  /** A 1px current ring outside each open copy, clear of its face and any minimized stack. */
  #ring(context: CanvasRenderingContext2D, at: Scene, what: Paint) {
    // Legible nodes are DOM elements; those elements own their own selection ring.
    if (!what.boxes || what.opened.size === 0) return;
    const { viewport, ratio } = what;
    const style = getComputedStyle(document.documentElement);
    const corner = Number.parseFloat(style.getPropertyValue("--radius-control")) || 0;
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.strokeStyle = style.getPropertyValue("--current-mark").trim();
    context.lineWidth = RING;
    at.ids.forEach((key, number) => {
      if (!what.opened.has(key)) return;
      const place = at.places[number];
      const [x, y] = [place.x * viewport.zoom + viewport.x, place.y * viewport.zoom + viewport.y];
      const stack = what.minimized.has(key) ? Math.ceil(STACK * Math.SQRT2 * viewport.zoom) : 0;
      const offset = stack + RING_GAP + RING / 2;
      context.beginPath();
      context.roundRect(x - offset, y - offset, place.width * viewport.zoom + 2 * offset, place.height * viewport.zoom + 2 * offset, Math.min(corner * viewport.zoom, corner) + offset);
      context.stroke();
    });
  }
}
