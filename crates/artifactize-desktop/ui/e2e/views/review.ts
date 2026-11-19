// A Human review as its reviewer reads it, in the main area or in a Detail: its stage, its
// instruction, its tools and their Tool log, its result form and its Debug facts; each action
// waits for what it shows.
import { fill, press, typeText } from "../keys.ts";
import { nativeInput } from "../input.ts";
import { chooseOption, holds, until } from "./wait.ts";

/** The review as read: its stage (`CLAIM`, `REVIEW (yours)`), who else claimed it, what its
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
    this.root = `${scope === "main" ? 'section.panel[aria-label="Main area"]' : 'aside[aria-label="Detail"]'} section[aria-label="Human review"]`;
  }

  read(): Promise<Shown | null> {
    return this.browser.execute((root) => {
      const review = document.querySelector<HTMLElement>(root);
      if (!review) return null;
      const state = [...review.querySelectorAll("dt")].find((term) => term.textContent?.trim() === "State");
      return {
        stage: review.querySelector(".stage")?.textContent?.trim() ?? "",
        claimant: review.querySelector(".claimant")?.textContent?.trim() ?? "",
        sections: [...review.querySelectorAll(".instruction, section.section")].map((part) => part.getAttribute("aria-label") ?? ""),
        verdict: review.querySelector('[role="radiogroup"][aria-label="Verdict"] [aria-checked="true"]')?.textContent?.trim() ?? "",
        completed: review.querySelector<HTMLElement>('section[aria-label="Result"] pre')?.innerText ?? "",
        readOnly: (review.textContent ?? "").includes("Read-only until released."),
        running: review.querySelector('.busy [role="img"], .busy [aria-label]')?.getAttribute("aria-label") ?? "",
        notice: review.querySelector('section[aria-label="Result"] .said [role="status"]:not(.busy)')?.textContent?.trim() ?? "",
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
    await this.until((shown) => shown.stage === "REVIEW (yours)" && !shown.running, "the review claimed");
  }

  /** Click Release claim: the review waits for a claim again. */
  async release(): Promise<void> {
    await this.browser.$(this.root).$("button=Release claim").click();
    await this.until((shown) => shown.stage === "CLAIM" && !shown.running, "the claim released");
  }

  /** Press `keys` with the review's instruction holding the keyboard. */
  async press(keys: string[], done: () => Promise<boolean>, what: string): Promise<void> {
    await this.browser.$(this.root).$('[aria-label="Instruction"]').click();
    await press(this.browser, keys, done, what);
  }

  /** Click the tool `name`: its run starts in the Tool log. */
  async runTool(name: string): Promise<void> {
    const button = this.browser.$(this.root).$('[role="group"][aria-label="Tools"]').$(`button=${name}`);
    await button.waitForEnabled({ timeout: 15000, timeoutMsg: `the tool ${name} is usable` });
    const before = (await this.log()).filter((row) => row.startsWith(`▶ HH:MM:SS · ${name} ·`)).length;
    await button.click();
    await until(this.browser, () => this.log(), (rows) => rows.filter((row) => row.startsWith(`▶ HH:MM:SS · ${name} ·`)).length > before, `${name}'s run started in the Tool log`);
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
      .map((row) => `${(row.textContent ?? "").replace(/^▶ \d\d:\d\d:\d\d /, "▶ HH:MM:SS ")}${row.classList.contains("error") || row.classList.contains("stderr") ? " [red]" : ""}`), this.root);
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

  /** Paste `text` into the form's field `id` through the engine's own editing (the test-only
   * input device), as a person's input method composes it: it is committed into the field. */
  async compose(id: string, text: string): Promise<void> {
    await this.browser.$(`${this.root} #field-${id}`).click();
    await nativeInput(this.browser, { type: "set", text });
    await nativeInput(this.browser, { type: "commit", text });
    await holds(this.browser, async () => (await this.value(id)).endsWith(text), `${text} composed into ${id}`);
  }
}
