// Markdown read into blocks the Markdown viewer draws (J-VIEWER-MARKDOWN): headings, paragraphs,
// lists, code blocks, tables and rules, each with the line it starts on, so Point can say where a
// click landed. Inline text becomes spans: code, strong, emphasis and links. Nothing is HTML: the
// viewer draws every span as text, so no Markdown runs as markup.

export type Span = { kind: "text" | "code" | "strong" | "em"; text: string } | { kind: "link"; text: string; url: string };
export type Block =
  | { kind: "heading"; level: number; spans: Span[]; line: number }
  | { kind: "paragraph"; lines: Span[][]; line: number }
  | { kind: "list"; ordered: boolean; items: { spans: Span[]; line: number }[]; line: number }
  | { kind: "code"; text: string; line: number }
  | { kind: "table"; head: Span[][]; rows: Span[][][]; line: number }
  | { kind: "rule"; line: number };

const INLINE = /(`[^`]+`)|(\*\*[^*]+\*\*|__[^_]+__)|(\*[^*\s][^*]*\*|_[^_\s][^_]*_)|(\[[^\]]+\]\([^)]*\))/g;

/** `text`'s inline spans. */
export function inline(text: string): Span[] {
  const spans: Span[] = [];
  let at = 0;
  for (const match of text.matchAll(INLINE)) {
    if (match.index > at) spans.push({ kind: "text", text: text.slice(at, match.index) });
    const [whole, code, strong, em, link] = match;
    if (code) spans.push({ kind: "code", text: code.slice(1, -1) });
    else if (strong) spans.push({ kind: "strong", text: strong.slice(2, -2) });
    else if (em) spans.push({ kind: "em", text: em.slice(1, -1) });
    else if (link) {
      const split = link.indexOf("](");
      const url = link.slice(split + 2, -1).trim();
      const label = link.slice(1, split);
      // Only external web links can leave this window; other destinations remain text.
      spans.push(/^https?:\/\//i.test(url) ? { kind: "link", text: label, url } : { kind: "text", text: label });
    }
    at = match.index + whole.length;
  }
  if (at < text.length) spans.push({ kind: "text", text: text.slice(at) });
  return spans;
}

const cells = (row: string) =>
  row
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((cell) => inline(cell.trim()));

/** `source`'s blocks, in order. */
export function blocks(source: string): Block[] {
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  const out: Block[] = [];
  let at = 0;
  const line = () => at + 1;
  while (at < lines.length) {
    const text = lines[at];
    const start = line();
    if (text.trim() === "") {
      at += 1;
    } else if (/^```/.test(text.trim())) {
      const body: string[] = [];
      at += 1;
      while (at < lines.length && !/^```/.test(lines[at].trim())) body.push(lines[at++]);
      at += 1;
      out.push({ kind: "code", text: body.join("\n"), line: start });
    } else if (/^#{1,6}\s/.test(text)) {
      const level = text.match(/^#+/)![0].length;
      out.push({ kind: "heading", level, spans: inline(text.slice(level).trim()), line: start });
      at += 1;
    } else if (/^(\s*[-*_]){3,}\s*$/.test(text)) {
      out.push({ kind: "rule", line: start });
      at += 1;
    } else if (/^\s*([-*+]|\d+[.)])\s+/.test(text)) {
      const ordered = /^\s*\d/.test(text);
      const items: { spans: Span[]; line: number }[] = [];
      while (at < lines.length && /^\s*([-*+]|\d+[.)])\s+/.test(lines[at])) {
        items.push({ spans: inline(lines[at].replace(/^\s*([-*+]|\d+[.)])\s+/, "")), line: line() });
        at += 1;
      }
      out.push({ kind: "list", ordered, items, line: start });
    } else if (text.trim().startsWith("|") && at + 1 < lines.length && /^\s*\|?[\s:-]+\|/.test(lines[at + 1])) {
      const head = cells(text);
      at += 2;
      const rows: Span[][][] = [];
      while (at < lines.length && lines[at].trim().startsWith("|")) rows.push(cells(lines[at++]));
      out.push({ kind: "table", head, rows, line: start });
    } else {
      const body: string[] = [];
      while (at < lines.length && lines[at].trim() !== "" && !/^(#{1,6}\s|```|\s*([-*+]|\d+[.)])\s+)/.test(lines[at])) body.push(lines[at++].trim());
      out.push({ kind: "paragraph", lines: body.map(inline), line: start });
    }
  }
  return out;
}
