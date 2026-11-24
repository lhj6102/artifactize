// A Human review as its reviewer reads it, in the main area or in a Detail: a conversation of its
// instruction, its tools, each tool run's line, its Points, its reasons as bubbles, the reasons
// box and the decision bar (where the claim stands, GREEN / RED, why Submit waits, Submit), the
// chosen verdict's own fields and its Debug facts; each action waits for what it shows.
import { clickRowGutter } from "../input.ts";
import { fill, press, typeText } from "../keys.ts";
import { nativeInput } from "../input.ts";
import { chooseOption, holds, until } from "./wait.ts";

/** The review as read: where its claim stands (`Waiting`, `Yours`, `Taken`, "" once submitted),
 * who else claimed it, what its conversation holds in order, the chosen verdict, the completed
 * result, why Submit waits, the request's state under Debug once unfolded, and whether work of
 * it runs. */
export type Shown = {
  stage: string;
  claimant: string;
  /** The conversation's parts in order: `Instruction`, `Tools`, `Debug`, `Runs`, `Points`,
   * `Reasons`, `Result`, then `Claim line` or `Reasons box`, then `Decision`. */
  sections: string[];
  verdict: string;
  completed: string;
  readOnly: boolean;
  running: string;
  /** What went wrong with a claim, a release or another action, in the decision bar. */
  notice: string;
  /** What the library refused of a submission or a field. (A page script's answer may not
   * carry a field named `error`: WebdriverIO reads that as a failed command.) */
  refusal: string;
  /** Why Submit cannot submit yet, as the decision bar says it. */
  why: string;
  submittable: boolean;
  reasons: string[];
  state: string;
  tools: { name: string; enabled: boolean; group: string }[];
};

export class Review {
  private readonly root: string;

  /** The review inside `scope`: the main area's or the Detail's. */
  private readonly browser: WebdriverIO.Browser;

  constructor(browser: WebdriverIO.Browser, scope: "main" | "detail" = "main") {
    this.browser = browser;
    this.root = `${scope === "main" ? `section.stage-area[aria-label="Review's main area"]` : 'aside[aria-label="Detail"]'} section[aria-label="Human review"]`;
  }

  /** The boxes of the conversation's column and of its reasons box and decision bar. */
  panels(): Promise<{ column: { left: number; right: number; top: number; bottom: number }; bar: { left: number; right: number; top: number; bottom: number } } | null> {
    return this.browser.execute((root) => {
      const review = document.querySelector(root);
      const column = review?.querySelector(".column")?.getBoundingClientRect();
      const bar = review?.querySelector('[role="group"][aria-label="Decision"]')?.getBoundingClientRect();
      if (!bar || !column) return null;
      const box = (rect: DOMRect) => ({ left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom });
      return { column: box(column), bar: box(bar) };
    }, this.root);
  }

  read(): Promise<Shown | null> {
    return this.browser.execute((root) => {
      const review = document.querySelector<HTMLElement>(root);
      if (!review) return null;
      const state = [...review.querySelectorAll("dt")].find((term) => term.textContent?.trim() === "State");
      const bar = review.querySelector<HTMLElement>('[role="group"][aria-label="Decision"]');
      const taken = /^Taken by (.+)$/.exec(bar?.querySelector(".said")?.textContent?.trim() ?? "");
      const parts: [string, string][] = [
        ["Instruction", '[aria-label="Instruction"] .text'], ["Tools", '[role="group"][aria-label="Tools"]'], ["Debug", ".debug"],
        ["Runs", ".run"], ["Points", '[aria-label="Points"]'], ["Reasons", '[aria-label="Reasons"]'], ["Result", '[aria-label="Result"]'],
        ["Claim line", ".bottom > .quiet"], ["Reasons box", "[data-reasons]"], ["Decision", '[aria-label="Decision"]'],
      ];
      const submit = bar?.querySelector<HTMLButtonElement>("button.submit");
      const failed = bar?.querySelector(".why.failed")?.textContent?.trim() ?? "";
      return {
        stage: taken ? "Taken" : bar?.querySelector(".stage")?.textContent?.trim() ?? "",
        claimant: taken?.[1] ?? "",
        sections: parts.filter(([, selector]) => review.querySelector(selector)).map(([name]) => name),
        verdict: review.querySelector('[role="radiogroup"][aria-label="Verdict"] [aria-checked="true"]')?.textContent?.trim() ?? "",
        completed: review.querySelector<HTMLElement>('[role="group"][aria-label="Result"]')?.innerText ?? "",
        readOnly: taken !== null,
        running: bar?.querySelector('.busy [aria-label]')?.getAttribute("aria-label")
          ?? review.querySelector('.run button.fold:is([aria-label^="Running "], [aria-label^="Opening "])')?.getAttribute("aria-label") ?? "",
        notice: bar?.querySelector(".claim > .failed")?.textContent?.trim() ?? "",
        refusal: failed || (review.querySelector('[role="alert"]')?.textContent?.trim() ?? ""),
        why: failed ? "" : bar?.querySelector(".why")?.textContent?.trim() ?? "",
        submittable: !!submit && !submit.disabled,
        reasons: [...review.querySelectorAll('[aria-label="Reasons"] .message')].map((reason) => reason.textContent ?? ""),
        state: state?.nextElementSibling?.textContent?.trim() ?? "",
        tools: [...review.querySelectorAll<HTMLButtonElement>('[role="group"][aria-label="Tools"] button')].map((button) => ({
          name: button.textContent?.trim() ?? "", enabled: !button.disabled,
          group: button.closest('[role="group"][aria-label^="Tools of "]')?.getAttribute("aria-label")?.replace(/^Tools of /, "") ?? "",
        })),
      };
    }, this.root);
  }

  /** The review once `ok` holds for it. */
  async until(ok: (shown: Shown) => boolean, what: string, timeout = 15000): Promise<Shown> {
    return (await until(this.browser, () => this.read(), (shown) => !!shown && ok(shown), what, timeout))!;
  }

  /** Wait until Review names `text`: the selected row of the Pane, or a read-only request's head. */
  async titleContains(text: string): Promise<void> {
    // By the Pane's selected row, or, for what the Pane does not list, by the column's head.
    const named = () => this.browser.execute(() => [document.querySelector('[aria-label="Human reviews"] [role="option"][aria-selected="true"]')?.textContent ?? "",
      document.querySelector(`[aria-label="Review's main area"] :is(.record-title, .rtitle)`)?.textContent ?? ""].join(" "));
    await holds(this.browser, async () => (await named()).includes(text), `Review names ${text}`);
  }

  async list(): Promise<{ id: string; name: string }[]> {
    return this.browser.execute(() => [...document.querySelectorAll<HTMLElement>('[aria-label="Human reviews"] [role="option"]')]
      .map((row) => ({ id: row.dataset.key ?? "", name: row.innerText })));
  }

  /** What Review's list says in place of reviews (`No review matches the filter.`), or "". */
  listEmpty(): Promise<string> {
    return this.browser.execute(() => document.querySelector('[aria-label="Human reviews"] li.empty')?.textContent?.trim() ?? "");
  }

  /** The notice in the header row of Review's main area, or "". */
  notice(): Promise<string> {
    return this.browser.execute(() => document.querySelector(`section.stage-area[aria-label="Review's main area"] .notice[role="status"]`)?.textContent?.trim() ?? "");
  }

  filter(): Promise<string> { return this.browser.$('input[aria-label="Filter the reviews"]').getValue(); }

  async filterFor(query: string): Promise<void> {
    await fill(this.browser, 'input[aria-label="Filter the reviews"]', query);
  }

  async openListed(id: string, gutter = false): Promise<void> {
    const row = this.browser.$(`[aria-label="Human reviews"] [data-key=${JSON.stringify(id)}]`);
    await row.waitForDisplayed();
    if (gutter) await clickRowGutter(this.browser, await row.getElement(), 'section[aria-label="Reviews"]');
    else await row.click();
    await holds(this.browser, async () => (await row.getAttribute("aria-selected")) === "true", `${id} selected in Review`);
  }

  hasOutsideHint(): Promise<boolean> {
    return this.browser.execute((root) => /outside the filter/i.test(document.querySelector(root)?.textContent ?? ""), this.root);
  }

  /** The way back, shown for a review followed from Verify: `Verify · {Run} › {Artifact}`. */
  back(): Promise<void> { return this.browser.$(`section.stage-area[aria-label="Review's main area"] > header`).$("button*=Verify · ").click(); }

  /** Whether the stage header leads back to Verify. */
  hasBack(): Promise<boolean> {
    return this.browser.execute(() => [...document.querySelectorAll(`section.stage-area[aria-label="Review's main area"] > header button`)].some((button) => button.textContent?.trim().startsWith("Verify · ")));
  }

  /** Open in Verify, always among the stage header's tools. */
  openInVerify(): Promise<void> { return this.browser.$(`section.stage-area[aria-label="Review's main area"] > header`).$("button=Open in Verify").click(); }

  /** The review's instruction. */
  instruction(): Promise<string> {
    return this.browser.$(this.root).$('[aria-label="Instruction"] .text').getText();
  }

  /** Unfold Debug: the request's state, `Request {id} · {status} · …`. */
  async debugState(): Promise<string> {
    const head = this.browser.$(this.root).$('button[aria-label="Debug (t)"]');
    if ((await head.getAttribute("aria-expanded")) === "false") await head.click();
    return (await this.until((shown) => shown.state !== "", "Debug unfolded")).state;
  }

  /** Click Claim to review: the review is this reviewer's. */
  async claim(): Promise<void> {
    await this.browser.$(this.root).$("button=Claim to review").click();
    await this.until((shown) => shown.stage === "Yours" && !shown.running, "the review claimed");
  }

  /** Click Release claim: the review waits for a claim again. */
  async release(): Promise<void> {
    await this.browser.$(this.root).$("button=Release claim").click();
    await this.until((shown) => shown.stage === "Waiting" && !shown.running, "the claim released");
  }

  /** Press `keys` with the review's instruction holding the keyboard. */
  async press(keys: string[], done: () => Promise<boolean>, what: string): Promise<void> {
    await this.browser.$(this.root).$('[aria-label="Instruction"]').click();
    await press(this.browser, keys, done, what);
  }

  /** The lines of the conversation's tool runs, oldest first, as they read folded:
   * `Ran notes · release exit 0 · 2 lines`. */
  runLines(): Promise<string[]> {
    return this.browser.execute((root) => [...document.querySelectorAll<HTMLElement>(`${root} .run button.fold`)]
      .map((line) => (line.innerText ?? "").replace(/\s+/g, " ").trim()), this.root);
  }

  /** The newest run's line as it reads folded, or "" before any run. */
  async logLine(): Promise<string> {
    return (await this.runLines()).at(-1) ?? "";
  }

  /** Click the newest run line of the tool `name`, as a person does, to unfold its lines. */
  async unfold(name: string): Promise<void> {
    const lines = await this.browser.$$(`${this.root} .run button.fold`).getElements();
    let newest: WebdriverIO.Element | undefined;
    for (const line of lines) if ((await line.$(".target").getText()) === name) newest = line;
    if (!newest || (await newest.getAttribute("aria-expanded")) !== "false") return;
    await newest.click();
    await holds(this.browser, async () => (await newest.getAttribute("aria-expanded")) === "true", `${name}'s run unfolded`);
  }

  /** Click the tool `name`: its run's line joins the conversation, unfolded into its lines. */
  async runTool(name: string): Promise<void> {
    const button = this.browser.$(this.root).$('[role="group"][aria-label="Tools"]').$(`button=${name}`);
    await button.waitForEnabled({ timeout: 15000, timeoutMsg: `the tool ${name} is usable` });
    const count = async () => (await this.browser.execute((root, operation) => [...document.querySelectorAll(`${root} .run .target`)]
      .filter((target) => target.textContent === operation).length, this.root, name));
    const before = await count();
    await button.click();
    await until(this.browser, count, (now) => now > before, `${name}'s run joined the conversation`);
    await this.unfold(name);
  }

  /** Click Cancel beside the running work: it stops. */
  async cancel(): Promise<void> {
    await this.browser.$(this.root).$("button=Cancel").click();
    await this.until((shown) => !shown.running, "the running work cancelled");
  }

  /** The unfolded runs' lines as they read, a run's time as HH:MM:SS, each with whether it is
   * an error line (stderr, a failed end) as the log marks it. */
  log(): Promise<string[]> {
    return this.browser.execute((root) => [...document.querySelectorAll(`${root} [role="log"][aria-label="Tool log"] p:not(.muted)`)]
      .map((row) => `${(row.textContent ?? "").replace(/^▶ \d\d:\d\d:\d\d /, "▶ HH:MM:SS ")}${row.querySelector(".red") || row.matches(".stderr, .end.failed") ? " [red]" : ""}`), this.root);
  }

  /** The lines of the newest run of the tool `name`, from its start line on. */
  async run(name: string): Promise<string[]> {
    const rows = await this.log();
    const at = rows.findLastIndex((row) => row.startsWith(`▶ HH:MM:SS · ${name} ·`));
    if (at < 0) return [];
    const next = rows.findIndex((row, index) => index > at && row.startsWith("▶ "));
    return rows.slice(at, next < 0 ? rows.length : next);
  }

  /** Wait until the newest run of `name`, unfolded, satisfies `ok`. */
  async runUntil(name: string, ok: (rows: string[]) => boolean, what: string): Promise<string[]> {
    await this.unfold(name);
    return until(this.browser, () => this.run(name), ok, what);
  }

  // -------------------------------------------------------------------------------
  // The reasons

  /** Type `text` into the reasons field and add it with Enter: it stands as the newest reason. */
  async addReason(text: string): Promise<void> {
    const before = (await this.read())?.reasons.length ?? 0;
    await fill(this.browser, `${this.root} [data-reasons]`, text);
    await this.browser.$(`${this.root} [data-reasons]`).click();
    await press(this.browser, ["Enter"], async () => ((await this.read())?.reasons.length ?? 0) > before, `${text} added as a reason`);
  }

  /** Press Enter in the reasons field: what it holds is added as the newest reason. */
  async addTyped(): Promise<void> {
    const before = (await this.read())?.reasons.length ?? 0;
    await this.browser.$(`${this.root} [data-reasons]`).click();
    await press(this.browser, ["Enter"], async () => ((await this.read())?.reasons.length ?? 0) > before, "the typed reason added");
  }

  /** Click Edit of reason `at` and replace its text with `text`, Enter keeping it. */
  async editReason(at: number, text: string): Promise<void> {
    await this.browser.$(this.root).$(`button[aria-label="Edit reason ${at + 1}"]`).click();
    await fill(this.browser, `${this.root} textarea[aria-label="Edit reason ${at + 1}"]`, text);
    await press(this.browser, ["Enter"], async () => (await this.read())?.reasons[at] === text, `reason ${at + 1} reads ${text}`);
  }

  /** Click Remove of reason `at`. */
  async removeReason(at: number): Promise<void> {
    const before = (await this.read())?.reasons.length ?? 0;
    await this.browser.$(this.root).$(`button[aria-label="Remove reason ${at + 1}"]`).click();
    await this.until((shown) => shown.reasons.length === before - 1, `reason ${at + 1} removed`);
  }

  // -------------------------------------------------------------------------------
  // The decision and the verdict's own fields

  /** Click GREEN or RED in the decision bar: it is the chosen verdict. */
  async choose(verdict: "GREEN" | "RED"): Promise<void> {
    await this.browser.$(this.root).$('[role="radiogroup"][aria-label="Verdict"]').$(`button*=${verdict}`).click();
    await this.until((shown) => shown.verdict === verdict, `${verdict} chosen`);
  }

  /** A field by `id`: `reasons` is the reasons field, any other a verdict's own field
   * (`score`, `findings-0-problem`). */
  private field(id: string): string {
    return id === "reasons" ? `${this.root} [data-reasons]` : `${this.root} #field-${id}`;
  }

  /** The value of the field `id`. */
  value(id: string): Promise<string> {
    return this.browser.$(this.field(id)).getValue();
  }

  /** Type `text` into the field `id`: it shows it. */
  async enter(id: string, text: string): Promise<void> {
    await fill(this.browser, this.field(id), text);
  }

  /** Click into the field `id` and type `text` at its end, a key at a time. */
  async typeInto(id: string, text: string): Promise<void> {
    const before = await this.value(id);
    await this.browser.$(this.field(id)).click();
    await typeText(this.browser, text);
    await until(this.browser, () => this.value(id), (value) => value === before + text, `${text} typed into ${id}`);
  }

  /** Type `text` into the `at`-th item of the list field `name`. */
  async enterItem(name: string, at: number, text: string): Promise<void> {
    await fill(this.browser, `${this.root} textarea[aria-label="${name} ${at + 1}"]`, text);
  }

  /** Choose `option` of the choice field `id`. */
  async select(id: string, option: string): Promise<void> {
    await chooseOption(this.browser, `${this.root} #field-${id}`, option);
  }


  /** Click the true/false field `id`'s `option`. */
  async check(id: string, option: "true" | "false"): Promise<void> {
    const choice = this.browser.$(`${this.root} [aria-labelledby="field-${id}"], ${this.root} #field-${id} + .choices`).$(`label*=${option}`).$("input");
    await choice.click();
    await holds(this.browser, () => choice.isSelected(), `${id} is ${option}`);
  }

  /** Click `+ Add` of the field `name`: one more item or group. */
  async add(name: string): Promise<void> {
    const before = await this.items(name);
    await this.browser.$(`${this.root} #field-${name} + button.add`).click();
    await until(this.browser, () => this.items(name), (count) => count === before + 1, `one more ${name}`);
  }

  /** Click `✕` of the `at`-th item or group of the field `name`. */
  async remove(name: string, at: number): Promise<void> {
    const before = await this.items(name);
    await this.browser.$(this.root).$(`button[aria-label="Remove ${name} ${at + 1}"]`).click();
    await until(this.browser, () => this.items(name), (count) => count === before - 1, `one ${name} fewer`);
  }

  /** How many items or groups the field `name` has. */
  items(name: string): Promise<number> {
    return this.browser.execute((root, field) => document.querySelectorAll(`${root} #field-${field} > li, ${root} #field-${field} > .group`).length, this.root, name);
  }

  /** The labels of the form's fields, as they read (a required one with `*`), and each field's
   * placeholder or hint as it reads (`Optional`). */
  fields(): Promise<{ label: string; hint: string }[]> {
    return this.browser.execute((root) => [...document.querySelectorAll<HTMLElement>(`${root} .form > .field`)].map((field) => ({
      label: field.querySelector(".name")?.textContent?.trim() ?? "",
      hint: [field.querySelector<HTMLInputElement | HTMLTextAreaElement>("input:not([type]), textarea")?.placeholder ?? "",
        field.querySelector("select option:checked")?.textContent?.trim() ?? "", field.querySelector(".optional")?.textContent?.trim() ?? ""]
        .filter(Boolean).join(" "),
    })), this.root);
  }

  /** Click Submit once it can submit: the review completes, or says what the library refused. */
  async submit(): Promise<Shown> {
    await this.until((shown) => shown.submittable, "Submit can submit");
    await this.browser.$(this.root).$('[role="group"][aria-label="Decision"]').$("button*=Submit").click();
    return this.until((shown) => !shown.running && (shown.completed !== "" || shown.refusal !== ""), "the submission completed or refused");
  }

  /** Compose `text` into the field `id` through the engine's own input method (the test-only
   * input device), as a person's does: the field first shows `composing`, the text still being
   * composed, which differs from `text`, and then `text` in its place once it is committed. */
  async compose(id: string, composing: string, text: string): Promise<void> {
    await this.browser.$(this.field(id)).click();
    const before = await this.value(id);
    await nativeInput(this.browser, { type: "set", text: composing });
    await holds(this.browser, async () => (await this.value(id)) === before + composing, `${composing} being composed in ${id}`);
    await nativeInput(this.browser, { type: "commit", text });
    await holds(this.browser, async () => (await this.value(id)) === before + text, `${text} committed into ${id} in place of ${composing}`);
  }
}
