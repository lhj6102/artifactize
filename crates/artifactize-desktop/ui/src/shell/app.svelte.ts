// The window's own facts, each with one owner: the launch the backend reported, a failure to
// start, the shown screen and the two dialogs. Shared with createContext.
import { createContext } from "svelte";
import type { LaunchView } from "../ipc/client";
import { Browse } from "../monitor/browse.svelte";
import { Registry } from "./commands.svelte";
import { Jobs } from "../ipc/jobs.svelte";
import { Review } from "../review/review.svelte";
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
  /** Human reviews by request, for every screen: leaving and reopening a request, or opening it
   * on the other screen, keeps its drafts. */
  readonly reviews = $state<Record<string, Review>>({});
  palette = $state(false);
  help = $state(false);

  /** The Review of request `id`, started on first use. A follower's ID and its original's name
   * one Review: it takes the original's ID once it has read the request. */
  review(id: string): Review {
    const known = this.reviews[id] ?? Object.values(this.reviews).find((review) => review.id === id);
    return (known ?? (this.reviews[id] = new Review(id, this.jobs)));
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
