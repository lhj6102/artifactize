// The window's own facts, each with one owner: the launch the backend reported, a failure to
// start, the shown screen and the two dialogs. Shared with createContext.
import { createContext } from "svelte";
import { commands as bindings, type LaunchView, type RequestLog } from "../ipc/client";
import { Activity, observeCommands } from "../ipc/activity.svelte";
import { Browse } from "../monitor/browse.svelte";
import { Registry } from "./commands.svelte";
import { Jobs } from "../ipc/jobs.svelte";
import { Review } from "../review/review.svelte";
import { Leaving } from "./leaving.svelte";

export type Screen = "monitor" | "review" | "settings" | "graph";

export class AppState {
  readonly activity = new Activity();
  readonly ipc = observeCommands(bindings, this.activity);
  /** The backend's launch, replaced whole. */
  launch = $state.raw<LaunchView | null>(null);
  failure = $state<string | null>(null);
  screen = $state<Screen>("monitor");
  /** The monitor's snapshot. */
  readonly browse = new Browse(this.ipc);
  readonly leaving = new Leaving(this.ipc);
  /** The numbers of this window's cancellable Human jobs. */
  readonly jobs = new Jobs();
  /** What the command palette lists: each screen registers its commands here. */
  readonly commands = new Registry();
  /** Human reviews by request, for every screen: leaving and reopening a request, or opening it
   * on the other screen, keeps its drafts. */
  readonly reviews = $state<Record<string, Review>>({});
  palette = $state(false);
  help = $state(false);

  /** One canonical Review, with requested IDs as aliases once IPC resolves them. */
  review(id: string): Review {
    return this.reviews[id] ?? (this.reviews[id] = new Review(id, this.jobs, (canonical, review) => this.#intern(canonical, review), this.ipc));
  }

  #intern(id: string, incoming: Review): Review {
    const aliases = Object.entries(this.reviews).filter(([, review]) => review === incoming);
    // A replaced state forgot this handle; a late read cannot repopulate that pool.
    if (aliases.length === 0) return incoming;
    const canonical = this.reviews[id] ?? incoming;
    for (const [alias] of aliases) this.reviews[alias] = canonical;
    this.reviews[id] = canonical;
    return canonical;
  }

  /** Each Tool log goes to the review of its request; one nobody opened drops it. */
  logged(logs: RequestLog[]): void {
    for (const { request, entries } of logs) this.reviews[request]?.logged(entries);
  }

  /** A replaced state: the reviews and drafts of the old one no longer apply. */
  forgetReviews(): void {
    for (const id of Object.keys(this.reviews)) delete this.reviews[id];
  }

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
