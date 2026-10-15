import { createContext, untrack } from "svelte";

import type { CommandActivity } from "./activity";

// Read-only command activity at the frontend's IPC boundary. Arguments and results are
// private; only the public search query is retained so a deferred search can be identified.

export class Activity {
  #revision = $state(0);
  #commands = $state<Record<string, { started: number; completed: number }>>({});
  #search = $state<{ query: string; sequence: number; completed: boolean } | null>(null);
  readonly #projectedCommands = $derived(Object.freeze(Object.fromEntries(
    Object.entries(this.#commands).map(([name, state]) => [name, Object.freeze({
      ...state, pending: state.started - state.completed,
    })]),
  )));
  readonly #pending = $derived(Object.values(this.#projectedCommands).reduce((total, state) => total + state.pending, 0));
  readonly #snapshot = $derived.by((): CommandActivity => Object.freeze({
    revision: this.#revision,
    pending: this.#pending,
    commands: this.#projectedCommands,
    search: this.#search === null ? null : Object.freeze({ ...this.#search }),
  }));

  snapshot(): CommandActivity {
    return this.#snapshot;
  }

  async run<T>(name: string, operation: () => Promise<T>, query?: string): Promise<T> {
    this.#commands[name] ??= { started: 0, completed: 0 };
    const state = this.#commands[name];
    state.started += 1;
    this.#revision += 1;
    const sequence = state.started;
    if (query !== undefined) this.#search = { query, sequence, completed: false };
    try { return await operation(); }
    finally {
      state.completed += 1;
      this.#revision += 1;
      if (query !== undefined && this.#search?.sequence === sequence) this.#search.completed = true;
    }
  }
}

export const [getActivity, setActivity] = createContext<Activity>();

/** Instrument the normal command boundary once, without changing generated bindings or
 * replacing Tauri's invoke. Consumers can only read detached, immutable snapshots. */
export function observeCommands<T extends Record<string, (...args: never[]) => Promise<unknown>>>(source: T, activity: Activity): T {
  return Object.fromEntries(Object.entries(source).map(([name, command]) => [name, (...args: never[]) => {
    const query: unknown = args[0];
    // Dispatch is an action, not a dependency of an attachment that requests a view.
    return untrack(() => activity.run(name, () => command(...args), name === "reviewSearch" && typeof query === "string" ? query : undefined));
  }])) as T;
}

/** Publish only this mounted window's projection, and never leave a disposed owner behind. */
export function exposeActivity(activity: Activity): () => void {
  const get = () => activity.snapshot();
  Object.defineProperty(window, "artifactizeCommands", { configurable: true, get });
  return () => {
    if (Object.getOwnPropertyDescriptor(window, "artifactizeCommands")?.get === get) {
      Reflect.deleteProperty(window, "artifactizeCommands");
    }
  };
}

declare global {
  interface Window {
    readonly artifactizeCommands: CommandActivity;
  }
}
