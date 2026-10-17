// A box floating beside the element it belongs to: an Artifact's eval popup beside its node,
// a summary beside the node it describes. The box is absolutely positioned in its offset
// parent and follows its anchor every frame, so camera moves and redraws never leave it behind.
import type { Attachment } from "svelte/attachments";

/** The room kept between an anchor and its box, and between a box and its parent's edges. */
const GAP = 8;

type Box = { left: number; top: number; right: number; bottom: number };

/**
 * Where a `width` × `height` box goes beside `anchor` inside `area`: to its right, else to its
 * left, else below it, else above it; always inside `area`. Coordinates are the area's.
 */
export function place(anchor: Box, area: Box, width: number, height: number): { x: number; y: number } {
  const clamp = (value: number, low: number, high: number) => Math.max(low, Math.min(value, Math.max(low, high)));
  const across = (start: number) => ({
    x: start - area.left,
    y: clamp(anchor.top, area.top + GAP, area.bottom - GAP - height) - area.top,
  });
  const along = (top: number) => ({
    x: clamp(anchor.left, area.left + GAP, area.right - GAP - width) - area.left,
    y: top - area.top,
  });
  if (anchor.right + GAP + width <= area.right - GAP) return across(anchor.right + GAP);
  if (anchor.left - GAP - width >= area.left + GAP) return across(anchor.left - GAP - width);
  if (anchor.bottom + GAP + height <= area.bottom - GAP) return along(anchor.bottom + GAP);
  return along(clamp(anchor.top - GAP - height, area.top + GAP, area.bottom - GAP - height));
}

/** Keep the box beside the element `selector` finds in the box's offset parent; while there is
 * none (a node outside the drawn part of the graph), the box hides. */
export function beside(selector: string): Attachment<HTMLElement> {
  return (box) => {
    let frame = 0;
    const follow = () => {
      // An unlaid-out page (no offset parent) still finds the anchor beside the box.
      const parent = box.offsetParent ?? box.parentElement;
      const anchor = parent?.querySelector(selector);
      if (!parent || !anchor) box.style.visibility = "hidden";
      else {
        const { x, y } = place(anchor.getBoundingClientRect(), parent.getBoundingClientRect(), box.offsetWidth, box.offsetHeight);
        box.style.left = `${x}px`;
        box.style.top = `${y}px`;
        box.style.visibility = "visible";
      }
      frame = requestAnimationFrame(follow);
    };
    follow();
    return () => cancelAnimationFrame(frame);
  };
}
