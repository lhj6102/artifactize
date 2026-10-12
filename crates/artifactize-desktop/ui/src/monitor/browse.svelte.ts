// The monitor's snapshot as the frontend holds it. The backend reads the database; this only
// asks for views, coalesces change hints and drops responses that arrive out of order.
import { call, commands, message } from "../ipc/client";
import type { BrowseView, Navigate } from "../ipc/client";

const still: Navigate = { scope: null, run: null, paging: null, full: false, resync: false };

export class Browse {
  view = $state<BrowseView | null>(null);
  error = $state<string | null>(null);
  /** Counts accepted snapshots, so views derived from one read again. */
  revision = $state(0);
  #asked = 0;
  #shown = 0;
  #draining = false;
  #dirty = false;
  #resync = false;

  /** Navigate and read: selection changes reuse the cached catalog, refreshes read it. */
  async navigate(step: Partial<Navigate>): Promise<void> {
    const asked = ++this.#asked;
    try {
      this.#show(asked, await call(commands.browse({ ...still, ...step })));
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
    const asked = ++this.#asked;
    this.#show(asked, await commands.redraw(), false);
  }

  #show(asked: number, view: BrowseView, read = true): void {
    if (asked < this.#shown) return;
    this.#shown = asked;
    this.view = view;
    if (read) {
      this.error = null;
      this.revision += 1;
    }
  }
}
