// What the window knows of each worktree's terminal besides its screen: whether its process
// ended, what a terminal that was gone offers to restore, whether it outlives the app, and where
// its agent moved. One owner, kept in the AppState; xterm.js holds the screen itself.
import { call, message } from "../ipc/client";
import type { RestoreChoice, RestoreOffer, TerminalFrame, Workbench, WorktreeKey } from "./backend";

export type Session = {
  /** Each fresh start attaches the screen anew. */
  attempt: number;
  /** The process ended, with its exit status when known. */
  ended: { code: number | null } | null;
  /** A terminal that was gone: what its fresh shell can restore, until the person chooses. */
  offer: RestoreOffer | null;
  /** Why the terminal ends with the app, if it does. */
  fragile: string | null;
  /** The worktree its agent moved to, until opened or dismissed. */
  moved: WorktreeKey | null;
  /** What went wrong last: attaching, typing or restoring. */
  error: string | null;
};

const UNTOUCHED: Session = { attempt: 0, ended: null, offer: null, fragile: null, moved: null, error: null };

export class Terminals {
  /** Each worktree's terminal by its worktree, once attached in this window. */
  readonly sessions = $state<Record<WorktreeKey, Session>>({});
  readonly #backend: Workbench;

  constructor(backend: Workbench) {
    this.#backend = backend;
  }

  /** The worktree's terminal; one never attached has nothing to say yet. */
  of(worktree: WorktreeKey): Session {
    return this.sessions[worktree] ?? UNTOUCHED;
  }

  #set(worktree: WorktreeKey, change: Partial<Session>): void {
    this.sessions[worktree] = { ...this.of(worktree), ...change };
  }

  /** A frame besides the screen's own output. A snapshot means a live terminal again. */
  heard(worktree: WorktreeKey, frame: TerminalFrame): void {
    if (frame.kind === "snapshot") this.#set(worktree, { ended: null, error: null });
    else if (frame.kind === "ended") this.#set(worktree, { ended: { code: frame.code } });
    else if (frame.kind === "restore") this.#set(worktree, { offer: frame.offer });
    else if (frame.kind === "notPersistent") this.#set(worktree, { fragile: frame.reason });
    else if (frame.kind === "agentMoved") this.#set(worktree, { moved: frame.worktree });
  }

  failed(worktree: WorktreeKey, failure: unknown): void {
    this.#set(worktree, { error: message(failure) });
  }

  /** Run what the person confirmed of the restore offer; the offer goes once the backend took
   * it, and stays on failure. */
  async restore(worktree: WorktreeKey, choice: RestoreChoice): Promise<void> {
    try {
      await call(this.#backend.terminalRestore(worktree, choice));
      this.#set(worktree, { offer: null, error: null });
    } catch (failure) {
      this.failed(worktree, failure);
    }
  }

  /** Start a fresh terminal after the last one ended. */
  fresh(worktree: WorktreeKey): void {
    this.#set(worktree, { attempt: this.of(worktree).attempt + 1, ended: null, error: null });
  }

  /** Forget where the agent moved: opened, or dismissed. */
  settled(worktree: WorktreeKey): void {
    this.#set(worktree, { moved: null });
  }
}
