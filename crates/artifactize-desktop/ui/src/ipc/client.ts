// The frontend's only door to the backend: typed commands that throw a typed failure, and
// event subscriptions that always clean up.
import type { Failure, KeyedLog } from "./bindings";
import { commands as bindings, events } from "./bindings";
import { getActivity, observeCommands } from "./activity.svelte";

export * from "./bindings";

export const commands = bindings;
export type Commands = typeof commands;

/** Capture the window owner during component initialization, not inside async callbacks. */
export function useIpc(): Commands {
  return observeCommands(bindings, getActivity());
}

/** A command failure, narrowed on `failure.kind`. */
export class CommandError extends Error {
  readonly failure: Failure;
  constructor(failure: Failure) {
    super(failure.message);
    this.failure = failure;
  }
}

type Typed<T> = { status: "ok"; data: T } | { status: "error"; error: Failure };

/** Await a typed command, throwing its failure as a `CommandError`. */
export async function call<T>(result: Promise<Typed<T>>): Promise<T> {
  const settled = await result;
  if (settled.status === "error") throw new CommandError(settled.error);
  return settled.data;
}

/** The message of anything a command or the runtime threw. */
export function message(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

type Listen<T> = (callback: (event: { payload: T }) => void) => Promise<() => void>;

/**
 * Listen for an event until the returned function is called. Tauri registers listeners
 * asynchronously; a cleanup that runs first still removes the listener once it exists.
 */
export function subscribe<T>(listen: Listen<T>, callback: (payload: T) => void): () => void {
  let stop: (() => void) | undefined;
  let stopped = false;
  listen((event) => {
    if (!stopped) callback(event.payload);
  }).then(
    (unlisten) => {
      if (stopped) unlisten();
      else stop = unlisten;
    },
    // Outside Tauri (component tests) there is nothing to listen to.
    () => {},
  );
  return () => {
    stopped = true;
    stop?.();
  };
}

export const onStateChanged = (callback: (resync: boolean) => void) =>
  subscribe(events.stateChanged.listen, (payload) => callback(payload.resync));

/** The Tool logs that Human tool runs changed, each whole: a review's or an Artifact's. */
export const onHumanToolLog = (callback: (logs: KeyedLog[]) => void) =>
  subscribe(events.humanToolLog.listen, (payload) => callback(payload.logs));

export const onSessionChanged = (callback: (session: string) => void) =>
  subscribe(events.sessionChanged.listen, (payload) => callback(payload.session));
