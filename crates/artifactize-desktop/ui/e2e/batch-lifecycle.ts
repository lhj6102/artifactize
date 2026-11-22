// One owner for batch startup, service shutdown and its foreground lease. Both the ordinary
// return and WDIO's asynchronous exit hook call the same idempotent close.
import { checkForeground, quarantineForeground, releaseForeground } from "./platform.ts";
import type { NativePath } from "./portable.ts";

type Stop = () => Promise<void>;

export class BatchLifecycle {
  readonly #stops = new Map<object, { stop: Stop; pending?: Promise<void> }>();
  readonly failures: unknown[] = [];
  #closing: Promise<boolean> | undefined;
  #marked = false;

  readonly marker: NativePath | null;
  readonly description: string;

  constructor(marker: NativePath | null, description: string) {
    checkForeground(marker);
    this.marker = marker;
    this.description = description;
  }

  /** Register before letting the service start anything. Initialization alone owns no app. */
  arm(service: object, stop: Stop): void {
    if (this.#closing) throw new Error("the batch closed before the service started");
    if (!this.#marked) {
      quarantineForeground(this.marker, this.description);
      this.#marked = true;
    }
    this.#stops.set(service, { stop });
  }

  async stop(service: object, preferred?: Stop): Promise<void> {
    const registered = this.#stops.get(service);
    if (!registered) return;
    if (registered.pending) return registered.pending;
    const stop = preferred ?? registered.stop;
    // Normal completion and a concurrent exit hook join the same attempt.
    registered.pending = Promise.resolve().then(stop).then(() => {
      this.#stops.delete(service);
    }, (error: unknown) => {
      registered.pending = undefined;
      this.failures.push(error);
      throw error;
    });
    return registered.pending;
  }

  close(): Promise<boolean> {
    return this.#closing ??= this.#finish();
  }

  async #finish(): Promise<boolean> {
    // WDIO can log and consume a failed stop. Retry it here; neither fixture deletion nor
    // releasing the foreground is allowed while the owner cannot confirm shutdown.
    for (const service of this.#stops.keys()) {
      try { await this.stop(service); } catch { /* retained in failures, with the lease */ }
    }
    if (this.#stops.size !== 0) return false;
    if (this.#marked) releaseForeground(this.marker);
    return true;
  }
}
