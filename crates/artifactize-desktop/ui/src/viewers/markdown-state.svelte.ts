// The Markdown viewer's facts: the Artifact's Markdown files, the one shown and its blocks. The
// files are the library's, read through the viewer session.
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { call, message, type ViewerFile } from "../ipc/client";
import { blocks } from "./markdown";
import type { ViewerState } from "./viewer-state.svelte";

export class MarkdownState {
  files = $state.raw<string[]>([]);
  tree = $state.raw(new SvelteMap<string, ViewerFile[]>());
  readonly unfolded = new SvelteSet<string>();

  toggle(path: string): void {
    if (!this.unfolded.delete(path)) this.unfolded.add(path);
  }
  #document = $state.raw<{ path: string; text: string } | null>(null);
  readonly shown = $derived(this.#document?.path ?? null);
  readonly text = $derived(this.#document?.text ?? null);
  #reads = 0;
  readonly read = $derived(this.text === null ? [] : blocks(this.text));
  error = $state<string | null>(null);

  constructor(readonly viewer: ViewerState) {}

  /** Find the Artifact's Markdown files and show the first; a file Artifact is shown alone. */
  async load(single: boolean): Promise<void> {
    if (single) return this.show("");
    const found: string[] = [];
    const tree = new SvelteMap<string, ViewerFile[]>();
    const walk = async (path: string): Promise<boolean> => {
      const entries = await call(this.viewer.ipc.viewerFiles(this.viewer.session, path));
      const visible: ViewerFile[] = [];
      for (const entry of entries.toSorted((a, b) => a.path.localeCompare(b.path))) {
        if (entry.kind === "file" && /\.(md|markdown)$/i.test(entry.name)) {
          found.push(entry.path);
          visible.push(entry);
        } else if (entry.kind === "directory") {
          const first = found.length === 0;
          if (await walk(entry.path)) {
            visible.push(entry);
            if (first) this.unfolded.add(entry.path);
          }
        }
      }
      tree.set(path, visible);
      return visible.length > 0;
    };
    try {
      await walk("");
      this.files = found;
      this.tree = tree;
      if (found.length > 0) await this.show(found[0]);
    } catch (failure) {
      this.error = message(failure);
    }
  }

  async show(path: string): Promise<void> {
    const read = ++this.#reads;
    try {
      const text = await call(this.viewer.ipc.viewerReadText(this.viewer.session, path));
      if (read !== this.#reads) return;
      this.#document = { path, text };
      this.error = null;
    } catch (failure) {
      if (read === this.#reads) this.error = message(failure);
    }
  }
}
