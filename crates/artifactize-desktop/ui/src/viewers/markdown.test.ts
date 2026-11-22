// Markdown read into the blocks the viewer draws, each with the line it starts on.
import { describe, expect, it } from "vitest";
import { blocks, inline } from "./markdown";
import { position } from "./point";

describe("blocks", () => {
  it("reads headings, paragraphs, lists, code and tables with their first lines", () => {
    const read = blocks("# Install\n\nRun the `installer`,\nthen check.\n\n- one\n- **two**\n\n```\nartifactize --version\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n");
    expect(read.map((block) => [block.kind, block.line])).toEqual([
      ["heading", 1],
      ["paragraph", 3],
      ["list", 6],
      ["code", 9],
      ["table", 13],
    ]);
    expect(read[1]).toMatchObject({ lines: [[{ kind: "text", text: "Run the " }, { kind: "code", text: "installer" }, { kind: "text", text: "," }], [{ kind: "text", text: "then check." }]] });
    expect(read[2]).toMatchObject({ items: [{ line: 6 }, { line: 7, spans: [{ kind: "strong", text: "two" }] }] });
    expect(read[3]).toMatchObject({ text: "artifactize --version" });
  });

  it("keeps web link destinations without turning raw markup into HTML", () => {
    expect(inline("see [the docs](https://example.com) <b>now</b>")).toEqual([
      { kind: "text", text: "see " },
      { kind: "link", text: "the docs", url: "https://example.com" },
      { kind: "text", text: " <b>now</b>" },
    ]);
  });
});

describe("position", () => {
  it("says a place as its line and column, led by the file when the Artifact has several", () => {
    expect(position(null, 42, 9)).toBe("L42:C9");
    expect(position("install.md", 12, 3)).toBe("install.md L12:C3");
  });
});
