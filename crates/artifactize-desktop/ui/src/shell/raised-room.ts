import type { Attachment } from "svelte/attachments";

/** Keep a locally positioned raised surface inside its actual scroll/clip ancestors. */
export function raisedRoom(edge: "above" | "below"): Attachment<HTMLElement> {
  return (surface) => {
    const anchor = surface.parentElement;
    if (!anchor) return;
    const clips: { node: HTMLElement; x: boolean; y: boolean }[] = [];
    for (let node = anchor.parentElement; node; node = node.parentElement) {
      const { overflowX, overflowY } = getComputedStyle(node);
      if (overflowX !== "visible" || overflowY !== "visible") clips.push({ node, x: overflowX !== "visible", y: overflowY !== "visible" });
    }
    const measure = () => {
      const anchorRect = anchor.getBoundingClientRect();
      let top = 0, right = window.innerWidth, bottom = window.innerHeight;
      for (const { node, x, y } of clips) {
        const rect = node.getBoundingClientRect();
        const innerTop = rect.top + node.clientTop;
        if (x) right = Math.min(right, rect.left + node.clientLeft + node.clientWidth);
        if (y) {
          top = Math.max(top, innerTop);
          bottom = Math.min(bottom, innerTop + node.clientHeight);
        }
      }
      const height = edge === "above" ? anchorRect.top - top : bottom - anchorRect.bottom;
      surface.style.setProperty("--raised-height", `${Math.max(0, height - 4)}px`);
      surface.style.setProperty("--raised-width", `${Math.max(0, right - anchorRect.left - 4)}px`);
    };
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    observer?.observe(anchor);
    for (const { node } of clips) observer?.observe(node);
    measure();
    window.addEventListener("resize", measure);
    window.addEventListener("scroll", measure, true);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", measure);
      window.removeEventListener("scroll", measure, true);
    };
  };
}
