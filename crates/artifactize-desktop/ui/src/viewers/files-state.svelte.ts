// The File Explorer's facts: each folder's entries once listed, the folders unfolded, and the
// file shown with its text. The files are the library's, read through the viewer session.
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { call, message, type ViewerFile } from "../ipc/client";
import type { ViewerState } from "./viewer-state.svelte";

export class FilesState {
  /** Each listed folder's entries, by its path (`""` the Artifact's own folder). */
  readonly entries = new SvelteMap<string, ViewerFile[]>();
  readonly unfolded = new SvelteSet<string>();
  /** The file shown, its text and the line pointed at. */
  shown = $state<string | null>(null);
  text = $state<string | null>(null);
  error = $state<string | null>(null);

  constructor(readonly viewer: ViewerState) {}

  /** List folder `path`, once. */
  async list(path: string): Promise<void> {
    if (this.entries.has(path)) return;
    try {
      const listed = await call(this.viewer.ipc.viewerFiles(this.viewer.session, path));
      // Folders first, then files, each by name.
      this.entries.set(path, listed.toSorted((a, b) => (a.kind === b.kind ? a.name.localeCompare(b.name) : a.kind === "directory" ? -1 : 1)));
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
    this.shown = path;
    this.text = null;
    try {
      const text = await call(this.viewer.ipc.viewerReadText(this.viewer.session, path));
      if (this.shown === path) this.text = text;
      this.error = null;
    } catch (failure) {
      if (this.shown === path) this.error = message(failure);
    }
  }
}
