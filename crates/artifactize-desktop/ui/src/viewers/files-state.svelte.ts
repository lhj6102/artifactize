// The File Explorer's facts: each folder's entries once listed, the folders unfolded, and the
// file shown with its text. The files are the library's, read through the viewer session.
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { call, message, type ViewerFile } from "../ipc/client";
import type { ViewerState } from "./viewer-state.svelte";

export class FilesState {
  /** Each listed folder's entries, by its path (`""` the Artifact's own folder). */
  readonly #folders = new SvelteMap<string, ViewerFile[]>();
  readonly entries = $derived(new SvelteMap([...this.#folders].map(([path, listed]) => [path,
    listed.toSorted((a, b) => a.kind === b.kind ? a.name.localeCompare(b.name) : a.kind === "directory" ? -1 : 1),
  ])));
  readonly unfolded = new SvelteSet<string>();
  /** The file shown, its text and the line pointed at. */
  #document = $state.raw<{ path: string; text: string } | null>(null);
  readonly shown = $derived(this.#document?.path ?? null);
  readonly text = $derived(this.#document?.text ?? null);
  #reads = 0;
  error = $state<string | null>(null);

  constructor(readonly viewer: ViewerState) {}

  /** List folder `path`, once. */
  async list(path: string): Promise<void> {
    if (this.#folders.has(path)) return;
    try {
      const listed = await call(this.viewer.ipc.viewerFiles(this.viewer.session, path));
      this.#folders.set(path, listed);
    } catch (failure) {
      this.error = message(failure);
    }
  }

  /** Unfold folder `path`, listing it, or fold it again. */
  async toggle(path: string): Promise<void> {
    if (this.unfolded.has(path)) {
      this.unfolded.delete(path);
      return;
    }
    this.unfolded.add(path);
    await this.list(path);
  }

  /** Show file `path`'s text in the document view. */
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
