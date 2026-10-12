// The window's own facts, each with one owner: the launch the backend reported, a failure to
// start, the shown screen and the two dialogs. Shared with createContext.
import { createContext } from "svelte";
import type { LaunchView } from "../ipc/client";
import { Browse } from "../monitor/browse.svelte";
import { Registry } from "./commands.svelte";
import { Jobs } from "../ipc/jobs.svelte";
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
  /** The numbers of this window's cancellable Human jobs. */
  readonly jobs = new Jobs();
  /** What the command palette lists: each screen registers its commands here. */
  readonly commands = new Registry();
  palette = $state(false);
  help = $state(false);

  /** Show `screen`. */
  show(screen: Screen): void {
    this.screen = screen;
  }

  /** The backend's launch arrived: remember it and open the screen it asks for. */
  start(launch: LaunchView): void {
    this.launch = launch;
    this.show(launch.mode);
  }
}

export const [getAppState, setAppState] = createContext<AppState>();
