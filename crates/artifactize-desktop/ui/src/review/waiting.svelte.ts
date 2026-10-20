// How many Human reviews wait, as the Review list's starting search finds them. The library
// counts; this holds the newest answer and, while it is shown, reads it again on every change
// of the state.
import { createSubscriber } from "svelte/reactivity";
import { call, commands, onStateChanged, type Commands } from "../ipc/client";

export class Waiting {
  constructor(readonly ipc: Commands = commands) {}

  #count = $state(0);
  /** Each read supersedes the ones before it, so a slow answer never shows an older count. */
  #reads = 0;
  /** While the count is shown: read it once, then again on every state change. */
  readonly #live = createSubscriber(() => {
    void this.refresh();
    return onStateChanged(() => void this.refresh());
  });

  /** The number of waiting Human reviews; showing it keeps it live. */
  get count(): number {
    this.#live();
    return this.#count;
  }

  /** Read the count again. A failed read keeps the last count: the Review list itself shows
   * why the state cannot be read. */
  async refresh(): Promise<void> {
    const read = ++this.#reads;
    try {
      const count = await call(this.ipc.reviewWaiting());
      if (read === this.#reads) this.#count = count;
    } catch {
      // Kept as it was; see above.
    }
  }
}
