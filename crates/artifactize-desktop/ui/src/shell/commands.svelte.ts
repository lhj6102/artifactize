// The command palette's registry. Screens register what they can do while they are shown;
// the palette lists what is enabled now and runs it.
import { untrack } from "svelte";

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
  // Raw state: removal compares each registered list by identity, which a deep $state proxy
  // of the lists would break (every list stayed registered and ids repeated).
  #sets = $state.raw<Command[][]>([]);
  /** Every registered command once: when two scopes offer the same id, the later one wins. */
  readonly all = $derived([...new Map(this.#sets.flat().map((command) => [command.id, command])).values()]);

  /** Add `commands` until the returned function removes them. */
  add(commands: Command[]): () => void {
    this.#sets = [...this.#sets, commands];
    return () => {
      this.#sets = this.#sets.filter((set) => set !== commands);
    };
  }
}

export const registry = new Registry();

/**
 * Register the commands `get` returns while the calling component is mounted. Only `get`
 * is tracked: the registry's own list must not make registrations re-run each other.
 */
export function useCommands(get: () => Command[]): void {
  $effect(() => {
    const commands = get();
    return untrack(() => registry.add(commands));
  });
}
