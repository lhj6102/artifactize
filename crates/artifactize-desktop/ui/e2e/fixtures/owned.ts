// What one scenario owns: its folder, under which every fixture of it lives, and what closes
// what it started. Teardown runs once the app has ended (E2E-ISOLATED).
import { waitForFixtureProcesses } from "../platform.ts";
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

  /** Delete inputs only once every owner has confirmed it no longer uses them. */
  async close(): Promise<void> {
    const failures: unknown[] = [];
    // Stand-in programs still waiting for a gate end now.
    try { writeText(nativePath(this.root, "closed"), ""); } catch (error) { failures.push(error); }
    for (const cleanup of this.cleanups.splice(0).reverse()) {
      try { await cleanup(); }
      catch (error) {
        failures.push(error);
        this.cleanups.unshift(cleanup);
      }
    }
    if (!failures.length) {
      try { await waitForFixtureProcesses(this.root); } catch (error) { failures.push(error); }
    }
    if (failures.length) throw new AggregateError(failures, `teardown of ${this.root} failed; inputs kept`);
    removeTree(this.root);
  }
}

/** A new scenario folder under `parent`. */
export function own(parent: NativePath): Owned {
  return new Owned(newDirectory(makeDirectory(parent), "fixture-"));
}
