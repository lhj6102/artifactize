import { createContext, untrack } from "svelte";

import type { CommandActivity, ReviewRefresh } from "./activity";

// Read-only command activity at the frontend's IPC boundary. Arguments and results are
// private; only the public search query is retained so a deferred search can be identified.

/** The manual Review-list refreshes of one window that showed a fresh read. */
class ReviewRefreshes {
  #last = $state.raw<ReviewRefresh | null>(null);

  get last(): ReviewRefresh | null {
    return this.#last;
  }

  shown(query: string, start: number): void {
    this.#last = Object.freeze({ generation: (this.#last?.generation ?? 0) + 1, query, start });
  }
}

// E2E builds only. The mode is replaced at build time, so other builds keep no table, no
// snapshot field and no recorder: the bundle drops all of it.
const refreshes = import.meta.env.MODE === "e2e" ? new WeakMap<Activity, ReviewRefreshes>() : null;

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
    ...(import.meta.env.MODE === "e2e" ? { reviewRefresh: refreshes?.get(this)?.last ?? null } : {}),
  }));

  constructor() {
    if (import.meta.env.MODE === "e2e") refreshes?.set(this, new ReviewRefreshes());
  }

  snapshot(): CommandActivity {
    return this.#snapshot;
  }

  /** Availability comes from the commands actually wrapped, not a missing-as-zero guess. */
  register(name: string): void {
    this.#commands[name] ??= { started: 0, completed: 0 };
  }

  async run<T>(name: string, operation: () => Promise<T>, query?: string): Promise<T> {
    this.register(name);
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

/** E2E builds only: a manual Review-list refresh showed a fresh read of `query` from `start`.
 * Observation only; nothing in the app reads it back. Callers guard on the build mode too. */
export function reviewRefreshed(activity: Activity, query: string, start: number): void {
  refreshes?.get(activity)?.shown(query, start);
}

export const [getActivity, setActivity] = createContext<Activity>();

/** Instrument the normal command boundary once, without changing generated bindings or
 * replacing Tauri's invoke. Consumers can only read detached, immutable snapshots. */
export function observeCommands<T extends Record<string, (...args: never[]) => Promise<unknown>>>(source: T, activity: Activity): T {
  return Object.fromEntries(Object.entries(source).map(([name, command]) => {
    activity.register(name);
    return [name, (...args: never[]) => {
      const query: unknown = args[0];
      // Dispatch is an action, not a dependency of an attachment that requests a view.
      return untrack(() => activity.run(name, () => command(...args), name === "reviewSearch" && typeof query === "string" ? query : undefined));
    }];
  })) as T;
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
