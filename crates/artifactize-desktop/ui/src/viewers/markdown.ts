// Markdown read into blocks the Markdown viewer draws (J-VIEWER-MARKDOWN): headings, paragraphs,
// lists, code blocks, tables and rules, each with the line it starts on, so Point can say where a
// click landed. Inline text becomes spans: code, strong, emphasis and links. Nothing is HTML: the
// viewer draws every span as text, so no Markdown runs as markup.

export type Span = { kind: "text" | "code" | "strong" | "em"; text: string } | { kind: "link"; text: string; url: string };
export type Block =
  | { kind: "heading"; level: number; spans: Span[]; line: number }
  | { kind: "paragraph"; lines: Span[][]; line: number }
  | { kind: "list"; ordered: boolean; start?: number; items: { spans: Span[]; line: number }[]; line: number }
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

const fence = (text: string) => /^ {0,3}(`{3,}|~{3,})(.*)$/.exec(text);
const atx = (text: string) => /^ {0,3}(#{1,6})(?:[ \t]+|$)(.*)$/.exec(text);
const underline = (text: string) => /^ {0,3}(=+|-+)[ \t]*$/.exec(text);
const marker = (text: string) => /^ {0,3}(?:([-*+])|(\d{1,9})([.)]))[ \t]+(.*)$/.exec(text);
const rule = (text: string) => /^(?: {0,3})(?:(?:-[ \t]*){3,}|(?:\*[ \t]*){3,}|(?:_[ \t]*){3,})$/.test(text);

/** `source`'s blocks, in order. */
export function blocks(source: string): Block[] {
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  const out: Block[] = [];
  let at = 0;
  const line = () => at + 1;
  while (at < lines.length) {
    const text = lines[at];
    const start = line();
    const opened = fence(text);
    const hashes = atx(text);
    const item = marker(text);
    const setext = at + 1 < lines.length && underline(lines[at + 1]);
    if (text.trim() === "") {
      at += 1;
    } else if (opened) {
      const body: string[] = [];
      const close = new RegExp(`^ {0,3}${opened[1][0]}{${opened[1].length},}[ \t]*$`);
      at += 1;
      while (at < lines.length && !close.test(lines[at])) body.push(lines[at++]);
      if (at < lines.length) at += 1;
      out.push({ kind: "code", text: body.join("\n"), line: start });
    } else if (hashes) {
      out.push({ kind: "heading", level: hashes[1].length, spans: inline(hashes[2].replace(/[ \t]+#+[ \t]*$/, "").trim()), line: start });
      at += 1;
    } else if (rule(text)) {
      out.push({ kind: "rule", line: start });
      at += 1;
    } else if (item) {
      const ordered = item[2] !== undefined;
      const items: { spans: Span[]; line: number }[] = [];
      for (let next: RegExpExecArray | null = item; next && !rule(lines[at]) && (next[2] !== undefined) === ordered && next[1] === item[1] && next[3] === item[3]; next = marker(lines[at] ?? "")) {
        items.push({ spans: inline(next[4]), line: line() });
        at += 1;
      }
      out.push({ kind: "list", ordered, ...(ordered ? { start: Number(item[2]) } : {}), items, line: start });
    } else if (/^(?: {4}|\t)/.test(text)) {
      const body: string[] = [];
      while (at < lines.length && /^(?: {4}|\t)/.test(lines[at])) body.push(lines[at++].replace(/^(?: {4}|\t)/, ""));
      out.push({ kind: "code", text: body.join("\n"), line: start - 1 });
    } else if (setext) {
      out.push({ kind: "heading", level: setext[1][0] === "=" ? 1 : 2, spans: inline(text.trim()), line: start });
      at += 2;
    } else if (text.trim().startsWith("|") && at + 1 < lines.length && /^\s*\|?[\s:-]+\|/.test(lines[at + 1])) {
      const head = cells(text);
      at += 2;
      const rows: Span[][][] = [];
      while (at < lines.length && lines[at].trim().startsWith("|")) rows.push(cells(lines[at++]));
      out.push({ kind: "table", head, rows, line: start });
    } else {
      const body: string[] = [text.trim()];
      at += 1;
      while (at < lines.length && lines[at].trim() !== "" && !atx(lines[at]) && !fence(lines[at]) && !marker(lines[at]) && !rule(lines[at])) {
        if (at + 1 < lines.length && underline(lines[at + 1])) break;
        body.push(lines[at++].trim());
      }
      out.push({ kind: "paragraph", lines: body.map(inline), line: start });
    }
  }
  return out;
}
