// The Git layer's history of the shown worktree, the worktree last created from it until the
// Pane lists it, and removing a worktree. One owner, kept in the AppState.
import { call, message } from "../ipc/client";
import type { Commit, Created, Workbench, WorktreeKey } from "./backend";

/** How many more commits each request shows. */
export const PAGE = 200;

export class GitLayer {
  /** The history read last, with the worktree it is of; replaced whole. */
  shown = $state.raw<{ worktree: WorktreeKey; commits: Commit[] } | null>(null);
  error = $state<string | null>(null);
  /** How many of the newest commits show. */
  limit = $state(PAGE);
  /** Asking to read again: the layer reads whenever this grows. */
  asked = $state(0);
  /** The commit a worktree is being created from. */
  creating = $state<string | null>(null);
  /** The worktree created last, which the layer names. */
  created = $state.raw<Created | null>(null);
  /** The created worktree's folder, until the Pane lists it and it opens. */
  opening = $state<string | null>(null);
  #reads = 0;
  readonly #backend: Workbench;

  constructor(backend: Workbench) {
    this.#backend = backend;
  }

  /** Read `worktree`'s history; only the newest read shows. */
  async read(worktree: WorktreeKey): Promise<void> {
    const read = ++this.#reads;
    try {
      const commits = await call(this.#backend.gitHistory(worktree));
      if (read !== this.#reads) return;
      this.shown = { worktree, commits };
      this.error = null;
    } catch (failure) {
      if (read !== this.#reads) return;
      this.shown = null;
      this.error = message(failure);
    }
  }

  /** Create a worktree at `commit` of `worktree`'s repository; it opens once the Pane lists it. */
  async create(worktree: WorktreeKey, commit: string): Promise<void> {
    if (this.creating !== null) return;
    this.creating = commit;
    try {
      this.created = await call(this.#backend.worktreeCreate(worktree, commit));
      this.opening = this.created.worktree;
      this.error = null;
    } catch (failure) {
      this.error = message(failure);
    } finally {
      this.creating = null;
    }
  }

  /** Remove `worktree`, which ends its terminal; the Pane drops it with the next snapshot.
   * Whether it went is the answer. */
  async remove(worktree: WorktreeKey): Promise<boolean> {
    try {
      await call(this.#backend.worktreeRemove(worktree));
      this.error = null;
      return true;
    } catch (failure) {
      this.error = message(failure);
      return false;
    }
  }
}
