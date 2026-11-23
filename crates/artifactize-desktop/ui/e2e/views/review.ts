// A Human review as its reviewer reads it, in the main area or in a Detail: its stage, its
// instruction, its tools and their Tool log, its result form and its Debug facts; each action
// waits for what it shows.
import { clickRowGutter } from "../input.ts";
import { fill, press, typeText } from "../keys.ts";
import { nativeInput } from "../input.ts";
import { chooseOption, holds, until } from "./wait.ts";

/** The review as read: where its claim stands (`Waiting`, `Yours`, `Taken`), who else claimed it, what its
 * sections are in order, what its Result says (a form's verdict, or the completed result),
 * the request's state under Debug once unfolded, and whether work of it runs. */
export type Shown = {
  stage: string;
  claimant: string;
  sections: string[];
  verdict: string;
  completed: string;
  readOnly: boolean;
  running: string;
  notice: string;
  /** What the form says is wrong with its result. (A page script's answer may not carry a
   * field named `error`: WebdriverIO reads that as a failed command.) */
  refusal: string;
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

  /** The boxes of the Tool log and of the column holding the instruction, Tools and Result. */
  panels(): Promise<{ log: { left: number; right: number; top: number; bottom: number }; column: { left: number; right: number; top: number; bottom: number } } | null> {
    return this.browser.execute((root) => {
      const review = document.querySelector(root);
      const log = review?.querySelector('[role="log"][aria-label="Tool log"]')?.getBoundingClientRect();
      const instruction = review?.querySelector(".instruction");
      const column = (instruction?.closest(".column") ?? null)?.getBoundingClientRect();
      if (!log || !column) return null;
      const box = (rect: DOMRect) => ({ left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom });
      return { log: box(log), column: box(column) };
    }, this.root);
  }

  read(): Promise<Shown | null> {
    return this.browser.execute((root) => {
      const review = document.querySelector<HTMLElement>(root);
      if (!review) return null;
      const state = [...review.querySelectorAll("dt")].find((term) => term.textContent?.trim() === "State");
      return {
        stage: (review.querySelector(".rstate")?.textContent?.trim() ?? "").split(" · ")[0],
        claimant: (review.querySelector(".rstate")?.textContent?.trim() ?? "").split(" · ")[1] ?? "",
        sections: [...review.querySelectorAll(".instruction, section.section")].map((part) => part.getAttribute("aria-label") ?? ""),
        verdict: review.querySelector('[role="radiogroup"][aria-label="Verdict"] [aria-checked="true"]')?.textContent?.trim() ?? "",
        completed: review.querySelector<HTMLElement>('section[aria-label="Verdict"] .completed')?.innerText ?? "",
        readOnly: (review.textContent ?? "").includes("Taken by "),
        running: review.querySelector('.busy [role="img"], .busy [aria-label]')?.getAttribute("aria-label") ?? "",
        notice: review.querySelector(':is(.said, .result) > [role="status"]:not(.busy)')?.textContent?.trim() ?? "",
        refusal: review.querySelector('[role="alert"]')?.textContent?.trim() ?? "",
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
    // A Human review by its selected row; a request not listed there by its column's head.
    const named = () => this.browser.execute(() => [document.querySelector('[aria-label="Human reviews"] [role="option"][aria-selected="true"]')?.textContent ?? "",
      document.querySelector(`[aria-label="Review's main area"] .record-title`)?.textContent ?? ""].join(" "));
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

  back(): Promise<void> { return this.browser.$(`section.stage-area[aria-label="Review's main area"] > header`).$("button*=Verify").click(); }

  /** The review's instruction. */
  instruction(): Promise<string> {
    return this.browser.$(this.root).$('[aria-label="Instruction"] .text').getText();
  }

  /** Unfold Debug: the request's state, `Request {id} · {status} · …`. */
  async debugState(): Promise<string> {
    const head = this.browser.$(this.root).$("button.head=Debug");
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

  /** Unfold the Tool log from its folded line, once a tool has run. */
  async showFullLog(): Promise<void> {
    const line = this.browser.$(this.root).$('button[aria-label="Tool log"]');
    await line.waitForDisplayed({ timeout: 15000, timeoutMsg: "the Tool log's line shows once a tool ran" });
    if (await line.getAttribute("aria-expanded") === "false") await line.click();
    await holds(this.browser, async () => await line.getAttribute("aria-expanded") === "true", "the Tool log unfolded");
  }

  /** The Tool log's folded line as it reads, or "" before any run. */
  logLine(): Promise<string> {
    return this.browser.execute((root) => (document.querySelector<HTMLElement>(`${root} button[aria-label="Tool log"]`)?.innerText ?? "").replace(/\s+/g, " ").trim(), this.root);
  }

  /** Click the tool `name`: its run starts in the Tool log. */
  async runTool(name: string): Promise<void> {
    const button = this.browser.$(this.root).$('[role="group"][aria-label="Tools"]').$(`button=${name}`);
    await button.waitForEnabled({ timeout: 15000, timeoutMsg: `the tool ${name} is usable` });
    const prior = await this.log();
    const head = `▶ HH:MM:SS · ${name} ·`;
    const before = prior.filter((row) => row.startsWith(head)).length;
    await button.click();
    await this.showFullLog();
    await until(this.browser, () => this.log(), (rows) => rows.filter((row) => row.startsWith(head)).length > before || (rows.some((row) => row.startsWith(head)) && JSON.stringify(rows) !== JSON.stringify(prior)), `${name}'s run started in the Tool log`);
  }

  /** Click Cancel beside the running work: it stops. */
  async cancel(): Promise<void> {
    await this.browser.$(this.root).$("button=Cancel").click();
    await this.until((shown) => !shown.running, "the running work cancelled");
  }

  /** The Tool log's rows as they read, a run's time as HH:MM:SS, each with whether it is an
   * error line (stderr, a failed end) as the log marks it. */
  log(): Promise<string[]> {
    return this.browser.execute((root) => [...document.querySelectorAll(`${root} [role="log"][aria-label="Tool log"] p:not(.muted)`)]
      .map((row) => `${(row.textContent ?? "").replace(/^▶ \d\d:\d\d:\d\d /, "▶ HH:MM:SS ")}${row.querySelector(".red") || row.matches(".stderr, .end.failed") ? " [red]" : ""}`), this.root);
  }

  /** The rows of the last run of the tool `name`, from its start line on. */
  async run(name: string): Promise<string[]> {
    const rows = await this.log();
    const at = rows.findLastIndex((row) => row.startsWith(`▶ HH:MM:SS · ${name} ·`));
    if (at < 0) return [];
    const next = rows.findIndex((row, index) => index > at && row.startsWith("▶ "));
    return rows.slice(at, next < 0 ? rows.length : next);
  }

  /** Wait until the last run of `name` satisfies `ok`. */
  async runUntil(name: string, ok: (rows: string[]) => boolean, what: string): Promise<string[]> {
    return until(this.browser, () => this.run(name), ok, what);
  }

  // -------------------------------------------------------------------------------
  // The result form

  /** Click GREEN or RED: that verdict's form shows. */
  async choose(verdict: "GREEN" | "RED"): Promise<void> {
    // The verdict's button before a form is open, its radio in the open form.
    await this.browser.$(this.root).$(`button=${verdict}`).click();
    await this.until((shown) => shown.verdict === verdict, `the ${verdict} form shown`);
  }

  /** The value of the form's field `id` (`comment`, `findings-0-problem`). */
  value(id: string): Promise<string> {
    return this.browser.$(`${this.root} #field-${id}`).getValue();
  }

  /** Type `text` into the form's field `id`: it shows it. */
  async enter(id: string, text: string): Promise<void> {
    await fill(this.browser, `${this.root} #field-${id}`, text);
  }

  /** Click into the form's field `id` and type `text` at its end, a key at a time. */
  async typeInto(id: string, text: string): Promise<void> {
    const before = await this.value(id);
    await this.browser.$(`${this.root} #field-${id}`).click();
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

  /** Click Submit: the review completes, or shows why the result fails its schema. */
  async submit(): Promise<Shown> {
    await this.browser.$(this.root).$("button*=Submit").click();
    return this.until((shown) => !shown.running && (shown.completed !== "" || shown.refusal !== ""), "the submission completed or refused");
  }

  /** Compose `text` into the form's field `id` through the engine's own input method (the
   * test-only input device), as a person's does: the field first shows `composing`, the text
   * still being composed, which differs from `text`, and then `text` in its place once it is
   * committed. */
  async compose(id: string, composing: string, text: string): Promise<void> {
    await this.browser.$(`${this.root} #field-${id}`).click();
    const before = await this.value(id);
    await nativeInput(this.browser, { type: "set", text: composing });
    await holds(this.browser, async () => (await this.value(id)) === before + composing, `${composing} being composed in ${id}`);
    await nativeInput(this.browser, { type: "commit", text });
    await holds(this.browser, async () => (await this.value(id)) === before + text, `${text} committed into ${id} in place of ${composing}`);
  }
}
