// The shown branch's Current: the backend scans its worktree as `artifactize status` does and
// keeps the scan for its graph and Details; this holds the newest answer for one branch.
import { call, commands, message, type BranchKey, type Commands, type CurrentView } from "../ipc/client";
import { keyOf } from "./monitor-state.svelte";

export class Current {
  constructor(readonly ipc: Commands = commands) {}

  /** The newest scan of the shown branch. */
  view = $state.raw<CurrentView | null>(null);
  error = $state<string | null>(null);
  /** A scan is running. */
  scanning = $state(false);
  /** The structure text is expanded. */
  structure = $state(false);
  /** Each scan; only the newest one shows what it found. */
  #scans = 0;

  /** The scan shown is of `branch`. */
  of(branch: BranchKey): boolean {
    return this.view !== null && keyOf(this.view.branch) === keyOf(branch);
  }

  /** Scan `branch` again; another branch's scan shown is dropped at once. */
  async scan(branch: BranchKey): Promise<void> {
    const scan = ++this.#scans;
    if (this.view && !this.of(branch)) {
      this.view = null;
      this.structure = false;
    }
    this.scanning = true;
    try {
      const view = await call(this.ipc.currentScan(branch));
      if (scan !== this.#scans) return;
      this.view = view;
      this.error = null;
    } catch (error) {
      if (scan === this.#scans) this.error = message(error);
    } finally {
      if (scan === this.#scans) this.scanning = false;
    }
  }

  /** Nothing of a branch is shown any more. */
  clear(): void {
    this.#scans += 1;
    this.view = null;
    this.error = null;
    this.scanning = false;
    this.structure = false;
  }
}
