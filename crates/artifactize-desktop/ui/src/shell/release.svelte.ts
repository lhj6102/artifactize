// The header's newer-release notice: what the library's lookup found, and its dialog.
import { call, message, type Commands, type ReleaseNotice } from "../ipc/client";

export class Release {
  /** The newer release, replaced whole; null while none is out. */
  notice = $state.raw<ReleaseNotice | null>(null);
  /** The dialog with how to update. */
  open = $state(false);
  /** Why the last link could not be opened. */
  failure = $state<string | null>(null);
  readonly #ipc: Commands;

  constructor(ipc: Commands) {
    this.#ipc = ipc;
  }

  /** Ask the library again; it reuses a lookup made within the day. A failed lookup shows nothing. */
  async look(): Promise<void> {
    try {
      this.notice = await call(this.#ipc.releaseNotice());
    } catch {
      this.notice = null;
    }
  }

  /** Open the release's download or page in the browser. */
  async follow(): Promise<void> {
    this.failure = null;
    try {
      await call(this.#ipc.releaseOpen());
    } catch (error) {
      this.failure = message(error);
    }
  }
}
