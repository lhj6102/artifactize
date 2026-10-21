// The window's own facts, each with one owner: the launch the backend reported, a failure to
// start, the Pane's and Main area's state and the dialogs. Shared with createContext.
import { createContext } from "svelte";
import { commands as bindings, type BranchKey, type KeyedLog, type LaunchView } from "../ipc/client";
import { Activity, observeCommands } from "../ipc/activity.svelte";
import { Browse } from "../monitor/browse.svelte";
import { Current } from "../monitor/current.svelte";
import { MonitorState } from "../monitor/monitor-state.svelte";
import { ArtifactTools, toolsKey } from "../monitor/artifact-tools.svelte";
import { Registry } from "./commands.svelte";
import { Jobs } from "../ipc/jobs.svelte";
import { Review } from "../review/review.svelte";
import { Leaving } from "./leaving.svelte";

export class AppState {
  readonly activity = new Activity();
  readonly ipc = observeCommands(bindings, this.activity);
  /** The backend's launch, replaced whole. */
  launch = $state.raw<LaunchView | null>(null);
  failure = $state<string | null>(null);
  /** The live branches, the waiting reviews and the shown Run. */
  readonly browse = new Browse(this.ipc);
  /** The shown branch's Current. */
  readonly current = new Current(this.ipc);
  /** The Pane's selection, what the Main area shows and its open Detail. */
  readonly monitor = new MonitorState();
  readonly leaving = new Leaving(this.ipc);
  /** The numbers of this window's cancellable Human jobs. */
  readonly jobs = new Jobs();
  /** What the command palette lists: each part of the window registers its commands here. */
  readonly commands = new Registry();
  /** Human reviews by request: leaving and reopening a request keeps its drafts. */
  readonly reviews = $state<Record<string, Review>>({});
  /** Each Artifact's own tools and Tool log, by worktree and Artifact, while the app runs. */
  readonly tools = $state<Record<string, ArtifactTools>>({});
  palette = $state(false);
  help = $state(false);
  /** The Open by ID dialog. */
  asking = $state(false);

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

  /** The tools of Artifact `artifact` in the worktree `workspace` of `branch`. */
  artifactTools(branch: BranchKey, workspace: string, artifact: string): ArtifactTools {
    const key = toolsKey(workspace, artifact);
    return this.tools[key] ?? (this.tools[key] = new ArtifactTools(branch, artifact, this.jobs, this.ipc));
  }

  /** Each Tool log goes to its review or Artifact; one nobody opened drops it. */
  logged(logs: KeyedLog[]): void {
    for (const { key, entries } of logs) {
      if (key.kind === "review") this.reviews[key.request]?.logged(entries);
      else this.tools[toolsKey(key.workspace, key.artifact)]?.logged(entries);
    }
  }

  /** A replaced state: the reviews and drafts of the old one no longer apply. */
  forgetReviews(): void {
    for (const id of Object.keys(this.reviews)) delete this.reviews[id];
  }

  /** The backend's launch arrived: remember it; the first snapshot opens what it asks for. */
  start(launch: LaunchView): void {
    this.launch = launch;
  }
}

export const [getAppState, setAppState] = createContext<AppState>();
