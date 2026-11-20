// The Markdown viewer's facts: the Artifact's Markdown files, the one shown and its blocks. The
// files are the library's, read through the viewer session.
import { call, message } from "../ipc/client";
import { blocks, type Block } from "./markdown";
import type { ViewerState } from "./viewer-state.svelte";

/** How deep into an Artifact's folders its Markdown files are looked for. */
const DEPTH = 4;

export class MarkdownState {
  files = $state.raw<string[]>([]);
  shown = $state<string | null>(null);
  read = $state.raw<Block[]>([]);
  error = $state<string | null>(null);

  constructor(readonly viewer: ViewerState) {}

  /** Find the Artifact's Markdown files and show the first; a file Artifact is shown alone. */
  async load(single: boolean): Promise<void> {
    if (single) return this.show("");
    const found: string[] = [];
    const walk = async (path: string, depth: number) => {
      const entries = await call(this.viewer.ipc.viewerFiles(this.viewer.session, path));
      for (const entry of entries.toSorted((a, b) => a.path.localeCompare(b.path))) {
        if (entry.kind === "file" && /\.(md|markdown)$/i.test(entry.name)) found.push(entry.path);
        else if (entry.kind === "directory" && depth < DEPTH) await walk(entry.path, depth + 1);
      }
    };
    try {
      await walk("", 0);
      this.files = found;
      if (found.length > 0) await this.show(found[0]);
    } catch (failure) {
      this.error = message(failure);
    }
  }

  async show(path: string): Promise<void> {
    this.shown = path;
    try {
      const text = await call(this.viewer.ipc.viewerReadText(this.viewer.session, path));
      if (this.shown === path) this.read = blocks(text);
      this.error = null;
    } catch (failure) {
      if (this.shown === path) this.error = message(failure);
    }
  }
}
