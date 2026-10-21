// The live branches, the waiting reviews and the shown Run as the frontend holds them. The
// backend reads the database and Git; this only asks for views, coalesces change hints and
// drops responses that arrive out of order.
import { call, commands, message, type Commands } from "../ipc/client";
import type { BranchKey, BrowseView, Completion, Navigate } from "../ipc/client";

const still: Navigate = { run: null, close: false, query: null, older: null, star: null, full: false, resync: false };

export class Browse {
  constructor(readonly ipc: Commands = commands) {}

  view = $state.raw<BrowseView | null>(null);
  error = $state<string | null>(null);
  /** The backend snapshot's revision: views derived from it (Detail) read again when it grows. */
  readonly revision = $derived(this.view?.revision ?? 0);
  /** The Pane's search as the library applied it; a snapshot with the same text leaves views of
   * it as they are. */
  readonly query = $derived(this.view?.query.text ?? "sort:recent");
  #listeners: ((view: BrowseView) => void)[] = [];
  #draining = false;
  #dirty = false;
  #resync = false;

  /** Call `listener` with every snapshot as it is accepted, until the returned function
   * removes it: the views that follow a snapshot (selection, an open Detail) update with it,
   * in the same turn. */
  onShown(listener: (view: BrowseView) => void): () => void {
    this.#listeners.push(listener);
    return () => {
      this.#listeners = this.#listeners.filter((entry) => entry !== listener);
    };
  }

  /** Navigate and read: selection changes reuse the cached reviews, refreshes read them. */
  async navigate(step: Partial<Navigate>): Promise<boolean> {
    try {
      this.#show(await call(this.ipc.browse({ ...still, ...step })));
      this.error = null;
      return true;
    } catch (error) {
      this.error = message(error);
      return false;
    }
  }

  /** Show Run `run` in the Main area. */
  open(run: string): Promise<boolean> {
    return this.navigate({ run });
  }

  /** Show no Run: the Main area shows a Current, a review or Settings. */
  close(): Promise<boolean> {
    return this.navigate({ close: true });
  }

  /** Search the Pane with `query`, as typed. */
  search(query: string): Promise<boolean> {
    return this.navigate({ query });
  }

  /** Reach further back in `branch`'s Runs. */
  older(branch: BranchKey): Promise<boolean> {
    return this.navigate({ older: branch });
  }

  /** Star `branch`, or take its star away: starred branches are listed first. */
  star(branch: BranchKey, on: boolean): Promise<boolean> {
    return this.navigate({ star: { branch, on } });
  }

  /** Candidates for the last term of the Pane's search. */
  complete(query: string): Promise<Completion> {
    return this.ipc.paneComplete(query);
  }

  /** A manual refresh: every cache dropped, the branches scanned again. Whether it read. */
  reload(): Promise<boolean> {
    return this.navigate({ full: true, resync: true });
  }

  /** State hints pass their explicit resync flag. One read is in flight, and later hints
   * coalesce without losing a reset. */
  refresh(resync = false): void {
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
