// The window's own facts, each with one owner: the launch the backend reported, a failure to
// start, the mode shown above the bottom bar, each mode's state, the Panes and the dialogs.
// Shared with createContext.
import { createContext } from "svelte";
import { call, commands as bindings, message, type KeyedLog, type LaunchView } from "../ipc/client";
import { Activity, observeCommands } from "../ipc/activity.svelte";
import { Browse } from "../monitor/browse.svelte";
import { Current } from "../monitor/current.svelte";
import { Definitions } from "../monitor/definitions.svelte";
import { MonitorState } from "../monitor/monitor-state.svelte";
import { ArtifactTools, toolsKey } from "../monitor/artifact-tools.svelte";
import { Registry } from "./commands.svelte";
import { Jobs } from "../ipc/jobs.svelte";
import { Review } from "../review/review.svelte";
import { ReviewModeState } from "../review/review-mode-state.svelte";
import { Leaving } from "./leaving.svelte";
import { Release } from "./release.svelte";
import { Sections } from "./sections.svelte";

/** The three modes the bottom bar switches between, each a workspace of its own. */
export type WindowMode = "chat" | "verify" | "review";

export const MODES: { mode: WindowMode; label: string; keys: string }[] = [
  { mode: "chat", label: "Chat", keys: "Ctrl+1" },
  { mode: "verify", label: "Verify", keys: "Ctrl+2" },
  { mode: "review", label: "Review", keys: "Ctrl+3" },
];

export class AppState {
  readonly activity = new Activity();
  readonly ipc = observeCommands(bindings, this.activity);
  /** The backend's launch, replaced whole. */
  launch = $state.raw<LaunchView | null>(null);
  failure = $state<string | null>(null);
  /** The live branches, the waiting reviews and the shown Run. */
  readonly browse = new Browse(this.ipc);
  /** The shown branch's Current. */
  readonly current: Current = new Current(this.ipc, (branch) => void this.definitions.load(branch));
  /** The shown branch's definitions, which its Current edits. */
  readonly definitions: Definitions = new Definitions(this.ipc, (branch) => void this.current.scan(branch, true));
  /** The mode shown above the bottom bar; the others keep their place while hidden. */
  mode = $state<WindowMode>("verify");
  /** Every mode's Pane is shown; the bar's Pane switch and Ctrl+B hide them and show them again. */
  paneShown = $state(true);
  /** Verify's Pane selection, what its Main area shows and its open Detail. */
  readonly monitor = new MonitorState();
  /** Review's chosen review and what its main area shows. */
  readonly reviewMode = new ReviewModeState();
  readonly leaving = new Leaving(this.ipc);
  /** A newer release and its dialog. */
  readonly release = new Release(this.ipc);
  /** The numbers of this window's cancellable Human jobs. */
  readonly jobs = new Jobs();
  /** Which inspector sections are folded, by name, kept across launches. */
  readonly sections = new Sections();
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
  /** The tools of the Artifact `artifact` in `workspace` if a Detail asked for them; a lookup
   * that registers nothing. */
  knownTools(workspace: string, artifact: string): ArtifactTools | undefined {
    return this.tools[toolsKey(workspace, artifact)];
  }

  artifactTools(workspace: string, artifact: string): ArtifactTools {
    const key = toolsKey(workspace, artifact);
    return this.tools[key] ?? (this.tools[key] = new ArtifactTools(artifact, this.jobs, this.ipc));
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

  /** Show Review with the review `id` open, as a reference opens it: its exact request,
   * whether or not the list shows it. */
  showReview(id: string): void {
    this.review(id);
    const r = this.reviewMode;
    r.shown = { kind: "review", id };
    r.selected = id;
    r.error = null;
    this.mode = "review";
  }

  /** Open the saved request `id` in Review, as a reference opens it: a waiting Human review
   * actionable, any other request, a settled Human one included, read-only. */
  async openRequest(id: string): Promise<void> {
    const r = this.reviewMode;
    this.mode = "review";
    try {
      const detail = await call(this.ipc.requestDetail(id));
      const waiting = detail.mark?.status === "WAITING_HUMAN" || detail.mark?.status === "DEFERRED";
      if (detail.profile === "human" && waiting) return this.showReview(id);
      r.folds = { log: false };
      r.shown = { kind: "request", detail };
      r.selected = null;
      r.error = null;
    } catch (failure) {
      r.error = message(failure);
    }
  }

  /** The backend's launch arrived: remember it; the first snapshot opens what it asks for. */
  start(launch: LaunchView): void {
    this.launch = launch;
  }
}

export const [getAppState, setAppState] = createContext<AppState>();
