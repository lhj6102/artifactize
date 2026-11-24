// Find in a conversation (J-CHAT-FIND, J-REVIEW-BACK): every place its text appears, marked
// through the CSS highlights so the conversation's own nodes stay as drawn.

/** Every place in `root`'s text where `text` (lower case) appears, ignoring case. */
export function places(root: HTMLElement, text: string): Range[] {
  const ranges: Range[] = [];
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const value = (node.nodeValue ?? "").toLowerCase();
    for (let at = value.indexOf(text); at !== -1; at = value.indexOf(text, at + text.length)) {
      const range = document.createRange();
      range.setStart(node, at);
      range.setEnd(node, at + text.length);
      ranges.push(range);
    }
  }
  return ranges;
}

/** Mark the places of `text` in `root`, the `at`th of them as the shown one, brought into view
 * when `shown` (the last place brought into view) differs; how many there are and the place now
 * shown, and a cleanup that takes the marks away. */
export function mark(root: HTMLElement, text: string, at: number, shown: string): { found: number; shown: string; clear: () => void } {
  const highlights = typeof CSS !== "undefined" && "highlights" in CSS ? CSS.highlights : null;
  const wanted = text.trim().toLowerCase();
  if (wanted === "") return { found: 0, shown: "", clear: () => {} };
  const ranges = places(root, wanted);
  if (ranges.length === 0) return { found: 0, shown, clear: () => {} };
  const place = ((at % ranges.length) + ranges.length) % ranges.length;
  if (highlights) {
    highlights.set("chat-find", new Highlight(...ranges));
    highlights.set("chat-find-current", new Highlight(ranges[place]));
  }
  const key = `${wanted}#${place}`;
  if (key !== shown) ranges[place].startContainer.parentElement?.scrollIntoView?.({ block: "center" });
  return {
    found: ranges.length,
    shown: key,
    clear: () => {
      highlights?.delete("chat-find");
      highlights?.delete("chat-find-current");
    },
  };
}
