// The command palette's registry. Screens register what they can do while they are shown;
// the palette lists what is enabled now and runs it.
import { onMount } from "svelte";

export type Command = {
  id: string;
  label: string;
  group: string;
  /** The shortcut that does the same, as shown. */
  keys?: string;
  enabled?: () => boolean;
  run: () => void;
};

class Registry {
  // Raw state: removal compares each registered getter by identity, which a deep $state proxy
  // would break (every getter stayed registered and ids repeated).
  #sources = $state.raw<(() => Command[])[]>([]);
  /** Every registered command once: when two scopes offer the same id, the later one wins.
   * The getters are read here, so a scope's commands follow the state they read. */
  readonly all = $derived([...new Map(this.#sources.flatMap((get) => get()).map((command) => [command.id, command])).values()]);

  /** Register what `get` returns until the returned function removes it. */
  add(get: () => Command[]): () => void {
    this.#sources = [...this.#sources, get];
    return () => {
      this.#sources = this.#sources.filter((source) => source !== get);
    };
  }
}

export const registry = new Registry();

/** Register the commands `get` returns while the calling component is mounted. */
export function useCommands(get: () => Command[]): void {
  onMount(() => registry.add(get));
}
