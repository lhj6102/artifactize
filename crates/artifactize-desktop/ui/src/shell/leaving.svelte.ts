// Leaving the window with claims this window took, or with a Chat turn running: every way out
// (q, the window's close button, repeated close requests) asks once and waits for the answer;
// only the prompt's keep, or its release (or quit) ends the window (J-QUIT).
import { call, commands, type Commands } from "../ipc/client";

/** What the open prompt asks about: the claims held, and the Chat sessions whose turn runs. */
export type Held = { taken: string[]; turns: string[] };

export class Leaving {
  /** The Chat sessions with a turn running, by title: quitting stops them. */
  turns: () => Promise<string[]> = async () => [];

  constructor(readonly ipc: Commands = commands) {}

  /** What the open prompt asks about; `null` while no prompt is open. */
  held = $state.raw<Held | null>(null);

  /** Whether the window may close now; otherwise the prompt opens (or stays open). */
  async allowed(): Promise<boolean> {
    if (this.held !== null) return false;
    // Claims that cannot be read are not asked about: quitting is never held by a failed read.
    const taken = await call(this.ipc.humanTaken()).catch(() => [] as string[]);
    const turns = await this.turns();
    if (taken.length === 0 && turns.length === 0) return this.held === null;
    this.held = { taken, turns };
    return false;
  }

  /** A close request from the window: prevented unless closing is allowed. */
  async closeRequested(event: { preventDefault(): void }): Promise<void> {
    if (!(await this.allowed())) event.preventDefault();
  }

  stay(): void {
    this.held = null;
  }
}
