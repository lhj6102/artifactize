// The window's own facts, each with one owner: the launch the backend reported, a failure to
// start, the shown screen and the two dialogs. Shared with createContext.
import { createContext } from "svelte";
import type { LaunchView } from "../ipc/client";
import { Browse } from "../monitor/browse.svelte";
import { Leaving } from "./leaving.svelte";

export type Screen = "monitor" | "review" | "settings";

export class AppState {
  /** The backend's launch, replaced whole. */
  launch = $state.raw<LaunchView | null>(null);
  failure = $state<string | null>(null);
  screen = $state<Screen>("monitor");
  /** The monitor's snapshot. */
  readonly browse = new Browse();
  readonly leaving = new Leaving();
  palette = $state(false);
  help = $state(false);
}

export const [getAppState, setAppState] = createContext<AppState>();
