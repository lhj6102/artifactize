import { createContext, untrack } from "svelte";

import type { CommandActivity, Refresh, SearchActivity } from "./activity";
import type { BrowseView } from "./client";

// Read-only command activity at the frontend's IPC boundary. Arguments and results are
// private; only the public search query is retained so a deferred search can be identified.

/** The manual refreshes of one window that showed a fresh read. */
class Refreshes {
  #last = $state.raw<Refresh | null>(null);

  get last(): Refresh | null {
    return this.#last;
  }

  shown(read: Pick<BrowseView, "branches">): void {
    const branches = read.branches.map(({ project, name, chips }) => Object.freeze({
      project, name,
      runs: Object.freeze(chips.flatMap((chip) => chip.kind === "run" ? [Object.freeze({ id: chip.id, comment: chip.comment })] : [])),
    }));
    this.#last = Object.freeze({ generation: (this.#last?.generation ?? 0) + 1, branches: Object.freeze(branches) });
  }
}

// E2E builds only. The mode is replaced at build time, so other builds keep no table, no
// snapshot field and no recorder: the bundle drops all of it.
const refreshes = import.meta.env.MODE === "e2e" ? new WeakMap<Activity, Refreshes>() : null;

/** Only the query shown in the branch search or Review filter. */
function searched(name: string, argument: unknown): string | undefined {
  if (name === "humanSearch") return typeof argument === "string" ? argument : undefined;
  if (name !== "browse" || typeof argument !== "object" || argument === null) return undefined;
  const query: unknown = (argument as { query?: unknown }).query;
  return typeof query === "string" ? query : undefined;
}

type SearchCommand = "browse" | "humanSearch";

export class Activity {
  #revision = $state(0);
  #commands = $state<Record<string, { started: number; completed: number; failed: number }>>({});
  #searches = $state<Partial<Record<SearchCommand, SearchActivity>>>({});
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
    search: this.#searches.browse ? Object.freeze({ ...this.#searches.browse }) : null,
    reviewSearch: this.#searches.humanSearch ? Object.freeze({ ...this.#searches.humanSearch }) : null,
    ...(import.meta.env.MODE === "e2e" ? { refresh: refreshes?.get(this)?.last ?? null } : {}),
  }));

  constructor() {
    if (import.meta.env.MODE === "e2e") refreshes?.set(this, new Refreshes());
  }

  snapshot(): CommandActivity {
    return this.#snapshot;
  }

  /** Availability comes from the commands actually wrapped, not a missing-as-zero guess. */
  register(name: string): void {
    this.#commands[name] ??= { started: 0, completed: 0, failed: 0 };
  }

  async run<T>(name: string, operation: () => Promise<T>, query?: string): Promise<T> {
    this.register(name);
    const state = this.#commands[name];
    state.started += 1;
    this.#revision += 1;
    const sequence = state.started;
    const key = name === "browse" || name === "humanSearch" ? name : null;
    const search = query === undefined ? null : { query, sequence, completed: false };
    if (key && search) this.#searches[key] = search;
    let failed = false;
    try { return await operation(); }
    catch (error) {
      failed = true;
      state.failed += 1;
      throw error;
    }
    finally {
      state.completed += 1;
      this.#revision += 1;
      if (key && search && this.#searches[key]?.sequence === sequence) {
        this.#searches[key] = { ...search, completed: true, ...(failed ? { failed: true } : {}) };
      }
    }
  }
}

/** E2E builds only: a manual refresh showed a fresh read. Observation only; nothing in the app
 * reads it back. Callers guard on the build mode too. */
export function refreshed(activity: Activity, read: Pick<BrowseView, "branches">): void {
  refreshes?.get(activity)?.shown(read);
}

export const [getActivity, setActivity] = createContext<Activity>();

/** Instrument the normal command boundary once, without changing generated bindings or
 * replacing Tauri's invoke. Consumers can only read detached, immutable snapshots. */
export function observeCommands<T extends Record<string, (...args: never[]) => Promise<unknown>>>(source: T, activity: Activity): T {
  return Object.fromEntries(Object.entries(source).map(([name, command]) => {
    activity.register(name);
    return [name, (...args: never[]) => {
      // Dispatch is an action, not a dependency of an attachment that requests a view.
      return untrack(() => activity.run(name, () => command(...args), searched(name, args[0])));
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
