// What one scenario owns: its folder, under which every fixture of it lives, and what closes
// what it started. Teardown runs once the app has ended (E2E-ISOLATED).
import { makeDirectory, nativePath, newDirectory, removeTree, writeText, type NativePath } from "../portable.ts";

export class Owned {
  private readonly cleanups: (() => unknown)[] = [];

  readonly root: NativePath;

  constructor(root: NativePath) {
    this.root = root;
  }

  /** Run `cleanup` at teardown, before the files go; the last deferred runs first. */
  defer(cleanup: () => unknown): void {
    this.cleanups.push(cleanup);
  }

  /** Close what the scenario started and delete its files. */
  async close(): Promise<void> {
    const failures: unknown[] = [];
    // Stand-in programs still waiting for a gate end now.
    try { writeText(nativePath(this.root, "closed"), ""); } catch (error) { failures.push(error); }
    for (const cleanup of this.cleanups.splice(0).reverse()) {
      try { await cleanup(); } catch (error) { failures.push(error); }
    }
    try { removeTree(this.root); } catch (error) { failures.push(error); }
    if (failures.length) throw new AggregateError(failures, `teardown of ${this.root} failed`);
  }
}

/** A new scenario folder under `parent`. */
export function own(parent: NativePath): Owned {
  return new Owned(newDirectory(makeDirectory(parent), "fixture-"));
}
