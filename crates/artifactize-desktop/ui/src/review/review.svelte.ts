// One request in the shared Human Detail: what it shows, which area has focus, the GREEN and
// RED drafts and the running job. Every write goes through a backend job; the request's state
// is whatever the backend last read.
import { CommandError, call, commands, message, nextJob } from "../ipc/client";
import type {
  Acted,
  Draft,
  FieldValue,
  FormField,
  FormView,
  HumanView,
  Notice,
  Output,
  Settled,
  Verdict,
} from "../ipc/client";

export type Area = "instruction" | "tools" | "fields";

const CLAIM_FIRST = "Claim this request before reviewing it.";

/** A field's empty draft value, by how it is entered. */
function initial(field: FormField): FieldValue {
  switch (field.input.kind) {
    case "fixed":
      return { kind: "fixed" };
    case "bool":
      return { kind: "bool", value: null };
    case "choice":
      return { kind: "choice", value: null };
    case "text":
      return { kind: "text", value: "" };
    case "integer":
      return { kind: "integer", value: "" };
    case "number":
      return { kind: "number", value: "" };
  }
}

/** The draft of one verdict; it survives switching verdicts and leaving the Detail. */
export class FormDraft {
  readonly verdict: Verdict;
  readonly fields: FormField[];
  values = $state<Record<string, FieldValue>>({});
  json = $state<string | null>(null);
  error = $state<string | null>(null);

  constructor(view: FormView) {
    this.verdict = view.verdict;
    this.fields = view.fields;
    this.json = view.json;
    this.values = Object.fromEntries(view.fields.map((field) => [field.name, initial(field)]));
  }

  draft(): Draft {
    return {
      verdict: this.verdict,
      fields: Object.entries($state.snapshot(this.values)) as [string, FieldValue][],
      json: this.json,
    };
  }
}

type Busy = { label: string; job: number; since: number };

export class Review {
  /** The request every action goes to; a follower's ID becomes its original's once read. */
  id = $state("");
  view = $state<HumanView | null>(null);
  area = $state<Area>("fields");
  /** The instruction takes most of the body (`i`); focus alone never resizes the Detail. */
  expanded = $state(false);
  technical = $state(false);
  tool = $state(0);
  output = $state<Output | null>(null);
  busy = $state<Busy | null>(null);
  notice = $state<Notice | null>(null);
  form = $state<FormDraft | null>(null);
  #drafts = new Map<Verdict, FormDraft>();
  /** Counts verdict choices: a form that arrives after a newer choice is not shown. */
  #choice = 0;

  readonly settled = $derived(this.view?.stage === "completed");
  readonly owned = $derived(this.view?.stage === "review");
  readonly claimable = $derived(this.view?.stage === "claim" && this.view.claim === null);
  /** A form takes the keys only while the request can still be reviewed. */
  readonly editing = $derived(this.form !== null && !this.settled);

  /** The standalone review offers its claims for release on quit; the monitor keeps them. */
  readonly standalone: boolean;

  constructor(id: string, standalone: boolean) {
    this.id = id;
    this.standalone = standalone;
  }

  /** Read the request (or, without `reload`, redraw its waiting time from the last read). */
  async load(reload = true): Promise<void> {
    try {
      this.#show(await call(commands.humanOpen(this.id, reload)));
    } catch (error) {
      this.notice = { text: message(error), error: true };
    }
  }

  #show(view: HumanView): void {
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
    const job = nextJob();
    this.busy = { label, job, since: Date.now() };
    try {
      return await work(job);
    } finally {
      this.busy = null;
    }
  }

  /** Claim explicitly; tools and submission never claim. */
  async claim(): Promise<void> {
    if (this.busy || this.settled) return;
    if (!this.claimable) {
      if (this.view?.claim && !this.view.claim.mine) {
        this.notice = {
          text: `Claimed by ${this.view.claim.reviewer}; read-only for ${this.view.reviewer}.`,
          error: true,
        };
      }
      return;
    }
    try {
      const acted = await this.#job("Claiming the review", (job) =>
        call(commands.humanClaim(this.id, job, this.standalone)),
      );
      if (acted) this.#acted(acted);
    } catch (error) {
      this.notice = { text: message(error), error: true };
    }
  }

  async release(): Promise<void> {
    if (this.busy || this.settled) return;
    if (!this.owned) {
      this.notice = { text: "The request is not claimed.", error: true };
      return;
    }
    try {
      const acted = await this.#job("Releasing the claim", (job) =>
        call(commands.humanRelease([this.id], job)),
      );
      if (acted) this.#acted(acted);
    } catch (error) {
      this.notice = { text: message(error), error: true };
    }
  }

  /** Open the form of `verdict`, keeping the other verdict's draft. */
  async choose(verdict: Verdict): Promise<void> {
    if (this.busy || this.settled) return;
    if (!this.owned) {
      this.notice = { text: CLAIM_FIRST, error: true };
      return;
    }
    this.area = "fields";
    const choice = ++this.#choice;
    if (this.form?.verdict === verdict) return;
    let draft = this.#drafts.get(verdict);
    if (!draft) {
      try {
        draft = new FormDraft(await call(commands.humanForm(this.id, verdict)));
      } catch (error) {
        if (choice === this.#choice) this.notice = { text: message(error), error: true };
        return;
      }
      // Another verdict was chosen, or editing stopped, while this form was on its way.
      if (choice !== this.#choice) return;
    }
    this.#drafts.delete(verdict);
    if (this.form) this.#drafts.set(this.form.verdict, this.form);
    this.form = draft;
  }

  /** Leave the form with its draft kept; the same verdict reopens it. */
  stopEditing(): void {
    this.#choice += 1;
    if (this.form) this.#drafts.set(this.form.verdict, this.form);
    this.form = null;
  }

  /** Edit the open typed form as owner-fields JSON, as the terminal review does in `$EDITOR`;
   * the JSON stays this verdict's draft. Values typed while the conversion was on its way are
   * converted again, never dropped. */
  async editJson(): Promise<void> {
    const form = this.form;
    if (this.busy || !form || form.json !== null) return;
    try {
      for (;;) {
        const draft = form.draft();
        const json = await call(commands.humanJson(this.id, draft));
        if (this.form !== form || form.json !== null) return;
        if (JSON.stringify(form.draft()) === JSON.stringify(draft)) {
          form.json = json;
          return;
        }
      }
    } catch (error) {
      form.error = message(error);
    }
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
    if (!this.owned) {
      form.error = CLAIM_FIRST;
      return null;
    }
    try {
      const submitted = await this.#job(`Submitting ${form.verdict}`, (job) =>
        call(commands.humanSubmit(this.id, form.draft(), job)),
      );
      if (!submitted) return null;
      form.error = null;
      this.form = null;
      this.#drafts.clear();
      this.output = null;
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

  /** Run a tool at once; the backend checks the claim when it starts. */
  async run(index = this.tool): Promise<void> {
    if (this.busy || this.settled) return;
    const tool = this.view?.tools[index];
    if (!tool) {
      this.notice = { text: "This request declares no Human tools.", error: true };
      return;
    }
    this.tool = index;
    if (!this.owned) {
      this.notice = { text: CLAIM_FIRST, error: true };
      return;
    }
    try {
      const ran = await this.#job(`Running ${tool.name}`, (job) =>
        call(commands.humanRun(this.id, tool.name, job)),
      );
      if (!ran) return;
      this.output = ran.output;
      this.notice = ran.notice;
      // Declarations may have changed; show what the next run resolves.
      await this.load();
    } catch (error) {
      this.notice = { text: message(error), error: true };
    }
  }

  cancel(): void {
    if (this.busy) void commands.humanCancel(this.busy.job);
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
