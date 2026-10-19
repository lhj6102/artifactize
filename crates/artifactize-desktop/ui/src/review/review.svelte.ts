// One request in the shared Human Detail: what it shows, which area has focus, the GREEN and
// RED drafts and the running job. Every write goes through a backend job; the request's state
// is whatever the backend last read.
import { SvelteMap } from "svelte/reactivity";
import type { Jobs } from "../ipc/jobs.svelte";
import { CommandError, call, commands, message, type Commands } from "../ipc/client";
import type {
  Acted,
  Draft,
  FieldValue,
  FormField,
  FormView,
  HumanView,
  LogEntry,
  Notice,
  Settled,
  Verdict,
} from "../ipc/client";

export type Area = "instruction" | "tools" | "fields";

const NO_DRAFT: Draft = { verdict: "GREEN", fields: [] };

/** The draft of one verdict; it survives switching verdicts and leaving the Detail. The library
 * owns what an edit means: every edit is sent to it and its answer replaces the draft whole. A
 * typed value shows at once as a temporary override of the values derived from that draft, and
 * is rolled back if the library refuses it. */
export class FormDraft {
  readonly id: string;
  readonly verdict: Verdict;
  readonly fields: FormField[];
  /** The library's draft. */
  snapshot = $state.raw<Draft>(NO_DRAFT);
  /** The draft's values; assigned over, they show what was typed until the next answer. */
  values = $derived<Record<string, FieldValue>>(Object.fromEntries(this.snapshot.fields));
  error = $state<string | null>(null);
  /** Counts edits: only the newest answer replaces the draft. */
  #edits = 0;

  constructor(id: string, view: FormView, readonly ipc: Commands = commands) {
    this.id = id;
    this.verdict = view.verdict;
    this.fields = view.fields;
    this.snapshot = view.draft;
  }

  draft(): Draft {
    return {
      verdict: this.verdict,
      fields: this.snapshot.fields.map(([name]) => [name, this.values[name]] as [string, FieldValue]),
    };
  }

  /** Set field `name`. */
  async set(name: string, value: FieldValue): Promise<void> {
    this.values = { ...this.values, [name]: value };
    await this.#ask((draft) => call(this.ipc.humanEditField(this.id, draft, name, value)));
  }

  /** Add an empty item to the list field `name`; the library builds it. */
  async add(name: string): Promise<void> {
    await this.#ask((draft) => call(this.ipc.humanAddItem(this.id, draft, name)));
  }

  /** Remove item `item` of the list field `name`; the library keeps one to type in. */
  async remove(name: string, item: number): Promise<void> {
    await this.#ask((draft) => call(this.ipc.humanRemoveItem(this.id, draft, name, item)));
  }

  async #ask(edit: (draft: Draft) => Promise<Draft>): Promise<void> {
    const mine = ++this.#edits;
    try {
      const answer = await edit(this.draft());
      if (mine === this.#edits) this.snapshot = answer;
    } catch (error) {
      if (mine === this.#edits) {
        // Rolled back: a new snapshot object derives the values from the draft again.
        this.snapshot = { ...this.snapshot };
        this.error = message(error);
      }
    }
  }
}

type Busy = { label: string; job: number; since: number };

/** One canonical request owns these facts. Alias handles adopt this same instance,
 * never copy its forms, drafts or running work. It is shared through AppState. */
class ReviewState {
  id = $state("");
  view = $state.raw<HumanView | null>(null);
  area = $state<Area>("fields");
  expanded = $state(false);
  technical = $state(false);
  tool = $state(0);
  /** Every tool run of this review, oldest first: kept while the review is open. */
  log = $state.raw<LogEntry[]>([]);
  busy = $state<Busy | null>(null);
  notice = $state<Notice | null>(null);
  form = $state<FormDraft | null>(null);
  readonly drafts = new SvelteMap<Verdict, Draft>();
  choice = 0;

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
  get expanded(): boolean { return this.#state.expanded; }
  set expanded(value: boolean) { this.#state.expanded = value; }
  get technical(): boolean { return this.#state.technical; }
  set technical(value: boolean) { this.#state.technical = value; }
  get tool(): number { return this.#state.tool; }
  set tool(value: number) { this.#state.tool = value; }
  get log(): LogEntry[] { return this.#state.log; }
  set log(value: LogEntry[]) { this.#state.log = value; }
  get busy(): Busy | null { return this.#state.busy; }
  set busy(value: Busy | null) { this.#state.busy = value; }
  get notice(): Notice | null { return this.#state.notice; }
  set notice(value: Notice | null) { this.#state.notice = value; }
  get form(): FormDraft | null { return this.#state.form; }
  set form(value: FormDraft | null) { this.#state.form = value; }

  readonly settled = $derived(this.view?.stage === "completed");
  readonly owned = $derived(this.view?.stage === "review");
  readonly claimable = $derived(this.view?.stage === "claim" && this.view.claim === null);
  /** A form takes the keys only while the request can still be reviewed. */
  readonly editing = $derived(this.form !== null && !this.settled);

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

  #show(view: HumanView): void {
    const canonical = this.#intern?.(view.id, this) ?? this;
    if (canonical !== this) this.#state = canonical.#state;
    this.view = view;
    this.id = view.id;
    this.tool = Math.min(this.tool, Math.max(view.tools.length - 1, 0));
    // Settled here or by another command: nothing is left to edit.
    if (view.stage === "completed") this.stopEditing();
  }

  #acted(acted: Pick<Acted, "notice" | "view">): void {
    this.notice = acted.notice;
    if (acted.view) this.#show(acted.view);
  }

  async #job<T>(label: string, work: (job: number) => Promise<T>): Promise<T | undefined> {
    if (this.busy) return undefined;
    const job = this.#jobs.next();
    this.busy = { label, job, since: Date.now() };
    try {
      return await work(job);
    } finally {
      if (this.busy?.job === job) this.busy = null;
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
    } catch (error) {
      this.notice = { text: message(error), error: true };
    }
  }

  async release(): Promise<void> {
    if (this.busy || this.settled) return;
    try {
      const acted = await this.#job("Releasing the claim", (job) =>
        call(this.ipc.humanRelease([this.id], job)),
      );
      if (acted) this.#acted(acted);
    } catch (error) {
      this.notice = { text: message(error), error: true };
    }
  }

  /** Open the form of `verdict`, keeping the other verdict's draft. */
  async choose(verdict: Verdict): Promise<void> {
    if (this.busy || this.settled) return;
    this.area = "fields";
    const state = this.#state;
    const choice = ++state.choice;
    if (this.form?.verdict === verdict) return;
    let draft: FormDraft;
    try {
      draft = new FormDraft(this.id, await call(this.ipc.humanForm(this.id, verdict, this.#state.drafts.get(verdict) ?? null)), this.ipc);
    } catch (error) {
      if (state === this.#state && choice === state.choice) this.notice = { text: message(error), error: true };
      return;
    }
    // Another verdict was chosen, or editing stopped, while this form was on its way.
    if (state !== this.#state || choice !== state.choice) return;
    this.#state.drafts.delete(verdict);
    if (this.form) this.#state.drafts.set(this.form.verdict, this.form.draft());
    this.form = draft;
  }

  /** Leave the form with its draft kept; the same verdict reopens it. */
  stopEditing(): void {
    this.#state.choice += 1;
    if (this.form) this.#state.drafts.set(this.form.verdict, this.form.draft());
    this.form = null;
  }

  /** Submit the open form; the result says how the settled request stands, or `null` when
   * nothing was settled. */
  async submit(): Promise<Settled | null> {
    const form = this.form;
    if (this.busy || this.settled) return null;
    if (!form) {
      this.notice = { text: "Choose GREEN or RED first.", error: true };
      return null;
    }
    try {
      const submitted = await this.#job(`Submitting ${form.verdict}`, (job) =>
        call(this.ipc.humanSubmit(this.id, form.draft(), job)),
      );
      if (!submitted) return null;
      form.error = null;
      this.form = null;
      this.#state.drafts.clear();
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
      );
      if (!notice) return;
      this.notice = notice;
      // Declarations may have changed; show what the next run resolves.
      await this.load();
    } catch (error) {
      this.notice = { text: message(error), error: true };
    }
  }

  /** This review's Tool log, whole, as the backend sends it after each change. */
  logged(entries: LogEntry[]): void {
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
