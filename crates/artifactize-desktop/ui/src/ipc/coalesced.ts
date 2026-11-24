// A read that the backend's change events ask for, at most one at a time: events that arrive
// while it runs ask for exactly one more read once it ends, and events that arrive while what it
// feeds is not on screen only mark it stale, read once when it is shown again. A burst of events
// (a verify finishing fires many) thus costs at most two reads, and a hidden mode none.

export class Coalesced {
  #running = false;
  #again = false;
  #stale = false;

  /** `read` reads; `shown` says whether what it feeds is on screen now. */
  constructor(
    readonly read: () => Promise<unknown>,
    readonly shown: () => boolean = () => true,
  ) {}

  /** Something changed: read now, after the read in flight, or once shown again. */
  request(): void {
    if (!this.shown()) {
      this.#stale = true;
      return;
    }
    if (this.#running) {
      this.#again = true;
      return;
    }
    void this.#run();
  }

  /** What it feeds is on screen again: a change missed meanwhile is read now. */
  showing(): void {
    if (!this.#stale) return;
    this.#stale = false;
    this.request();
  }

  async #run(): Promise<void> {
    this.#running = true;
    try {
      do {
        this.#again = false;
        // A failed read says why where its caller shows it; the next change reads again.
        await this.read().catch(() => {});
      } while (this.#again && this.shown());
      if (this.#again) {
        this.#again = false;
        this.#stale = true;
      }
    } finally {
      this.#running = false;
    }
  }
}
