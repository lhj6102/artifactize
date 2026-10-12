// The window's own facts, each with one owner: the launch the backend reported, a failure to
// start, the shown screen and the two dialogs. Shared with createContext.
import { createContext } from "svelte";
import type { LaunchView } from "../ipc/client";

export type Screen = "monitor" | "review" | "settings";

export class AppState {
  /** The backend's launch, replaced whole. */
  launch = $state.raw<LaunchView | null>(null);
  failure = $state<string | null>(null);
  screen = $state<Screen>("monitor");
  palette = $state(false);
  help = $state(false);
}

export const [getAppState, setAppState] = createContext<AppState>();
