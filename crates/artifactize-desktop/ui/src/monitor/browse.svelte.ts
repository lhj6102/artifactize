// The monitor's snapshot as the frontend holds it. The backend reads the database; this only
// asks for views, coalesces change hints and drops responses that arrive out of order.
import { call, commands, message } from "../ipc/client";
import type { BrowseView, Navigate } from "../ipc/client";

const still: Navigate = { scope: null, run: null, paging: null, full: false, resync: false };

export class Browse {
  view = $state<BrowseView | null>(null);
  error = $state<string | null>(null);
  /** The backend snapshot's revision: views derived from it (Detail) read again when it grows. */
  revision = $state(0);
  #draining = false;
  #dirty = false;
  #resync = false;

  /** Navigate and read: selection changes reuse the cached catalog, refreshes read it. */
  async navigate(step: Partial<Navigate>): Promise<void> {
    try {
      this.#show(await call(commands.browse({ ...still, ...step })));
      this.error = null;
    } catch (error) {
      this.error = message(error);
    }
  }

  /** A refresh (`r`, a change hint, a resync): one read in flight, later ones coalesce. */
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
    this.#show(await commands.redraw());
  }

  /** Responses may arrive in any order; one older than the shown snapshot is dropped. A
   * redraw that already carries a newer read's snapshot advances the revision too. */
  #show(view: BrowseView): void {
    if (this.view && view.revision < this.view.revision) return;
    this.view = view;
    this.revision = view.revision;
  }
}
