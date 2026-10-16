// The monitor's snapshot as the frontend holds it. The backend reads the database; this only
// asks for views, coalesces change hints and drops responses that arrive out of order.
import { call, commands, message, type Commands } from "../ipc/client";
import type { BrowseView, Navigate, RepositoryKey } from "../ipc/client";

const still: Navigate = { scope: null, run: null, paging: null, full: false, resync: false };

export class Browse {
  constructor(readonly ipc: Commands = commands) {}

  view = $state.raw<BrowseView | null>(null);
  error = $state<string | null>(null);
  /** The backend snapshot's revision: views derived from it (Detail) read again when it grows. */
  readonly revision = $derived(this.view?.revision ?? 0);
  #listeners: ((view: BrowseView) => void)[] = [];
  #draining = false;
  #dirty = false;
  #resync = false;

  /** Call `listener` with every snapshot as it is accepted, until the returned function
   * removes it: the views that follow a snapshot (selection, folds, an open Detail) update
   * with it, in the same turn. */
  onShown(listener: (view: BrowseView) => void): () => void {
    this.#listeners.push(listener);
    return () => {
      this.#listeners = this.#listeners.filter((entry) => entry !== listener);
    };
  }

  /** Navigate and read: selection changes reuse the cached catalog, refreshes read it. */
  async navigate(step: Partial<Navigate>): Promise<void> {
    try {
      this.#show(await call(this.ipc.browse({ ...still, ...step })));
      this.error = null;
    } catch (error) {
      this.error = message(error);
    }
  }

  /** Manual refresh drops all caches; state hints pass their explicit resync flag.
   * One read is in flight, and later refreshes coalesce without losing a reset. */
  refresh(resync = true): void {
    this.#dirty = true;
    this.#resync ||= resync;
    void this.#drain();
  }

  async #drain(): Promise<void> {
    if (this.#draining) return;
    this.#draining = true;
    try {
      while (this.#dirty) {
        const resync = this.#resync;
        this.#dirty = false;
        this.#resync = false;
        await this.navigate({ full: true, resync });
      }
    } finally {
      this.#draining = false;
    }
  }

  /** Fold or unfold a worktree group of the scope list: the library answers with the snapshot. */
  async toggleWorktrees(fold: RepositoryKey): Promise<void> {
    this.#show(await this.ipc.toggleWorktrees(fold));
  }

  /** Redraw ages and elapsed times from the backend's snapshot; nothing is read. */
  async redraw(): Promise<void> {
    if (!this.view) return;
    this.#show(await this.ipc.redraw());
  }

  /** Responses may arrive in any order; one older than the shown snapshot is dropped. A
   * redraw that already carries a newer read's snapshot advances the revision too. */
  #show(view: BrowseView): void {
    if (this.view && view.revision < this.view.revision) return;
    this.view = view;
    for (const listener of this.#listeners) listener(view);
  }
}
