// One Human review, shown as a conversation in Review, the Detail and the pet overlay: what it
// shows, which area has focus and the running job. Every write goes through a backend job; the
// request's state is whatever the backend last read. Its draft (its Points, its issues and the
// chosen verdict) is the library's, one per review and reviewer, shared by every surface and a
// viewer's Point; whether a verdict can be submitted is the library's readiness: GREEN with no
// issue, RED with at least one.
import type { Jobs } from "../ipc/jobs.svelte";
import { CommandError, call, commands, message, type Commands } from "../ipc/client";
import type {
  Acted,
  DraftChange,
  HumanView,
  LogEntry,
  Notice,
  ReviewDraft,
  ReviewPoint,
  Settled,
  Verdict,
} from "../ipc/client";

export type Area = "instruction" | "tools" | "issues";

/** What the review's running work is: the decision bar shows a claim's, a release's and a
 * submission's seconds and Cancel in their places; a tool's stand on its run's line. */
export type Action = "claim" | "release" | "submit" | "tool";

/** The review's running work; `tool` marks a tool's run, which a submission cancels first. */
type Busy = { label: string; action: Action; job: number; since: number; tool: boolean };

/** One canonical request owns these facts. Alias handles adopt this same instance,
 * never copy its drafts or running work. It is shared through AppState. */
class ReviewState {
  id = $state("");
  view = $state.raw<HumanView | null>(null);
  area = $state<Area>("issues");
  tool = $state(0);
  /** Every tool run of this review, oldest first: kept while the review is open. */
  log = $state.raw<LogEntry[]>([]);
  busy = $state<Busy | null>(null);
  /** Settles once the running work has ended, its busy state cleared. */
  idle: Promise<void> = Promise.resolve();
  notice = $state.raw<Notice | null>(null);
  /** Why the library refused the last submission, until the draft changes or it is tried again. */
  refusal = $state<string | null>(null);
  /** The library's draft of this review for this reviewer, as last read or answered. */
  draft = $state.raw<ReviewDraft | null>(null);
  /** The newest point seen; removing it does not make another point look newly added. */
  latestPoint = $state.raw<ReviewPoint | null>(null);
  draftRead = 0;
  draftReadJob: Promise<void> | null = null;
  draftRevision = 0;
  choice = 0;
  /** Issue edits, one after another, so each names the issue it saw. */
  issuesQueue: Promise<void> = Promise.resolve();

  constructor(id: string) { this.id = id; }
}

export class Review {
  #state = $state.raw(new ReviewState(""));
  get id(): string { return this.#state.id; }
  set id(value: string) { this.#state.id = value; }
  get view(): HumanView | null { return this.#state.view; }
  set view(value: HumanView | null) { this.#state.view = value; }
  get area(): Area { return this.#state.area; }
  set area(value: Area) { this.#state.area = value; }
  get tool(): number { return this.#state.tool; }
  set tool(value: number) { this.#state.tool = value; }
  get log(): LogEntry[] { return this.#state.log; }
  set log(value: LogEntry[]) { this.#state.log = value; }
  get busy(): Busy | null { return this.#state.busy; }
  set busy(value: Busy | null) { this.#state.busy = value; }
  get notice(): Notice | null { return this.#state.notice; }
  set notice(value: Notice | null) { this.#state.notice = value; }
  get draft(): ReviewDraft | null { return this.#state.draft; }
  get refusal(): string | null { return this.#state.refusal; }

  get latestPoint(): ReviewPoint | null { return this.#state.latestPoint; }

  readonly settled = $derived(this.view?.stage === "completed");
  readonly owned = $derived(this.view?.stage === "review");
  readonly claimable = $derived(this.view?.stage === "claim" && this.view.claim === null);
  /** The verdict the draft holds, once claimed: none until GREEN or RED is chosen. */
  readonly verdict = $derived(this.owned ? this.draft?.verdict ?? null : null);
  /** What must change, oldest first, as typed. */
  readonly issues = $derived(this.owned ? this.draft?.issues ?? [] : []);

  /** Why Submit cannot submit now, `typed` being what the issue field holds and has not added;
   * `null` when it can. */
  blocked(typed: string): string | null {
    const verdict = this.verdict;
    if (!verdict || !this.draft) return "Choose GREEN or RED";
    if (typed.trim() !== "") return "Add the typed issue first";
    const ready = verdict === "GREEN" ? this.draft.readiness.green : this.draft.readiness.red;
    return ready.kind === "blocked" ? ready.reason : null;
  }

  /** Submit is open while nothing blocks it, also while a tool runs (a submission cancels it
   * first), but not while a claim, a release or a submission runs. */
  submittable(typed: string): boolean {
    return this.owned && !this.settled && this.blocked(typed) === null && (this.busy === null || this.busy.tool);
  }

  readonly #jobs: Jobs;
  readonly #intern: ((id: string, review: Review) => Review) | undefined;

  constructor(id: string, jobs: Jobs, intern?: (id: string, review: Review) => Review, readonly ipc: Commands = commands) {
    this.#state.id = id;
    this.#jobs = jobs;
    this.#intern = intern;
  }

  /** Read the request (or, without `reload`, redraw its waiting time from the last read). */
  async load(reload = true): Promise<void> {
    try {
      this.#show(await call(this.ipc.humanOpen(this.id, reload)));
    } catch (error) {
      this.notice = { text: message(error), error: true };
    }
  }

  /** A read made elsewhere (a reference's opening): show it as this review's own. */
  adopt(view: HumanView): void {
    this.#show(view);
  }

  #show(view: HumanView): void {
    const canonical = this.#intern?.(view.id, this) ?? this;
    if (canonical !== this) this.#state = canonical.#state;
    // Reads answer in any order. One read before the shown view would bring back what that
    // view already replaced, a claim's absence say, and take the open draft away until the
    // next read; it is dropped.
    if (this.view && view.read < this.view.read) return;
    this.view = view;
    this.id = view.id;
    this.tool = Math.min(this.tool, Math.max(view.tools.length - 1, 0));
    // Settled here or by another command: nothing is left to edit, and the Tool log is cleared.
    if (view.stage === "completed") {
      this.#state.choice += 1;
      this.#state.draft = null;
      this.#state.latestPoint = null;
      this.log = [];
    }
    // Claimed by this reviewer: its draft, kept by the library, is written from here on.
    if (view.stage === "review" && this.#state.draft === null) void this.readDraft();
  }

  /** Read the library's draft again: after a change made elsewhere (the pet overlay, a Point,
   * another window). */
  readDraft(): Promise<void> {
    const state = this.#state;
    const read = ++state.draftRead;
    const job = this.#readDraft(state, read);
    state.draftReadJob = job;
    return job;
  }

  async #readDraft(state: ReviewState, read: number): Promise<void> {
    try {
      const draft = await call(this.ipc.humanDraft(this.id));
      if (state === this.#state && read === state.draftRead && this.owned) this.#take(draft);
    } catch (error) {
      if (state === this.#state && read === state.draftRead && this.owned) this.notice = { text: message(error), error: true };
    } finally {
      if (read === state.draftRead) state.draftReadJob = null;
      // A write waiting for this refresh must keep its typed override until the newer read answers.
      else if (state === this.#state) await state.draftReadJob;
    }
  }

  /** Remove the exact point the reviewer saw, as identified by the library. */
  async removePoint(point: ReviewPoint): Promise<void> {
    const state = this.#state;
    const answered = this.#answering();
    try {
      await answered(await call(this.ipc.humanDraftRemovePoint(this.id, point)));
    } catch (error) {
      if (state === this.#state && this.owned) this.notice = { text: message(error), error: true };
    }
  }

  /** The library's answer: the draft whole, its verdict and issues following it. */
  #take(draft: ReviewDraft): void {
    this.#saveDraft(draft);
  }

  #saveDraft(draft: ReviewDraft): void {
    const state = this.#state;
    state.draftRevision += 1;
    state.draft = draft;
    state.refusal = null;
    const newest = draft.points.at(-1);
    // The library timestamps in fixed-width UTC RFC3339, so this preserves nanosecond order.
    if (newest && (!state.latestPoint || newest.at > state.latestPoint.at)) state.latestPoint = newest;
  }

  /** A local write and a draft invalidation can overlap (e.g. typing while Point arrives).
   * An answer from before another observed snapshot cannot replace it: re-read instead. */
  #answering(): (draft: ReviewDraft) => Promise<void> {
    const state = this.#state;
    const revision = state.draftRevision;
    return async (draft) => {
      if (state !== this.#state || !this.owned) return;
      if (revision !== state.draftRevision || state.draftReadJob !== null) await this.readDraft();
      else this.#take(draft);
    };
  }

  #acted(acted: Pick<Acted, "notice" | "view">): void {
    this.notice = acted.notice;
    if (acted.view) this.#show(acted.view);
  }

  async #job<T>(label: string, action: Action, work: (job: number) => Promise<T>): Promise<T | undefined> {
    if (this.busy) return undefined;
    const job = this.#jobs.next();
    this.busy = { label, action, job, since: Date.now(), tool: action === "tool" };
    let ended!: () => void;
    this.#state.idle = new Promise((resolve) => (ended = resolve));
    try {
      return await work(job);
    } finally {
      if (this.busy?.job === job) this.busy = null;
      ended();
    }
  }

  /** Claim explicitly; tools and submission never claim. */
  async claim(): Promise<void> {
    if (this.busy || this.settled) return;
    try {
      const acted = await this.#job("Claiming the review", "claim", (job) =>
        call(this.ipc.humanClaim(this.id, job)),
      );
      if (acted) this.#acted(acted);
      if (acted && this.owned) await this.readDraft();
    } catch (error) {
      this.notice = { text: message(error), error: true };
    }
  }

  /** Every issue edit asked for has reached the library. */
  async #issuesSaved(): Promise<void> {
    await this.#state.issuesQueue;
  }

  /** Release the claim, also while issues are written: the draft is kept for the next claim. */
  async release(): Promise<void> {
    if (this.busy || this.settled) return;
    try {
      // What was written reaches the library first; released, its draft stays for the next claim.
      const acted = await this.#job("Releasing the claim", "release", async (job) => {
        await this.#issuesSaved();
        return call(this.ipc.humanRelease([this.id], job));
      });
      if (acted) this.#acted(acted);
    } catch (error) {
      this.notice = { text: message(error), error: true };
    }
  }

  /** Choose `verdict`: the library's draft takes it, the issues kept. */
  async choose(verdict: Verdict): Promise<void> {
    if (!this.owned || this.settled || (this.busy && !this.busy.tool)) return;
    const state = this.#state;
    const choice = ++state.choice;
    if (this.verdict === verdict) return;
    const answered = this.#answering();
    let answer: ReviewDraft;
    try {
      answer = await call(this.ipc.humanDraftEdit(this.id, { kind: "verdict", verdict }));
    } catch (error) {
      if (state === this.#state && choice === state.choice) this.notice = { text: message(error), error: true };
      return;
    }
    // Another verdict was chosen, or editing stopped, while this choice was on its way.
    if (state !== this.#state || choice !== state.choice) return;
    await answered(answer);
  }

  /** Add `text` as the newest issue; empty text adds nothing. Whether the library took it. */
  addIssue(text: string): Promise<boolean> {
    if (text.trim() === "") return Promise.resolve(false);
    return this.#issues({ kind: "addIssue", text });
  }

  /** Change issue `index`, which read `previous` when its edit began, to `text`. */
  editIssue(index: number, previous: string, text: string): Promise<boolean> {
    if (text.trim() === "" || text === previous) return Promise.resolve(text === previous);
    return this.#issues({ kind: "editIssue", index, previous, text });
  }

  /** Remove issue `index`, which read `previous`. */
  removeIssue(index: number, previous: string): Promise<boolean> {
    return this.#issues({ kind: "removeIssue", index, previous });
  }

  /** One issue edit after another; the library's answer replaces the draft whole. A refused
   * edit says why and shows the draft as it now stands. */
  #issues(change: DraftChange): Promise<boolean> {
    if (!this.owned || this.settled) return Promise.resolve(false);
    const state = this.#state;
    const wrote = state.issuesQueue.then(async () => {
      const answered = this.#answering();
      try {
        await answered(await call(this.ipc.humanDraftEdit(this.id, change)));
        return true;
      } catch (error) {
        if (state !== this.#state) return false;
        this.notice = { text: message(error), error: true };
        if (this.owned) await this.readDraft();
        return false;
      }
    });
    state.issuesQueue = wrote.then(() => {});
    return wrote;
  }

  /** Submit the chosen verdict with the issues, `typed` being what the issue field holds and has
   * not added; the result says how the settled request stands, or `null` when nothing was
   * settled. */
  async submit(typed = ""): Promise<Settled | null> {
    if (!this.submittable(typed)) return null;
    // A running tool ends first: submitting settles the review its run belongs to.
    if (this.busy?.tool) {
      const idle = this.#state.idle;
      this.cancel();
      await idle;
    }
    const verdict = this.verdict;
    const state = this.#state;
    if (this.busy || this.settled || !verdict) return null;
    try {
      const submitted = await this.#job(`Submitting ${verdict}`, "submit", async (job) => {
        // What is written reaches the library first: it submits the draft it keeps.
        await this.#issuesSaved();
        return call(this.ipc.humanDraftSubmit(this.id, job));
      });
      if (!submitted) return null;
      state.refusal = null;
      state.draft = null;
      this.log = [];
      this.#acted(submitted);
      return submitted.settled;
    } catch (error) {
      if (error instanceof CommandError && error.failure.kind !== "read") state.refusal = error.message;
      else this.notice = { text: message(error), error: true };
      return null;
    }
  }


  /** Run a tool at once; the backend checks the claim when it starts. What the tool prints
   * reaches the Tool log as it runs ([`logged`](#logged)); a launch tool returns at once. */
  async run(index = this.tool): Promise<void> {
    if (this.busy || this.settled) return;
    const tool = this.view?.tools[index];
    if (!tool) {
      this.notice = { text: "This request declares no Human tools.", error: true };
      return;
    }
    this.tool = index;
    try {
      const notice = await this.#job(`Running ${tool.operation}`, "tool", (job) =>
        call(this.ipc.humanRun(this.id, tool.name, job)),
      );
      if (!notice) return;
      // A tool's failure is said on its run's line, and nowhere else (D-REV-2).
      this.notice = notice.error ? null : notice;
      // A viewer tool opened a viewer session: its Artifact Viewer window shows it (J-VIEWER).
      if (notice.viewer) await call(this.ipc.viewerWindow(notice.viewer.session, notice.viewer.title));
      // Declarations may have changed; show what the next run resolves.
      await this.load();
    } catch (error) {
      this.notice = { text: message(error), error: true };
    }
  }

  /** This review's Tool log, whole, as the backend sends it after each change. */
  logged(entries: LogEntry[]): void {
    if (this.view?.stage === "completed") return;
    this.log = entries;
  }

  cancel(): void {
    if (this.busy) void this.ipc.humanCancel(this.busy.job);
  }

  /** Esc closes one thing at a time: the running work first; `true` means leave the review. */
  escape(): boolean {
    if (this.busy) {
      this.cancel();
      return false;
    }
    return true;
  }
}
