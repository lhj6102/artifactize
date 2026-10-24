// The shown branch's Current: the backend scans its worktree as `artifactize status` does,
// publishing the graph first and each Artifact's plan as it is known, and keeps the newest
// step for its graph and Details; this holds the newest step for one branch.
import { call, commands, message, type BranchKey, type Commands, type CurrentView } from "../ipc/client";
import { keyOf } from "./monitor-state.svelte";

export class Current {
  constructor(readonly ipc: Commands = commands) {}

  /** The newest step of the shown branch's scan. */
  view = $state.raw<CurrentView | null>(null);
  /** Why the last request failed; a scan's own failure comes with its view. */
  #failed = $state<string | null>(null);
  /** A scan request has not answered yet. */
  #asking = $state(false);
  /** The structure text is expanded. */
  structure = $state(false);
  /** The structure text, read when expanded, and the declarations it describes. */
  text = $state.raw<{ fingerprint: string; text: string } | null>(null);
  /** Each request; only the newest one shows what it found. */
  #scans = 0;
  /** The branch last asked for: its steps are read even before its request answers. */
  #branch: BranchKey | null = null;

  /** The scan still runs, or its first step has not come yet. */
  readonly scanning = $derived(this.#asking || (this.view?.scanning ?? false));
  /** Why the last request failed, else why the shown scan did. */
  readonly error = $derived(this.#failed ?? this.view?.failure ?? null);

  /** The scan shown is of `branch`. */
  of(branch: BranchKey): boolean {
    return this.view !== null && keyOf(this.view.branch) === keyOf(branch);
  }

  /** Scan `branch` again, or show its scan still running; another branch's scan shown is
   * dropped at once. */
  async scan(branch: BranchKey, refresh = false): Promise<void> {
    const scan = ++this.#scans;
    if (this.view && !this.of(branch)) {
      this.view = null;
      this.structure = false;
      this.text = null;
    }
    this.#branch = branch;
    this.#asking = true;
    try {
      const view = await call(this.ipc.currentScan(branch, refresh));
      if (scan !== this.#scans) return;
      this.#show(view);
      this.#failed = null;
    } catch (error) {
      if (scan === this.#scans) this.#failed = message(error);
    } finally {
      if (scan === this.#scans) this.#asking = false;
    }
  }

  /** `branch`'s scan published a step or ended: read it when it is the branch asked for. A
   * fast scan can end before its request answers, so this does not wait for that answer. */
  async changed(branch: BranchKey): Promise<void> {
    const asked = () => this.#branch !== null && keyOf(this.#branch) === keyOf(branch);
    if (!asked()) return;
    const scan = this.#scans;
    try {
      const view = await call(this.ipc.currentView(branch));
      if (scan === this.#scans && asked()) this.#show(view);
    } catch {
      // The state was replaced meanwhile; the next scan shows it.
    }
  }

  /** Expand or fold the structure text, reading it when expanded. */
  async expand(open: boolean): Promise<void> {
    this.structure = open;
    if (open) await this.#read();
  }

  #show(view: CurrentView): void {
    if (this.view && this.of(view.branch) && view.revision < this.view.revision) return;
    this.view = view;
    if (this.structure) void this.#read();
  }

  /** Read the structure text, unless it already describes the shown declarations. */
  async #read(): Promise<void> {
    const view = this.view;
    if (!view || this.text?.fingerprint === view.fingerprint) return;
    try {
      const text = await call(this.ipc.currentStructure(view.branch));
      if (this.view?.fingerprint === view.fingerprint) this.text = { fingerprint: view.fingerprint, text };
    } catch (error) {
      this.#failed = message(error);
    }
  }

  /** Nothing of a branch is shown any more. */
  clear(): void {
    this.#scans += 1;
    this.#branch = null;
    this.view = null;
    this.#failed = null;
    this.#asking = false;
    this.structure = false;
    this.text = null;
  }
}
