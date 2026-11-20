// Which sections of the inspector are folded, by their names: a section stays folded or
// unfolded as a person left it, in every Detail opened later and in the next launch. Until a
// person folds or unfolds one, what a tab is opened for is unfolded and what is read now and
// then starts folded.
import { SvelteMap } from "svelte/reactivity";

const KEY = "detail-sections";

/** The sections a tab is opened for; every other foldable section starts folded. */
const UNFOLDED = new Set(["Evals", "Tools", "Tool log", "Path", "Definition", "Result", "Outcome", "Waits for"]);

export class Sections {
  /** What a person chose, by section name: true for unfolded. */
  readonly #chosen = new SvelteMap<string, boolean>(load());

  /** Whether the section `name` shows what it holds. */
  open(name: string): boolean {
    return this.#chosen.get(name) ?? UNFOLDED.has(name);
  }

  /** Fold or unfold the section `name`, and keep that for the next launch. */
  set(name: string, open: boolean): void {
    this.#chosen.set(name, open);
    save(this.#chosen);
  }

  toggle(name: string): void {
    this.set(name, !this.open(name));
  }
}

/** The folds kept from earlier launches. Storage may be unavailable; then there are none. */
function load(): [string, boolean][] {
  try {
    const kept: unknown = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    if (typeof kept !== "object" || kept === null) return [];
    return Object.entries(kept).filter((entry): entry is [string, boolean] => typeof entry[1] === "boolean");
  } catch {
    return [];
  }
}

function save(chosen: ReadonlyMap<string, boolean>): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(Object.fromEntries(chosen)));
  } catch {
    // Nothing to keep it in: the folds last this window only.
  }
}
