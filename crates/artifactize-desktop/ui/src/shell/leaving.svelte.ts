// Leaving the window with claims the standalone review took: every way out (q, the window's
// close button, repeated close requests) asks once and waits for the answer; only the
// prompt's keep or release ends the window.
import { commands } from "../ipc/client";

export class Leaving {
  /** The claims the open prompt asks about; `null` while no prompt is open. */
  taken = $state<string[] | null>(null);

  /** Whether the window may close now; otherwise the prompt opens (or stays open). */
  async allowed(): Promise<boolean> {
    if (this.taken !== null) return false;
    const taken = await commands.humanTaken();
    if (taken.length === 0) return this.taken === null;
    this.taken = taken;
    return false;
  }

  /** A close request from the window: prevented unless closing is allowed. */
  async closeRequested(event: { preventDefault(): void }): Promise<void> {
    if (!(await this.allowed())) event.preventDefault();
  }

  stay(): void {
    this.taken = null;
  }
}
