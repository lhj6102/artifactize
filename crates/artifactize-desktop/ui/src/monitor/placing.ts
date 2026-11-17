// Where a drop on a Current's graph goes, and which places it saves. An Artifact dragged and
// released stays in the box it was in (or at the top level), joins the Artifact under the
// pointer as its part (a whole's box, or a node, which becomes a whole), or leaves its whole
// for the top level. Places are saved as shown: a box's parts from its inner corner, in the
// whole's file; the top level's in each node's own.
import type { Shape } from "../ipc/client";
import { roomOf, type Annotations } from "./annotations";
import type { Point } from "./graph";
import { BOX, header, type Copy, type Rect } from "./parts";

/** Where a release puts the dragged copy: placed in the box it was in (`place`), made a part of
 * the Artifact `to` (`join`, refused by the library when it would make a cycle), or out of its
 * whole to the top level (`leave`). */
export type Drop = { kind: "place" } | { kind: "join"; to: string; key: string } | { kind: "leave" };

const contains = (rect: Rect, point: Point) => point.x >= rect.x && point.x <= rect.x + rect.width && point.y >= rect.y && point.y <= rect.y + rect.height;

/** The copy drawn under `point`, the innermost, leaving out `skip` and the copies inside it. */
export function under(drawn: Copy[], rects: Map<string, Rect>, point: Point, skip: string | null = null): Copy | null {
  let found: Copy | null = null;
  // Parts come after their wholes: the last copy under the point is the innermost.
  for (const copy of drawn) {
    if (skip !== null && (copy.key === skip || copy.key.startsWith(`${skip}/`))) continue;
    const rect = rects.get(copy.key);
    if (rect && contains(rect, point)) found = copy;
  }
  return found;
}

/** Where releasing the dragged copy `dragged` with the pointer at `point` puts it. A copy is a
 * drop target unless it is the dragged Artifact itself, already holds it as a part, or is the
 * box it is dragged in; one inside it would make a cycle and is still the target the library
 * then refuses. Over no box it leaves its whole; over its own box it is placed. */
export function dropAt(graph: Shape, drawn: Copy[], rects: Map<string, Rect>, dragged: Copy, point: Point): Drop {
  const target = under(drawn, rects, point, dragged.key);
  if (target === null) return dragged.parent === null ? { kind: "place" } : { kind: "leave" };
  if (target.key === dragged.parent) return { kind: "place" };
  const parts = graph.nodes.find((node) => node.id === target.node.id)?.parts ?? [];
  if (target.node.id === dragged.node.id || parts.includes(dragged.node.id)) return target.parent === dragged.parent ? { kind: "place" } : { kind: "leave" };
  return { kind: "join", to: target.node.name, key: target.key };
}


/** The least place `annotations` stores for the parts of the whole `name`: its box starts there,
 * its parts placed from its inner corner. Nothing stored starts at its corner. */
function least(annotations: Annotations, name: string): Point {
  const stored = [...(annotations.places.get(name)?.values() ?? [])];
  if (stored.length === 0) return { x: 0, y: 0 };
  return { x: Math.min(...stored.map((at) => at.x)), y: Math.min(...stored.map((at) => at.y)) };
}

/** The place of a copy as its box or the top level stores it: a node's corner, or for a whole
 * drawn as a box, the corner it is laid out from. */
export function anchor(copy: Copy, rect: Rect, annotations: Annotations): Point {
  if (copy.parts.length === 0) return { x: rect.x, y: rect.y };
  const start = least(annotations, copy.node.name);
  return { x: rect.x - start.x, y: rect.y - start.y };
}

/** The inner corner of the box drawn as `box` for the whole `copy`: its parts' places are from
 * there. */
export function inner(copy: Copy, box: Rect, annotations: Annotations): Point {
  const start = least(annotations, copy.node.name);
  return { x: box.x + BOX.pad - start.x, y: box.y + header(roomOf(annotations), copy.node.name, box.width) - start.y };
}

const round = (at: Point): [number, number] => [Math.round(at.x), Math.round(at.y)];

/** `at` as a place inside the box of `whole`: from its inner corner, never above or left of it. */
export function inside(at: Point, whole: Copy, box: Rect, annotations: Annotations): [number, number] {
  const corner = inner(whole, box, annotations);
  return round({ x: Math.max(at.x - corner.x, 0), y: Math.max(at.y - corner.y, 0) });
}

/** The places to save for the level the copy `moved` is in, as shown, with `moved` at `to`: in a
 * box, every part's, from its inner corner; at the top level, the moved node's alone once the
 * top level is placed by hand, else every top-level node's. Keys are a node's name, or
 * `whole/part`. */
export function placesOf(drawn: Copy[], rects: Map<string, Rect>, annotations: Annotations, moved: Copy, to: Point): Record<string, [number, number]> {
  const byKey = new Map(drawn.map((copy) => [copy.key, copy]));
  const where = (copy: Copy) => (copy.key === moved.key ? to : anchor(copy, rects.get(copy.key)!, annotations));
  if (moved.parent !== null) {
    const whole = byKey.get(moved.parent)!;
    const box = rects.get(whole.key)!;
    const placed: Record<string, [number, number]> = {};
    for (const key of whole.parts) {
      const part = byKey.get(key);
      if (part && rects.has(key)) placed[`${whole.node.name}/${part.node.name}`] = inside(where(part), whole, box, annotations);
    }
    return placed;
  }
  if (annotations.at.size > 0) return { [moved.node.name]: round(to) };
  return Object.fromEntries(drawn.filter((copy) => copy.parent === null && rects.has(copy.key)).map((copy) => [copy.node.name, round(where(copy))]));
}

/** The places of the top level as shown, every node's but `but`'s: saved with a drop that puts
 * an Artifact at a top level still laid out automatically. */
export function topLevel(drawn: Copy[], rects: Map<string, Rect>, annotations: Annotations, but: string): Record<string, [number, number]> {
  return Object.fromEntries(
    drawn.filter((copy) => copy.parent === null && copy.node.name !== but && rects.has(copy.key)).map((copy) => [copy.node.name, round(anchor(copy, rects.get(copy.key)!, annotations))]),
  );
}

/** The places of the box `whole` as shown, every part's but `but`'s: saved with a drop that puts
 * an Artifact in a box still laid out automatically. */
export function boxPlaces(drawn: Copy[], rects: Map<string, Rect>, annotations: Annotations, whole: Copy, but: string): Record<string, [number, number]> {
  const byKey = new Map(drawn.map((copy) => [copy.key, copy]));
  const box = rects.get(whole.key)!;
  const placed: Record<string, [number, number]> = {};
  for (const key of whole.parts) {
    const part = byKey.get(key);
    if (part && part.node.name !== but && rects.has(key)) placed[`${whole.node.name}/${part.node.name}`] = inside(anchor(part, rects.get(key)!, annotations), whole, box, annotations);
  }
  return placed;
}
