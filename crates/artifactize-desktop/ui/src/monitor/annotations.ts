// What a branch's definitions say about how its graph is shown, apart from what it is: each
// Artifact's place and its comment (the `[annotations]` of its definition file). Runs and the
// Current of a branch are drawn with them alike; only the Current edits them.
import type { DefinitionsView } from "../ipc/client";
import type { Point } from "./graph";

/** Places and comments by Artifact name: `at`, a node's own place at the top level; `places`,
 * a whole's places of its parts inside its box, by part; `comments`, each comment's text. */
export type Annotations = {
  at: ReadonlyMap<string, Point>;
  places: ReadonlyMap<string, ReadonlyMap<string, Point>>;
  comments: ReadonlyMap<string, string>;
};

export const NO_ANNOTATIONS: Annotations = { at: new Map(), places: new Map(), comments: new Map() };

const point = ([x, y]: [number | null, number | null]): Point | null => (x === null || y === null ? null : { x, y });

/** The annotations of `view`; none without one. A place or a comment its file cannot hold is
 * left out: that box is laid out automatically, that comment not drawn. */
export function annotationsOf(view: DefinitionsView | null): Annotations {
  if (!view) return NO_ANNOTATIONS;
  const at = new Map<string, Point>();
  const places = new Map<string, Map<string, Point>>();
  const comments = new Map<string, string>();
  for (const node of view.nodes) {
    if (node.styleError === null) {
      const own = node.at ? point(node.at) : null;
      if (own) at.set(node.name, own);
      const inside = new Map<string, Point>();
      for (const [part, place] of Object.entries(node.places)) {
        const found = place ? point(place) : null;
        if (found) inside.set(part, found);
      }
      if (inside.size > 0) places.set(node.name, inside);
    }
    if (node.commentError === null && node.comment) comments.set(node.name, node.comment);
  }
  return { at, places, comments };
}

/** A text key of `annotations`: a layout is reused only for the same places and comments. */
export function annotationsKey(annotations: Annotations): string {
  const places = [...annotations.places].map(([whole, inside]) => [whole, [...inside].map(([part, at]) => [part, at.x, at.y])]);
  return JSON.stringify([[...annotations.at].map(([name, at]) => [name, at.x, at.y]), places, [...annotations.comments]]);
}

/** A comment's room under what it is under: a gap, then at most `lines` lines of `line` px;
 * `char` is an average character's width at the comment's size, to guess the lines. */
export const COMMENT = { gap: 4, line: 17, lines: 3, char: 6.6 };

/** How much room the comment `text` takes under something `width` wide: none without one. */
export function commentHeight(text: string | undefined, width: number): number {
  if (!text) return 0;
  const perLine = Math.max(1, Math.floor(width / COMMENT.char));
  let lines = 0;
  for (const paragraph of text.split("\n")) {
    // Words wrap at spaces: a line holds whole words while they fit.
    let used = 0;
    let count = 1;
    for (const word of paragraph.split(" ")) {
      if (used > 0 && used + 1 + word.length > perLine) {
        count += 1;
        used = 0;
      }
      used += used === 0 ? word.length : word.length + 1;
      // A word longer than a line breaks inside it.
      while (used > perLine) {
        count += 1;
        used -= perLine;
      }
    }
    lines += count;
    if (lines >= COMMENT.lines) break;
  }
  return COMMENT.gap + Math.min(lines, COMMENT.lines) * COMMENT.line;
}
