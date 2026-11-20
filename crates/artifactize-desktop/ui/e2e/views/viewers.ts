// The app's windows beside the main one, as a person sees them: an Artifact Viewer (its strip,
// File Explorer, Markdown, Artifactize Viewer, a plugin's page) and the pet overlay; and, in the
// main window, what leads to them: Review's list and review, its Notes and the bottom bar's
// modes. WebDriver reaches one window at a time: each view switches to its window first.
import { fill } from "../keys.ts";
import { callWindow } from "../portable.ts";
import { holds, until } from "./wait.ts";

const MAIN = "main";
const PET = "pet";
const VIEWER = "viewer-";
const REVIEW = `[aria-label="Review's main area"] section[aria-label="Human review"]`;

/** The pet overlay as read: its head row, its Notes, the verdict whose form is open, Point and
 * why it is off, and what it says went wrong. */
export type Pet = { head: string; notes: string; verdict: string; point: boolean; why: string; refusal: string };

export class Windows {
  private readonly browser: WebdriverIO.Browser;

  constructor(browser: WebdriverIO.Browser) {
    this.browser = browser;
  }

  /** The window labels open now. */
  private handles(): Promise<string[]> {
    return this.browser.getWindowHandles();
  }

  /** Reach the window whose label `matches` once it is open; its label. */
  private async reach(matches: (label: string) => boolean, what: string): Promise<string> {
    const label = await until(this.browser, async () => (await this.handles()).find(matches) ?? "", (found) => found !== "", what);
    await this.browser.switchToWindow(label);
    await holds(this.browser, () => this.browser.execute(() => (document.getElementById("app")?.childElementCount ?? 0) > 0), `${label} drew its page`);
    return label;
  }

  /** Back to the main window. */
  async main(): Promise<void> {
    await this.browser.switchToWindow(MAIN);
  }

  /** The Artifact Viewer windows open now. */
  async viewers(): Promise<string[]> {
    return (await this.handles()).filter((label) => label.startsWith(VIEWER));
  }

  // -------------------------------------------------------------------------------
  // The main window

  /** Click the bottom bar's mode `name`: its workspace shows. */
  async mode(name: "Chat" | "Verify" | "Review"): Promise<void> {
    await this.main();
    await this.browser.$('nav[aria-label="Modes"]').$(`button*=${name}`).click();
    await holds(this.browser, () => this.browser.execute((shown) => document.querySelector(`main[aria-label="${shown}"]`)?.hasAttribute("hidden") === false, name), `${name} shown`);
  }

  /** Open the review of `evalId` from Review's list, and claim it: its Notes show. */
  async claimReview(evalId: string): Promise<void> {
    await this.main();
    const option = this.browser.$('[role="listbox"][aria-label="Human reviews"]').$(`[role="option"]*=${evalId}`);
    await option.waitForExist({ timeout: 15000, timeoutMsg: `timed out: Review lists ${evalId}` });
    await option.click();
    await holds(this.browser, () => this.browser.execute((expected) => document.querySelector(`[aria-label="Review's main area"] > header h2`)?.textContent?.trim() === expected, evalId), `the review of ${evalId} opened`);
    await this.browser.$(REVIEW).$("button=Claim to review").click();
    await holds(this.browser, () => this.browser.$(`${REVIEW} label.notes textarea`).isExisting(), `the review of ${evalId} claimed`);
  }

  /** Click the shown review's tool `name`: a viewer tool's window opens; its label. */
  async runTool(name: string): Promise<string> {
    await this.main();
    const before = await this.viewers();
    const button = this.browser.$(REVIEW).$('[role="group"][aria-label="Tools"]').$(`button=${name}`);
    await button.waitForEnabled({ timeout: 15000, timeoutMsg: `timed out: the tool ${name} is usable` });
    await button.click();
    return until(this.browser, async () => (await this.viewers()).find((label) => !before.includes(label)) ?? "", (label) => label !== "", `${name} opened an Artifact Viewer`);
  }

  /** The shown review's Notes. */
  async notes(): Promise<string> {
    await this.main();
    return this.browser.$(`${REVIEW} label.notes textarea`).getValue();
  }

  /** The shown review's Notes once `ok` holds for them. */
  async notesUntil(ok: (notes: string) => boolean, what: string): Promise<string> {
    await this.main();
    return until(this.browser, () => this.browser.$(`${REVIEW} label.notes textarea`).getValue(), ok, what);
  }

  /** The lines of the shown review's Notes that stand out as pointed, once `ok` holds for them. */
  async pointedUntil(ok: (lines: string[]) => boolean, what: string): Promise<string[]> {
    await this.main();
    const read = () => this.browser.execute((root) => [...document.querySelectorAll(`${root} label.notes mark`)].map((mark) => mark.textContent ?? ""), REVIEW);
    return until(this.browser, read, ok, what);
  }

  // -------------------------------------------------------------------------------
  // An Artifact Viewer

  /** Reach the viewer window `label`; its native title. */
  async viewer(label: string): Promise<string> {
    await this.reach((each) => each === label, `the viewer ${label} is open`);
    return String(await callWindow(this.browser, "title"));
  }

  /** The strip's text and controls, left to right. */
  strip(): Promise<{ items: string[]; point: boolean; pointing: boolean }> {
    return this.browser.execute(() => {
      const strip = document.querySelector("header.strip");
      const point = strip?.querySelector("button");
      return {
        items: [...(strip?.children ?? [])].map((item) => item.textContent?.replace(/\s+/g, " ").trim() ?? "").filter(Boolean),
        point: !!point,
        pointing: point?.getAttribute("aria-pressed") === "true",
      };
    });
  }

  /** The strip once `ok` holds for it. */
  stripUntil(ok: (strip: Awaited<ReturnType<Windows["strip"]>>) => boolean, what: string): ReturnType<Windows["strip"]> {
    return until(this.browser, () => this.strip(), ok, what);
  }

  /** Click Point in the strip: it reads as on. */
  async point(): Promise<void> {
    await this.browser.$("header.strip").$("button*=Point").click();
    await this.stripUntil((strip) => strip.pointing, "pointing");
  }

  /** A list's entries as they read (`[aria-label=...]`). */
  entries(list: string): Promise<string[]> {
    return this.browser.execute((query) => [...document.querySelectorAll(`[aria-label="${query}"] button`)].map((item) => item.textContent?.trim() ?? ""), list);
  }

  /** The list `list`'s entries once `ok` holds for them. */
  entriesUntil(list: string, ok: (entries: string[]) => boolean, what: string): Promise<string[]> {
    return until(this.browser, () => this.entries(list), ok, what);
  }

  /** Click the entry `name` of the list `list`. */
  async choose(list: string, name: string): Promise<void> {
    await this.browser.$(`[aria-label="${list}"]`).$(`button=${name}`).click();
  }

  /** The document view's lines (File Explorer) or blocks (Markdown), as they read. */
  document(): Promise<{ head: string; lines: string[]; headings: string[]; items: string[] }> {
    return this.browser.execute(() => {
      const doc = document.querySelector('[aria-label="Document"]');
      return {
        head: doc?.querySelector(".head")?.textContent?.trim() ?? "",
        lines: [...(doc?.querySelectorAll("ol.source li") ?? [])].map((line) => line.textContent ?? ""),
        headings: [...(doc?.querySelectorAll("h1, h2, h3") ?? [])].map((heading) => heading.textContent?.trim() ?? ""),
        items: [...(doc?.querySelectorAll("li") ?? [])].map((item) => item.textContent?.trim() ?? ""),
      };
    });
  }

  documentUntil(ok: (shown: Awaited<ReturnType<Windows["document"]>>) => boolean, what: string): ReturnType<Windows["document"]> {
    return until(this.browser, () => this.document(), ok, what);
  }

  /** Click the document's line `line` (File Explorer): while pointing, its place is appended. */
  async clickLine(line: number): Promise<void> {
    await this.browser.$(`[aria-label="Document"] ol.source li:nth-child(${line})`).click();
    await this.stripUntil((strip) => !strip.pointing, "pointing ended with the click");
  }

  /** The Artifactize Viewer's nodes: each declared Artifact's name and tag. */
  nodes(): Promise<string[]> {
    return this.browser.execute(() => [...document.querySelectorAll(".svelte-flow__node")].map((node) =>
      [...node.querySelectorAll(".name, .tag")].map((part) => part.textContent?.trim() ?? "").join(" ")));
  }

  nodesUntil(ok: (nodes: string[]) => boolean, what: string): Promise<string[]> {
    return until(this.browser, () => this.nodes(), ok, what);
  }

  /** Click the declared Artifact `name`: its panel opens; its evals, each as its ID, kind,
   * title, criteria and reads in one line. */
  async openNode(name: string): Promise<{ head: string; evals: string[] }> {
    const node = this.browser.$(`.svelte-flow__node[data-id="${name}"]`);
    await node.waitForExist({ timeout: 15000, timeoutMsg: `timed out: the Artifactize Viewer draws ${name}` });
    await node.click();
    const read = () => this.browser.execute((artifact) => {
      const panel = document.querySelector(`section[aria-label="Artifact ${artifact}"]`);
      return {
        head: panel?.querySelector<HTMLElement>("header")?.innerText.replace(/\s+/g, " ").trim() ?? "",
        evals: [...(panel?.querySelectorAll<HTMLElement>("section.eval") ?? [])].map((each) => each.innerText.replace(/\s+/g, " ").trim()),
      };
    }, name);
    return until(this.browser, read, (shown) => shown.head !== "", `${name}'s panel opened`);
  }

  /** A plugin's page: its frame's sandbox and where it loads from. */
  frame(): Promise<{ sandbox: string; src: string; title: string } | null> {
    return this.browser.execute(() => {
      const frame = document.querySelector("iframe");
      return frame ? { sandbox: frame.getAttribute("sandbox") ?? "", src: frame.src, title: frame.title } : null;
    });
  }

  frameUntil(ok: (frame: Awaited<ReturnType<Windows["frame"]>>) => boolean, what: string): ReturnType<Windows["frame"]> {
    return until(this.browser, () => this.frame(), ok, what);
  }

  /** Close the viewer `label` by its window frame, as a person does: it is gone, the others
   * stay. */
  async closeViewer(label: string): Promise<void> {
    await this.reach((each) => each === label, `the viewer ${label} is open`);
    await this.browser.closeWindow();
    await holds(this.browser, async () => !(await this.handles()).includes(label), `${label} closed`);
    await this.main();
  }

  // -------------------------------------------------------------------------------
  // The pet overlay

  /** Reach the pet overlay once it shows. */
  async pet(): Promise<Pet> {
    await this.reach((label) => label === PET, "the pet overlay is open");
    return this.petUntil((pet) => pet.head !== "", "the pet overlay shows its review");
  }

  /** Whether the pet overlay's window shows. */
  async petShown(): Promise<boolean> {
    if (!(await this.handles()).includes(PET)) return false;
    await this.browser.switchToWindow(PET);
    const shown = await this.browser.execute(() => document.visibilityState === "visible");
    await this.main();
    return shown;
  }

  readPet(): Promise<Pet> {
    return this.browser.execute(() => {
      const card = document.querySelector(".card");
      const point = [...(card?.querySelectorAll("button") ?? [])].find((button) => button.textContent?.trim() === "Point");
      return {
        head: [...(card?.querySelector(".head")?.children ?? [])].map((part) => part.textContent?.trim() ?? "").filter(Boolean).join(" "),
        notes: card?.querySelector<HTMLTextAreaElement>(".notes textarea")?.value ?? "",
        verdict: card?.querySelector('[role="radiogroup"][aria-label="Verdict"] [aria-checked="true"]')?.textContent?.trim() ?? "",
        point: !!point && !point.hasAttribute("disabled"),
        why: card?.querySelector(".why")?.textContent?.trim() ?? "",
        refusal: card?.querySelector(".error")?.textContent?.trim() ?? "",
      };
    });
  }

  petUntil(ok: (pet: Pet) => boolean, what: string): Promise<Pet> {
    return until(this.browser, () => this.readPet(), ok, what);
  }

  /** Type `text` as the pet's Notes. */
  async petNotes(text: string): Promise<void> {
    await fill(this.browser, ".card .notes textarea", text);
  }

  /** Click GREEN or RED in the pet: that verdict's form opens. */
  async petChoose(verdict: "GREEN" | "RED"): Promise<void> {
    await this.browser.$(".card .verdicts").$(`button=${verdict}`).click();
    await this.petUntil((pet) => pet.verdict === verdict, `the pet's ${verdict} form`);
  }

  /** Type `text` into the pet's form field `id`. */
  async petEnter(id: string, text: string): Promise<void> {
    await fill(this.browser, `.card #field-${id}`, text);
  }

  /** Click Submit in the pet: the review is submitted. */
  async petSubmit(): Promise<Pet> {
    await this.browser.$(".card").$("button*=Submit").click();
    return this.petUntil((pet) => pet.why === "Submitted." || pet.refusal !== "", "the pet's review submitted or refused");
  }
}

// The session's browser, as the spec's WDIO adapter gives it.
declare const browser: WebdriverIO.Browser;

/** The app's other windows, reached through the session's browser. */
export const windows = (): Windows => new Windows(browser);
