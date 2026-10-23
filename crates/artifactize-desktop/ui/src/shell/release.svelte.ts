// The header's newer-release notice: what the library's lookup found, and its dialog.
import { call, message, type Commands, type ReleaseNotice } from "../ipc/client";

/** When the library could not be asked: try again in an hour, as it would say. */
const FALLBACK_MS = 60 * 60 * 1000;

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

  /** Ask the library now and again whenever it says to, until the returned function is called.
   * It reuses a lookup made within the day; a failed call shows nothing and asks again later. */
  watch(): () => void {
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const look = async () => {
      let wait = FALLBACK_MS;
      try {
        const view = await call(this.#ipc.releaseNotice());
        this.notice = view.notice;
        wait = view.recheckMs;
      } catch {
        this.notice = null;
      }
      if (!stopped) timer = setTimeout(() => void look(), wait);
    };
    void look();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
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
