// The shown branch's definitions as its `.artf` code declares them: where each Artifact is
// bound, the macro that made it, its target, parts and toolsets. Nothing here writes: a
// definition changes only by an edit of its file, and the next scan of the Current reads it
// again (D-INS-10). An Artifact's Source is read when its section first shows it.
import { SvelteMap } from "svelte/reactivity";
import { call, commands, message, type BranchKey, type Commands, type DefinitionNode, type DefinitionSource, type DefinitionsView } from "../ipc/client";
import { keyOf } from "./monitor-state.svelte";

export class Definitions {
  constructor(readonly ipc: Commands = commands) {}

  /** The definitions of the branch last read. */
  view = $state.raw<DefinitionsView | null>(null);
  readonly #nodes = $derived(new SvelteMap(this.view?.nodes.map((node) => [node.name, node])));
  /** Why the definitions cannot be read now; the graph is drawn from its scan alone. */
  error = $state<string | null>(null);
  /** Each Artifact's Source as read since the definitions were, by name. */
  readonly sources = new SvelteMap<string, DefinitionSource | { error: string }>();
  /** The branch whose definitions are shown: state, so that what asked whether they are of a
   * branch before any were read asks again once they are. */
  #branch = $state.raw<BranchKey | null>(null);
  #reads = 0;
  /** The read of the branch last asked for, while it has not answered. */
  #loading: { key: string; read: Promise<void> } | null = null;

  /** The definitions are of `branch`. */
  of(branch: BranchKey): boolean {
    return this.#branch !== null && keyOf(this.#branch) === keyOf(branch) && this.view !== null;
  }

  node(name: string): DefinitionNode | undefined {
    return this.#nodes.get(name);
  }

  /** `branch`'s definitions are read, or cannot be. */
  ready(branch: BranchKey): Promise<void> {
    if (this.#loading?.key === keyOf(branch)) return this.#loading.read;
    if (this.of(branch)) return Promise.resolve();
    return this.load(branch);
  }

  /** Read `branch`'s definitions as they are now; another branch's are dropped at once. */
  load(branch: BranchKey): Promise<void> {
    const read = this.#read(branch);
    const loading = { key: keyOf(branch), read };
    this.#loading = loading;
    return read.finally(() => {
      if (this.#loading === loading) this.#loading = null;
    });
  }

  async #read(branch: BranchKey): Promise<void> {
    const read = ++this.#reads;
    if (this.#branch && keyOf(this.#branch) !== keyOf(branch)) this.view = null;
    this.#branch = branch;
    try {
      const view = await call(this.ipc.definitions(branch));
      if (read !== this.#reads) return;
      // The code may have changed: each Source is read again when it is next shown.
      this.sources.clear();
      this.view = view;
      this.error = null;
    } catch (failure) {
      if (read === this.#reads) {
        this.view = null;
        this.sources.clear();
        this.error = message(failure);
      }
    }
  }

  /** Read `name`'s Source on `branch`, unless it was read since the definitions were. */
  async source(branch: BranchKey, name: string): Promise<void> {
    if (this.sources.has(name)) return;
    const read = this.#reads;
    try {
      const source = await call(this.ipc.definitionSource(branch, name));
      if (read === this.#reads) this.sources.set(name, source);
    } catch (failure) {
      if (read === this.#reads) this.sources.set(name, { error: message(failure) });
    }
  }

  /** Nothing of a branch is shown any more. */
  clear(): void {
    this.#reads += 1;
    this.#branch = null;
    this.view = null;
    this.error = null;
    this.sources.clear();
  }
}
