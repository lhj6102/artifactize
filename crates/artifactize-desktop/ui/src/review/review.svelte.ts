// One request in the shared Human Detail: what it shows, which area has focus, the open form and
// the running job. Every write goes through a backend job; the request's state is whatever the
// backend last read. Its draft (its Points, the chosen verdict and both forms' values) is the
// library's, one per review and reviewer, shared with the pet overlay and a viewer's Point.
import type { Jobs } from "../ipc/jobs.svelte";
import { CommandError, call, commands, message, type Commands } from "../ipc/client";
import type {
  Acted,
  Draft,
  DraftChange,
  FieldValue,
  FormField,
  FormView,
  HumanView,
  LogEntry,
  Notice,
  ReviewDraft,
  ReviewPoint,
  Settled,
  Verdict,
} from "../ipc/client";

export type Area = "instruction" | "tools" | "fields";

const NO_DRAFT: Draft = { verdict: "GREEN", fields: [] };

/** The form of one verdict of the library's draft. The library owns the draft and what an edit
 * means: every edit is sent to it and its answer replaces the draft whole. A typed value shows at
 * once as a temporary override of the values derived from that draft, and is rolled back if the
 * library refuses it. */
export class FormDraft {
  readonly id: string;
  readonly verdict: Verdict;
  readonly fields: FormField[];
  /** The library's draft. */
  #snapshot!: Draft;
  /** The draft's values; assigned over, they show what was typed until the next answer. */
  values = $derived<Record<string, FieldValue>>(Object.fromEntries(this.#snapshot.fields));
  error = $state<string | null>(null);
  /** Pending field overrides are tied to individual commands, not a second saved draft. */
  readonly #pending: Record<string, { value: FieldValue }> = Object.create(null);
  #queue: Promise<void> = Promise.resolve();

  get ready(): Promise<void> { return this.#queue; }

  /** An incoming draft may be older than text already typed in this window. */
  showPending(): void {
    this.values = {
      ...Object.fromEntries(this.#snapshot.fields),
      ...Object.fromEntries(Object.entries(this.#pending).map(([name, edit]) => [name, edit.value])),
    };
  }

  constructor(
    id: string,
    view: FormView,
    readonly ipc: Commands,
    readonly saved: () => Draft | undefined,
    /** Capture where the library's whole answer belongs before the edit leaves the window. */
    readonly answering: () => (draft: ReviewDraft) => Promise<void>,
  ) {
    this.id = id;
    this.verdict = view.verdict;
    this.fields = view.fields;
    this.#snapshot = $derived(this.saved() ?? NO_DRAFT);
  }

  draft(): Draft {
    return {
      verdict: this.verdict,
      fields: this.#snapshot.fields.map(([name]) => [name, this.values[name]] as [string, FieldValue]),
    };
  }

  /** Set field `name`. */
  async set(name: string, value: FieldValue): Promise<void> {
    await this.#setPending(name, value, { kind: "field", verdict: this.verdict, name, value });
  }

  /** Save just the edited child field; the rest of the group stays owned by the library. */
  async setGroupField(name: string, item: number, field: string, value: FieldValue): Promise<void> {
    const groups = this.values[name];
    if (groups?.kind !== "groups" || !groups.value[item]) return;
    const current = groups.value[item];
    const at = current.findIndex(([key]) => key === field);
    const pair: [string, FieldValue] = [field, value];
    const next = at < 0 ? [...current, pair] : current.with(at, pair);
    await this.#setPending(name, { kind: "groups", value: groups.value.with(item, next) }, {
      kind: "groupField", verdict: this.verdict, name, item, field, value,
    });
  }

  async #setPending(name: string, value: FieldValue, change: DraftChange): Promise<void> {
    const edit = { value };
    this.#pending[name] = edit;
    this.showPending();
    await this.#ask(change, () => {
      if (this.#pending[name] === edit) delete this.#pending[name];
    });
  }

  /** Add an empty item to the list field `name`; the library builds it. */
  async add(name: string): Promise<void> {
    await this.#ask({ kind: "addItem", verdict: this.verdict, name });
  }

  /** Remove item `item` of the list field `name`; the library keeps one to type in. */
  async remove(name: string, item: number): Promise<void> {
    await this.#ask({ kind: "removeItem", verdict: this.verdict, name, item });
  }

  #ask(change: DraftChange, settled = () => {}): Promise<void> {
    const wrote = this.#queue.then(async () => {
      const answered = this.answering();
      try {
        const answer = await call(this.ipc.humanDraftEdit(this.id, change));
        this.error = null;
        await answered(answer);
      } catch (error) {
        this.error = message(error);
      } finally {
        settled();
        this.showPending();
      }
    });
    this.#queue = wrote;
    return wrote;
  }
}

/** The review's running work; `tool` marks a tool's run, which a submission cancels first. */
type Busy = { label: string; job: number; since: number; tool: boolean };

/** One canonical request owns these facts. Alias handles adopt this same instance,
 * never copy its forms, drafts or running work. It is shared through AppState. */
class ReviewState {
  id = $state("");
  view = $state.raw<HumanView | null>(null);
  area = $state<Area>("fields");
  tool = $state(0);
  /** Every tool run of this review, oldest first: kept while the review is open. */
  log = $state.raw<LogEntry[]>([]);
  busy = $state<Busy | null>(null);
  /** Settles once the running work has ended, its busy state cleared. */
  idle: Promise<void> = Promise.resolve();
  notice = $state.raw<Notice | null>(null);
  formOpen = $state(false);
  forms = $state.raw<Record<Verdict, FormDraft> | null>(null);
  /** The library's draft of this review for this reviewer, as last read or answered. */
  draft = $state.raw<ReviewDraft | null>(null);
  /** The newest point seen; removing it does not make another point look newly added. */
  latestPoint = $state.raw<ReviewPoint | null>(null);
  draftRead = 0;
  draftReadJob: Promise<void> | null = null;
  draftRevision = 0;
  choice = 0;
  /** The saved draft's verdict was brought back as its open form: once per review shown, so
   * leaving the form is not undone by the next read. */
  restored = false;

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
  readonly form = $derived(this.#state.formOpen && this.draft?.verdict ? this.#state.forms?.[this.draft.verdict] ?? null : null);
  get draft(): ReviewDraft | null { return this.#state.draft; }

  get latestPoint(): ReviewPoint | null { return this.#state.latestPoint; }

  readonly settled = $derived(this.view?.stage === "completed");
  readonly owned = $derived(this.view?.stage === "review");
  readonly claimable = $derived(this.view?.stage === "claim" && this.view.claim === null);
  /** A form takes the keys only while the request can still be reviewed. */
  readonly editing = $derived(this.form !== null && !this.settled);
  /** Submit is open while a form is, also while a tool runs (a submission cancels it first),
   * but not while a claim, a release or a submission runs. */
  readonly submittable = $derived(this.editing && (this.busy === null || this.busy.tool));

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
    // view already replaced, a claim's absence say, and take the open form away until the
    // next read; it is dropped.
    if (this.view && view.read < this.view.read) return;
    this.view = view;
    this.id = view.id;
    this.tool = Math.min(this.tool, Math.max(view.tools.length - 1, 0));
    // Settled here or by another command: nothing is left to edit, and the Tool log is cleared.
    if (view.stage === "completed") {
      this.stopEditing();
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

  /** The library's answer: the draft whole, the open form following it. */
  #take(draft: ReviewDraft): void {
    const state = this.#state;
    this.#saveDraft(draft);
    // A draft saved with a verdict (before a restart, in another window) shows that verdict
    // chosen, its form open with what was entered; nothing is sent (J-HUMAN-DRAFT).
    if (!state.restored && this.owned) {
      state.restored = true;
      state.formOpen = draft.verdict !== null;
    }
  }

  #saveDraft(draft: ReviewDraft): void {
    const state = this.#state;
    state.draftRevision += 1;
    state.draft = draft;
    if (!state.forms || state.forms.GREEN.id !== this.id) {
      const form = (verdict: Verdict) => {
        const view = verdict === "GREEN" ? draft.green : draft.red;
        return new FormDraft(this.id, view, this.ipc, () => (verdict === "GREEN" ? state.draft?.green : state.draft?.red)?.draft, () => this.#answering());
      };
      state.forms = { GREEN: form("GREEN"), RED: form("RED") };
    }
    for (const form of Object.values(state.forms)) form.showPending();
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

  async #job<T>(label: string, work: (job: number) => Promise<T>, tool = false): Promise<T | undefined> {
    if (this.busy) return undefined;
    const job = this.#jobs.next();
    this.busy = { label, job, since: Date.now(), tool };
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
      const acted = await this.#job("Claiming the review", (job) =>
        call(this.ipc.humanClaim(this.id, job)),
      );
      if (acted) this.#acted(acted);
      if (acted && this.owned) await this.readDraft();
    } catch (error) {
      this.notice = { text: message(error), error: true };
    }
  }

  async #fieldsSaved(): Promise<void> {
    const forms = this.#state.forms;
    if (forms) await Promise.all([forms.GREEN.ready, forms.RED.ready]);
  }

  /** Release the claim, also while the form is edited: the form is left, its draft kept. */
  async release(): Promise<void> {
    if (this.busy || this.settled) return;
    try {
      // The form stays while the release runs, its Submit disabled; released, the form is
      // left and its draft kept for the next claim.
      const acted = await this.#job("Releasing the claim", async (job) => {
        await this.#fieldsSaved();
        return call(this.ipc.humanRelease([this.id], job));
      });
      if (acted && this.editing) this.stopEditing();
      if (acted) this.#acted(acted);
    } catch (error) {
      this.notice = { text: message(error), error: true };
    }
  }

  /** Open the form of `verdict`: the library's draft takes the verdict, each verdict keeping
   * what was typed into its form. */
  async choose(verdict: Verdict): Promise<void> {
    if (this.busy || this.settled) return;
    this.area = "fields";
    const state = this.#state;
    const choice = ++state.choice;
    if (this.form?.verdict === verdict) return;
    const answered = this.#answering();
    let answer: ReviewDraft;
    try {
      answer = await call(this.ipc.humanDraftEdit(this.id, { kind: "verdict", verdict }));
    } catch (error) {
      if (state === this.#state && choice === state.choice) this.notice = { text: message(error), error: true };
      return;
    }
    // Another verdict was chosen, or editing stopped, while this form was on its way.
    if (state !== this.#state || choice !== state.choice) return;
    state.restored = true;
    await answered(answer);
    if (state === this.#state && choice === state.choice && this.owned) state.formOpen = true;
  }

  /** Leave the form; the library keeps its draft, and the same verdict reopens it. */
  stopEditing(): void {
    this.#state.choice += 1;
    this.#state.formOpen = false;
  }

  /** Submit the open form; the result says how the settled request stands, or `null` when
   * nothing was settled. */
  async submit(): Promise<Settled | null> {
    // A running tool ends first: submitting settles the review its run belongs to.
    if (this.busy?.tool && this.editing) {
      const idle = this.#state.idle;
      this.cancel();
      await idle;
    }
    const form = this.form;
    if (this.busy || this.settled) return null;
    if (!form) {
      this.notice = { text: "Choose GREEN or RED first.", error: true };
      return null;
    }
    try {
      const submitted = await this.#job(`Submitting ${form.verdict}`, async (job) => {
        // What is typed reaches the library first: it submits the draft it keeps.
        await this.#fieldsSaved();
        return call(this.ipc.humanDraftSubmit(this.id, job));
      });
      if (!submitted) return null;
      form.error = null;
      this.#state.formOpen = false;
      this.#state.draft = null;
      this.log = [];
      this.#acted(submitted);
      return submitted.settled;
    } catch (error) {
      if (error instanceof CommandError && error.failure.kind !== "read") {
        form.error = error.message;
      } else {
        this.notice = { text: message(error), error: true };
      }
      return null;
    }
  }

  /** Settles once no Notes are on their way to the library. */


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
      const notice = await this.#job(`Running ${tool.operation}`, (job) =>
        call(this.ipc.humanRun(this.id, tool.name, job)),
      true);
      if (!notice) return;
      this.notice = notice;
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

  /** Esc closes one thing at a time: the job, then the form; `true` means leave the Detail. */
  escape(): boolean {
    if (this.busy) {
      this.cancel();
      return false;
    }
    if (this.editing) {
      this.stopEditing();
      return false;
    }
    return true;
  }
}
