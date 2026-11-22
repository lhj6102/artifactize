// Chat supports a small Markdown vocabulary. The parser produces text and typed nodes only;
// unsupported syntax is not turned into HTML, including literal markup and images.
export type Inline = { kind: "text" | "code"; text: string } | { kind: "link"; text: string; url: string };
export type Block =
  | { kind: "paragraph"; parts: Inline[] }
  | { kind: "code"; text: string }
  | { kind: "list"; start: number | null; items: Block[][] };

function webUrl(text: string): boolean {
  if (!/^https?:\/\//i.test(text)) return false;
  try {
    return new URL(text).hostname !== "";
  } catch {
    return false;
  }
}

function inline(text: string): Inline[] {
  const parts: Inline[] = [];
  const tokens = /\\([!"#$%&'()*+,\-./:;<=>?@[\]\\^_`{|}~])|(?<!`)(`+)(?!`)([^`\n]+(?:`[^`\n]+)*?)\2(?!`)|(!?)\[([^\[\]\n]*)\]\((<?)([^\s()<>]*(?:\([^\s()<>]*\)[^\s()<>]*)*)(>?)(?:[ \t]+(?:"[^"\n]*"|'[^'\n]*'))?\)|<(https?:\/\/[^\s<>]+)>|<!--[\s\S]*?(?:-->|$)|<\/?[A-Za-z][^<>]*>/gi;
  let at = 0;
  for (const match of text.matchAll(tokens)) {
    const [whole, escaped, ticks, code, image, label, left, target, right, auto] = match;
    if (at < match.index) parts.push({ kind: "text", text: text.slice(at, match.index) });
    at = match.index + whole.length;
    if (escaped !== undefined) parts.push({ kind: "text", text: escaped });
    else if (ticks !== undefined) {
      const value = code.startsWith(" ") && code.endsWith(" ") && /\S/.test(code) ? code.slice(1, -1) : code;
      parts.push({ kind: "code", text: value });
    } else if (auto && webUrl(auto)) parts.push({ kind: "link", text: auto, url: auto });
    else if (image === "" && Boolean(left) === Boolean(right) && webUrl(target)) parts.push({ kind: "link", text: label, url: target });
    else parts.push({ kind: "text", text: whole });
  }
  if (at < text.length) parts.push({ kind: "text", text: text.slice(at) });
  return parts;
}

const fence = /^ {0,3}(`{3,}|~{3,})(.*)$/;
const marker = /^( {0,3})([-+*]|\d{1,9}[.)])([ \t]+)(.*)$/;
const indented = /^(?: {4}|\t)/;
const literal = /^ {0,3}(?:#{1,6}(?:\s|$)|>|(?:[-*_]\s*){3,}$|<\/?[a-z][\w-]*(?:\s|\/?>)|<!--)/i;

function blocks(lines: string[], depth: number): Block[] {
  const result: Block[] = [];
  for (let i = 0; i < lines.length;) {
    const line = lines[i];
    if (!line.trim()) { i++; continue; }
    const opening = fence.exec(line)?.[1];
    if (opening) {
      const closing = new RegExp(`^ {0,3}${opening[0]}{${opening.length},}\\s*$`);
      const body: string[] = [];
      while (++i < lines.length && !closing.test(lines[i])) body.push(lines[i]);
      i++;
      result.push({ kind: "code", text: body.join("\n") });
      continue;
    }
    if (indented.test(line)) {
      const body: string[] = [];
      while (i < lines.length && indented.test(lines[i])) body.push(lines[i++].replace(indented, ""));
      result.push({ kind: "code", text: body.join("\n") });
      continue;
    }
    const first = marker.exec(line);
    if (!literal.test(line) && first && depth < 16) {
      const ordered = first[2].length > 1;
      const list: Extract<Block, { kind: "list" }> = { kind: "list", start: ordered ? parseInt(first[2], 10) : null, items: [] };
      let item: RegExpExecArray | null = first;
      while (item && (item[2].length > 1) === ordered && !literal.test(lines[i])) {
        const content = [item[4]];
        const prefix = " ".repeat(item[1].length + item[2].length + item[3].length);
        for (i++; i < lines.length; i++) {
          if (lines[i].startsWith(prefix)) content.push(lines[i].slice(prefix.length));
          else if (!lines[i].trim()) content.push("");
          else break;
        }
        list.items.push(blocks(content, depth + 1));
        item = i < lines.length ? marker.exec(lines[i]) : null;
      }
      result.push(list);
      continue;
    }
    const para = [line];
    const plain = literal.test(line);
    i++;
    // Raw HTML at the start of a paragraph stays literal as a block, not just its first tag.
    const html = /^ {0,3}</.test(line) && plain;
    while (i < lines.length && lines[i].trim() && (html || (!plain && !fence.test(lines[i]) && !marker.test(lines[i]) && !literal.test(lines[i])))) para.push(lines[i++]);
    const text = para.join("\n");
    result.push({ kind: "paragraph", parts: plain ? [{ kind: "text", text }] : inline(text) });
  }
  return result;
}

export const markdown = (text: string): Block[] => blocks(text.replace(/\r\n?/g, "\n").split("\n"), 0);
