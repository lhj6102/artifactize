// The window's own facts, each with one owner: the launch the backend reported, a failure to
// start, the mode shown above the bottom bar, each mode's state, the Panes and the dialogs.
// Shared with createContext.
import { createContext } from "svelte";
import { SvelteSet } from "svelte/reactivity";
import { call, commands as bindings, message, type BranchKey, type KeyedLog, type LaunchView } from "../ipc/client";
import { ArtifactStarState } from "../monitor/artifact-star-state.svelte";
import { Activity, observeCommands } from "../ipc/activity.svelte";
import { Browse } from "../monitor/browse.svelte";
import { Current } from "../chat/current.svelte";
import { loadWidth } from "../monitor/detail-width";
import { Definitions } from "../monitor/definitions.svelte";
import { MonitorState, keyOf } from "../monitor/monitor-state.svelte";
import { ArtifactTools, toolsKey } from "../monitor/artifact-tools.svelte";
import { Registry } from "./commands.svelte";
import { Jobs } from "../ipc/jobs.svelte";
import { Review } from "../review/review.svelte";
import { ReviewModeState } from "../review/review-mode-state.svelte";
import { ChatModeState } from "../chat/chat-state.svelte";
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
  readonly artifactStars = new ArtifactStarState(this.ipc);
  /** The backend's launch, replaced whole. */
  launch = $state.raw<LaunchView | null>(null);
  failure = $state<string | null>(null);
  /** The live branches, the waiting reviews and the shown Run. */
  readonly browse = new Browse(this.ipc);
  /** The Current of the branch shown in Chat's Graph. */
  readonly current: Current = new Current(this.ipc, (branch) => void this.prepareDefinitions(branch).load(branch));
  /** The shown branch's definitions, which its Current edits. */
  #definitions = $state.raw<Record<string, Definitions>>({});
  prepareDefinitions(branch: BranchKey): Definitions {
    const key = keyOf(branch);
    const existing = this.#definitions[key];
    if (existing) return existing;
    const owner = new Definitions(this.ipc, (each) => void this.current.scan(each, true));
    this.#definitions = { ...this.#definitions, [key]: owner };
    return owner;
  }
  readonly #emptyDefinitions = new Definitions(this.ipc);
  definitionsFor(branch: BranchKey): Definitions {
    return this.#definitions[keyOf(branch)] ?? this.#emptyDefinitions;
  }
  get definitions(): Definitions {
    const branch = this.chat.view?.session.branchKey;
    return branch ? this.definitionsFor(branch) : this.#emptyDefinitions;
  }
  /** The dragged Detail width, shared by the graph workspaces and Review. */
  detailWidth = $state<number | null>(loadWidth());
  /** The mode shown above the bottom bar; the others keep their place while hidden. */
  mode = $state<WindowMode>("verify");
  /** Every mode's Pane is shown; the bar's Pane switch and Ctrl+B hide them and show them again. */
  paneShown = $state(true);
  /** Verify's Pane selection, what its Main area shows and its open Detail. */
  readonly monitor = new MonitorState();
  /** Review's chosen review and what its main area shows. */
  readonly reviewMode = new ReviewModeState();
  /** Chat's sessions and the one shown. */
  readonly chat: ChatModeState = new ChatModeState(this.ipc, () => this.mode === "chat");
  settingsTab = $state<"providers" | "remote" | "limits">("providers");

  openSettings() {
    if (!this.monitor.settings) this.settingsTab = "providers";
    this.monitor.settings = true;
  }

  /** Quitting asks first while claims are held or a Chat turn runs in any session, as the
   * library lists them. */
  readonly leaving = Object.assign(new Leaving(this.ipc), {
    turns: async () => {
      try {
        const sessions = await call(this.ipc.chatList(null, null));
        return sessions.filter((session) => session.running).map((session) => session.title || "New chat");
      } catch {
        // Unread, no turn is named; the claims are still asked about.
        return [];
      }
    },
  });
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
  /** The Human reviews waiting for a person, whatever Review's filter shows: the bar's count. */
  waiting = $state(0);
  /** The Open by ID dialog. */
  asking = $state(false);
  wanted = $state("");
  windowWidth = $state(typeof window === "undefined" ? 0 : window.innerWidth);
  now = $state(Date.now());
  /** The active review: the claimed review last shown in Review, which viewers' Point and the
   * pet overlay act on; the library holds it and clears it when the review settles. */
  active = $state<string | null>(null);
  /** This window is in front. */
  focused = $state(true);
  /** The review the pet overlay was last asked to show, or `null` when it was hidden. */
  #pet: string | null = null;

  /** Register the last opened review, even unclaimed. Only the backend event arms Point. */
  async registerReview(id: string): Promise<void> {
    if (this.#registered === id) return;
    this.#registered = id;
    try {
      await call(this.ipc.activeReviewSet(id));
    } catch {
      if (this.#registered === id) this.#registered = null;
    }
  }
  #registered: string | null = null;

  /** Show the pet overlay for review `id`, or hide it for `null`. */
  async pet(id: string | null): Promise<void> {
    if (this.#pet === id) return;
    this.#pet = id;
    await this.ipc.petShow(id).catch(() => {});
  }

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
  knownTools(workspace: string, branch: BranchKey, artifact: string): ArtifactTools | undefined {
    return this.tools[toolsKey(workspace, branch, artifact)];
  }

  artifactTools(workspace: string, branch: BranchKey, artifact: string): ArtifactTools {
    const key = toolsKey(workspace, branch, artifact);
    return this.tools[key] ?? (this.tools[key] = new ArtifactTools(artifact, this.jobs, this.ipc));
  }

  /** Each Tool log goes to its review or Artifact; one nobody opened drops it. */
  logged(logs: KeyedLog[]): void {
    for (const { key, entries } of logs) {
      if (key.kind === "review") {
        for (const review of this.#reviewsOwnedBy(key.request)) review.logged(entries);
      } else this.knownTools(key.workspace, key.branch, key.artifact)?.logged(entries);
    }
  }

  /** Draft and Tool log events address the original request, including for shared followers. */
  #reviewsOwnedBy(owner: string): Review[] {
    return [...new SvelteSet(Object.values(this.reviews))].filter((review) => review.view?.owner === owner);
  }

  draftChanged(owner: string): void {
    for (const review of this.#reviewsOwnedBy(owner)) void review.readDraft();
  }

  /** A replaced state: the reviews and drafts of the old one no longer apply. */
  forgetReviews(): void {
    for (const id of Object.keys(this.reviews)) delete this.reviews[id];
    this.#registered = null;
  }

  /** Read the count of Human reviews waiting for a person again. */
  async countWaiting(): Promise<void> {
    try {
      this.waiting = await call(this.ipc.humanWaitingCount());
    } catch {
      // The count stays as it was; the next state change reads it again.
    }
  }

  /** Show Review with the Human review `id` open, as a reference opens it: its exact request,
   * without changing Review's filter or list (J-REVIEW-REF). */
  async showReview(id: string): Promise<void> {
    const r = this.reviewMode;
    this.mode = "review";
    try {
      const opened = await call(this.ipc.humanOpenReference(id, r.query));
      const review = this.review(id);
      review.adopt(opened.view);
      r.shown = { kind: "review", id: opened.view.id };
      r.selected = id;
      r.error = null;
    } catch (failure) {
      r.error = message(failure);
    }
  }

  /** Open the saved request `id` in Review: Human reviews open as references do, including
   * taken and submitted ones; other profiles open in the read-only Request view (J-OPEN-ID). */
  async openRequest(id: string): Promise<void> {
    const r = this.reviewMode;
    this.mode = "review";
    try {
      const detail = await call(this.ipc.requestDetail(id));
      if (detail.profile === "human") return await this.showReview(id);
      r.folds = { log: false };
      r.shown = { kind: "request", detail };
      r.selected = null;
      r.error = null;
    } catch (failure) {
      r.error = message(failure);
    }
  }

  /** The persisted Graph view of the shown, usable session, while Chat is in front. */
  readonly liveGraph = $derived(this.mode === "chat" && this.chat.view?.session.view === "graph" && this.chat.view.session.unavailable === null ? this.chat.view.session : null);

  /** A Run finished on the branch of the shown Chat session, independent of Verify's filter. */
  runFinished(branch: BranchKey): void {
    const session = this.mode === "chat" ? this.chat.view?.session : null;
    if (session && session.unavailable === null && keyOf(session.branchKey) === keyOf(branch)) void this.current.scan(session.branchKey);
  }

  /** New chat on the indicated branch or, without one, where Verify is left. */
  newChat(branch?: BranchKey): void {
    const shown = this.monitor.shown;
    void this.chat.newChat(branch ?? (shown.kind === "none" ? null : shown.branch), branch !== undefined);
  }

  /** Show a branch's Current in its most recent Chat session, or start New chat there. */
  async showBranchGraph(branch: BranchKey): Promise<void> {
    this.monitor.settings = false;
    this.mode = "chat";
    await this.chat.openBranchGraph(branch, () => {
      if (this.mode === "chat") this.newChat(branch);
    });
  }

  /** Show Chat with session `id`, brought to Run `run`'s reference when one is named. */
  showChat(id: string, run: string | null = null): void {
    this.monitor.settings = false;
    void this.chat.open(id, run ? "conversation" : undefined, run);
    this.mode = "chat";
  }

  /** Show Verify with Run `run` on its branch and node `node`'s Detail open on `tab`. */
  showRun(run: string, node: string | null = null, tab: string | null = null): void {
    const m = this.monitor;
    m.settings = false;
    m.finding = false;
    this.mode = "verify";
    void this.#showRun(run, node, tab);
  }

  /** The library reads Run `run` by its ID, whatever the Pane lists, and says its live branch. */
  async #showRun(run: string, node: string | null, tab: string | null): Promise<void> {
    if (!(await this.browse.navigate({ run: { kind: "reference", id: run } }))) return;
    const shown = this.browse.view?.run;
    if (shown?.kind !== "read" || shown.id !== run) return;
    const branch = shown.branch;
    if (!branch) {
      // Only a live branch's Runs are drawn: one whose branch is gone is read in its Detail.
      this.browse.error = `Run ${run} is not on a live branch.`;
      return;
    }
    const m = this.monitor;
    m.expanded.add(keyOf(branch));
    m.selected = { kind: "chip", branch, chip: run };
    m.shown = { kind: "run", branch, run };
    m.opened = node === null ? null : { source: { kind: "run", run }, node, tab };
  }

  /** The backend's launch arrived: remember it; the first snapshot opens what it asks for. */
  start(launch: LaunchView): void {
    this.launch = launch;
  }
}

export const [getAppState, setAppState] = createContext<AppState>();
