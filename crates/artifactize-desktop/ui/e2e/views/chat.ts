// Chat through the same visible controls a person uses.
import { clickRowGutter, scrollViewport } from "../input.ts";
import { fill, press } from "../keys.ts";
import { chooseOption, holds, until } from "./wait.ts";

const MAIN = `main[aria-label="Chat"]:not([hidden]) section[aria-label="Chat's main area"]`;
const DIALOG = '[role="dialog"].new-chat';
const LOG = '[role="log"][aria-label="Conversation"]';

/** In the page: the branch shown selected in Chat's New chat dialog, if it is open. */
export function newChatBranch(): string | null {
  if (!document.querySelector('main[aria-label="Chat"]:not([hidden])')) return null;
  const dialog = document.querySelector('[role="dialog"].new-chat');
  if (!dialog?.getBoundingClientRect().height) return null;
  const label = document.getElementById(dialog.getAttribute("aria-labelledby") ?? "");
  if (label?.textContent?.trim() !== "New chat") return null;
  return dialog.querySelector("select")?.selectedOptions[0]?.label.trim() ?? null;
}

export class Chat {
  private readonly browser: WebdriverIO.Browser;

  constructor(browser: WebdriverIO.Browser) { this.browser = browser; }

  /** Click New chat, then Create once the model lookup settles. Return the dialog's model
   * status: a listed default or the explanation for creating without one. */
  async create(): Promise<string> {
    await this.browser.$('[aria-label="Chats"]').$("button=New chat").click();
    return this.createFromDialog();
  }

  /** Finish a New chat dialog already open, for example from a branch reference or launch. */
  async createFromDialog(branch?: string): Promise<string> {
    if (branch !== undefined) {
      await holds(this.browser, async () => await this.browser.execute(newChatBranch) === branch, `New chat is open on ${branch}`);
    }
    const dialog = this.browser.$(DIALOG);
    await dialog.waitForDisplayed();
    const sessionCount = () => this.browser.execute(() => document.querySelectorAll('[aria-label="Chats"] button.session').length);
    const before = await sessionCount();
    const create = dialog.$("button=Create");
    await create.waitForEnabled({ timeoutMsg: "timed out: the model lookup has settled and Create is enabled" });
    const starting = (await dialog.$('[role="status"][aria-labelledby="new-chat-model"]').getText()).trim();
    await create.click();
    await dialog.waitForDisplayed({ reverse: true });
    await holds(this.browser, async () => await sessionCount() === before + 1, "Create added another session to the Chat Pane");
    await holds(this.browser, () => this.browser.execute((main, log) => {
      const selected = document.querySelector('[aria-label="Chats"] button.session[aria-current="true"] .title');
      return selected?.textContent?.trim() === "New chat" && document.querySelector(main)?.querySelector(log)?.textContent?.trim() === "";
    }, MAIN, LOG), "the new session is selected with an empty conversation");
    return starting;
  }

  /** Select a view through the stage header's view tabs. */
  async show(view: "Conversation" | "Graph"): Promise<void> {
    const tab = this.viewTab(view);
    await tab.waitForDisplayed();
    if (await tab.getAttribute("aria-selected") !== "true") await tab.click();
    await this.viewShown(view);
  }

  /** Observe only: the app or an in-app reference opened a session in this view. */
  async viewShown(view: "Conversation" | "Graph"): Promise<void> {
    const tab = this.viewTab(view);
    await holds(this.browser, async () => await tab.isDisplayed() && await tab.getAttribute("aria-selected") === "true", `the session shows its ${view} view`);
  }

  private viewTab(view: "Conversation" | "Graph") {
    return this.browser.$(`${MAIN} > header [role="tablist"][aria-label="Session view"]`).$(`[role="tab"]=${view}`);
  }

  /** Move the shown session through its Move tool to the checkout `checkout` (as the dialog
   * lists it, `project / branch`), and wait until its row names the new branch. */
  async move(checkout: string, branch: string): Promise<void> {
    const tool = this.browser.$(`${MAIN} > header button[aria-label="Move to another checkout"]`);
    await tool.waitForEnabled({ timeoutMsg: "timed out: Move is usable" });
    await tool.click();
    const dialog = '[role="dialog"].move-chat';
    await holds(this.browser, () => this.browser.execute((query) => !!document.querySelector(query)
      && (document.querySelector<HTMLSelectElement>(`${query} select`)?.options.length ?? 0) > 1, dialog), "Move chat lists the live checkouts");
    await chooseOption(this.browser, `${dialog} select`, checkout);
    await this.browser.$(dialog).$("button=Move").click();
    await this.browser.$(dialog).waitForDisplayed({ reverse: true, timeoutMsg: "Move chat closed once the session moved" });
    await holds(this.browser, async () => await this.shownBranch() === branch, `the session's row names ${branch}`);
  }

  /** New chat from a project's row: its New chat cell, shown under the pointer or the keyboard. */
  async newChatIn(project: string): Promise<void> {
    const cell = this.browser.$(`[aria-label="Chats"] button[aria-label="New chat in ${project}"]`);
    await cell.waitForExist({ timeoutMsg: `the ${project} row offers New chat` });
    await cell.click();
    await this.browser.$('[role="dialog"].new-chat').waitForDisplayed({ timeoutMsg: "New chat opened" });
  }

  /** The branch New chat's dialog shows chosen, if it is open. */
  newChatBranch(): Promise<string | null> {
    return this.browser.execute(newChatBranch);
  }

  /** The branch the Pane's selected session row names after its title. */
  shownBranch(): Promise<string> {
    return this.browser.execute(() => document.querySelector('[aria-label="Chats"] button.session[aria-current="true"] .branch')?.textContent?.trim() ?? "");
  }

  /** The model the shown session's selector names. */
  selected(): Promise<string> {
    return this.browser.$(`${MAIN} button[aria-haspopup="menu"]`).getText();
  }

  async model(name: string, exclude?: string): Promise<void> {
    await this.browser.$(`${MAIN} button[aria-haspopup="menu"]`).click();
    const menu = '[role="menu"][aria-label="Chat agents and models"]';
    if (exclude) {
      const other = this.browser.$(menu).$(`[role="menuitemradio"]=${exclude}`);
      await other.waitForDisplayed({ timeoutMsg: `the unfiltered menu shows ${exclude}` });
      await fill(this.browser, `${menu} input[aria-label="Filter agents and models"]`, name);
      await holds(this.browser, async () => !(await other.isDisplayed()), `filtering for ${name} excludes ${exclude}`);
    }
    const option = this.browser.$(menu).$(`[role="menuitemradio"]=${name}`);
    await option.waitForDisplayed();
    await option.click();
    await holds(this.browser, async () => !(await this.browser.$(menu).isDisplayed()) && (await this.selected()).includes(name), `${name} selected and its model menu closed`);
  }

  /** Read the Trust workspace modal's agent, scope, executable and SHA-256 facts, then confirm it. */
  async trustWorkspace(): Promise<{ agent: string; scope: string; executable: string; sha256: string; text: string }> {
    const selector = '[role="dialog"].trust-workspace';
    const dialog = this.browser.$(selector);
    await dialog.waitForDisplayed({ timeoutMsg: "Trust workspace dialog opened" });
    const facts = await this.browser.execute((query) => {
      const root = document.querySelector<HTMLElement>(query);
      const map: Record<string, string> = {};
      for (const term of root?.querySelectorAll("dl.trust-facts > dt") ?? []) {
        map[term.textContent?.trim() ?? ""] = (term.nextElementSibling as HTMLElement | null)?.textContent?.replace(/\s+/g, " ").trim() ?? "";
      }
      return {
        agent: map.Agent ?? "",
        scope: map.Scope ?? "",
        executable: map.Executable ?? "",
        sha256: map["SHA-256"] ?? "",
        text: root?.innerText.replace(/\s+/g, " ").trim() ?? "",
      };
    }, selector);
    const confirm = dialog.$("button=Trust and continue");
    await confirm.waitForEnabled({ timeoutMsg: "Trust and continue is enabled" });
    await confirm.click();
    await dialog.waitForDisplayed({ reverse: true, timeoutMsg: "Trust workspace dialog closed" });
    return facts;
  }

  /** Search the sessions: Search in the Pane's top row turns into its field, then the text. */
  async filter(text: string): Promise<void> {
    const field = '[aria-label="Chats"] input[aria-label="Search chats"]';
    if (!(await this.browser.$(field).isExisting())) {
      await this.browser.$('[aria-label="Chats"] button[aria-label="Search chats"]').click();
      await holds(this.browser, () => this.browser.execute((query) => document.activeElement === document.querySelector(query), field), "the search field took the top row, focused");
    }
    await fill(this.browser, field, text);
  }

  /** Esc in the emptied search field: the top row is New chat and Search again. */
  async closeFilter(): Promise<void> {
    const field = '[aria-label="Chats"] input[aria-label="Search chats"]';
    await fill(this.browser, field, "");
    await press(this.browser, ["Escape"], async () => !(await this.browser.$(field).isExisting())
      && await this.browser.$('[aria-label="Chats"]').$("button=New chat").isDisplayed(), "the top row is New chat and Search again");
  }

  /** The projects in the Pane, in order, each with whether it is open. */
  projects(): Promise<{ name: string; open: boolean }[]> {
    return this.browser.execute(() => [...document.querySelectorAll<HTMLElement>('[aria-label="Chats"] button.project')]
      .map((row) => ({ name: row.innerText.trim(), open: row.getAttribute("aria-expanded") === "true" })));
  }

  /** Click a project's row: it folds, or unfolds. */
  async toggleProject(name: string): Promise<void> {
    const row = this.browser.$('[aria-label="Chats"]').$(`button.project=${name}`);
    const was = await row.getAttribute("aria-expanded");
    await row.click();
    await holds(this.browser, async () => await row.getAttribute("aria-expanded") !== was, `${name} ${was === "true" ? "folded" : "unfolded"}`);
  }

  /** Find in the conversation from the stage header's Find, typing `text`; what the field says. */
  async find(text: string): Promise<string> {
    const field = `${MAIN} > header input[aria-label="Find in the conversation"]`;
    const had = await this.browser.$(field).isExisting();
    const before = had ? await this.browser.execute((main) => document.querySelector(`${main} > header .find [role="status"]`)?.textContent?.trim() ?? "", MAIN) : "";
    if (!had) {
      await this.browser.$(`${MAIN} > header button[aria-label^="Find ("]`).click();
      await holds(this.browser, () => this.browser.execute((query) => document.activeElement === document.querySelector(query), field), "the find field took Find's place, focused");
    }
    await fill(this.browser, field, text);
    return (await until(this.browser, () => this.browser.execute((main, selector) => ({
      value: document.querySelector<HTMLInputElement>(selector)?.value ?? "",
      said: document.querySelector(`${main} > header .find [role="status"]`)?.textContent?.trim() ?? "",
    }), MAIN, field), ({ value, said }) => value === text && said !== "" && said !== before, "the find field says what it found")).said;
  }

  /** What the find field says: which place of how many, or that none matches. */
  findCount(): Promise<string> {
    return until(this.browser, () => this.browser.execute((main) => document.querySelector(`${main} > header .find [role="status"]`)?.textContent?.trim() ?? "", MAIN),
      (said) => said !== "", "the find field says what it found");
  }

  /** Enter (or Shift+Enter) in the find field: the next (or previous) place is shown. */
  async findStep(back = false): Promise<string> {
    const before = await this.findCount();
    await press(this.browser, back ? ["Shift", "Enter"] : ["Enter"], async () => (await this.findCount()) !== before || before === "1 of 1", "another place is shown");
    return this.findCount();
  }

  /** Esc in the find field: Find's icon is back. */
  async closeFind(): Promise<void> {
    await press(this.browser, ["Escape"], async () => !(await this.browser.$(`${MAIN} > header input[aria-label="Find in the conversation"]`).isExisting()), "the find field closed");
  }

  /** The reply card over the graph: its head (the message sent from Graph), whether it is open,
   * and its reply's text; null while there is none. */
  reply(): Promise<{ said: string; open: boolean; text: string } | null> {
    return this.browser.execute((main) => {
      const card = document.querySelector<HTMLElement>(`${main} [role="group"][aria-label="Chat"] section[aria-label="Reply"]`);
      if (!card) return null;
      const head = card.querySelector<HTMLElement>(".reply-head");
      return { said: head?.querySelector(".said")?.textContent?.trim() ?? "", open: head?.getAttribute("aria-expanded") === "true", text: card.querySelector<HTMLElement>(".reply-body")?.innerText.trim() ?? "" };
    }, MAIN);
  }

  /** Click the reply card's head: it folds to the head, or unfolds. */
  async toggleReply(): Promise<void> {
    const before = await this.reply();
    await this.browser.$(`${MAIN} section[aria-label="Reply"] .reply-head`).click();
    await holds(this.browser, async () => (await this.reply())?.open === !before?.open, "the reply card folded or unfolded");
  }

  /** In Graph: the floating chat's box and the canvas's, to check it floats over the canvas's
   * bottom, centred left of the Detail. */
  floating(): Promise<{ chat: { left: number; right: number; bottom: number }; canvas: { left: number; right: number; bottom: number }; detail: number | null } | null> {
    return this.browser.execute((main) => {
      const chat = document.querySelector(`${main} [role="group"][aria-label="Chat"]`)?.getBoundingClientRect();
      const canvas = document.querySelector(`${main} [data-graph-area]`)?.getBoundingClientRect();
      const detail = document.querySelector(`${main} aside[aria-label="Detail"]`)?.getBoundingClientRect();
      if (!chat || !canvas) return null;
      return { chat: { left: chat.left, right: chat.right, bottom: chat.bottom }, canvas: { left: canvas.left, right: canvas.right, bottom: canvas.bottom }, detail: detail ? detail.left : null };
    }, MAIN);
  }

  async sessionsUntil(titles: string[]): Promise<void> {
    const read = () => this.browser.execute(() => [...document.querySelectorAll<HTMLElement>('[aria-label="Chats"] button.session .title')]
      .map((title) => title.innerText.trim()).sort());
    await until(this.browser, read, (found) => JSON.stringify(found) === JSON.stringify(titles.toSorted()), `Chat lists ${titles.join(", ")}`);
  }

  async openSaved(title: string, gutter = false): Promise<void> {
    const saved = this.browser.$('[aria-label="Chats"]').$(`button.session*=${title}`);
    await saved.waitForDisplayed();
    if (gutter) await clickRowGutter(this.browser, await saved.getElement(), 'section[aria-label="Chats"]');
    else await saved.click();
    await holds(this.browser, async () => (await saved.getAttribute("aria-current")) === "true", `the saved session ${title} is selected`);
  }

  async send(text: string, conversation = true): Promise<void> {
    await fill(this.browser, `${MAIN} textarea[aria-label="Message"]`, text);
    const send = this.browser.$(`${MAIN} button[aria-label="Send (Enter)"]`);
    await send.waitForEnabled({ timeoutMsg: "Send is enabled for the typed message" });
    await send.click();
    if (conversation) await this.says(text);
  }

  async says(text: string): Promise<void> {
    await holds(this.browser, async () => (await this.browser.$(LOG).getText()).includes(text), `Chat shows ${text}`);
  }

  reading(run: string): Promise<{ top: number; atEnd: boolean; reference: boolean }> {
    return this.browser.execute((selector, id) => {
      const log = document.querySelector<HTMLElement>(selector);
      if (!log) return { top: -1, atEnd: false, reference: false };
      const frame = log.getBoundingClientRect();
      const card = log.querySelector(`[data-run=${JSON.stringify(id)}]`)?.getBoundingClientRect();
      return { top: log.scrollTop, atEnd: log.scrollHeight - log.clientHeight - log.scrollTop <= 2,
        reference: !!card && frame.height > 0 && card.top >= frame.top && card.bottom <= frame.bottom };
    }, `${MAIN} ${LOG}`, run);
  }

  async scrollUp(run: string): Promise<void> {
    const before = await this.reading(run);
    await scrollViewport(this.browser, await this.browser.$(`${MAIN} ${LOG}`).getElement(), -10000);
    await holds(this.browser, async () => (await this.reading(run)).top < before.top && !(await this.reading(run)).atEnd
      && await this.browser.$(MAIN).$("button=Scroll to end").isDisplayed(), "the conversation is scrolled up and Scroll to end is shown");
  }

  async scrollToEnd(run: string): Promise<void> {
    const button = this.browser.$(MAIN).$("button=Scroll to end");
    await button.waitForDisplayed({ timeoutMsg: "Scroll to end shows while reading an earlier part of the conversation" });
    await button.click();
    await holds(this.browser, async () => (await this.reading(run)).atEnd && !(await button.isDisplayed()), "the conversation is back at its end and follows again");
  }

  async runCard(): Promise<string> {
    return (await until(this.browser, () => this.browser.execute((log) => document.querySelector(`${log} [data-run]`)?.getAttribute("data-run") ?? "", LOG),
      (run) => /^run-/.test(run), "Chat shows a Run card"));
  }

  async runStatus(run: string, status: string): Promise<void> {
    const verdict = this.browser.$(`${LOG} [data-run=${JSON.stringify(run)}] b`);
    await holds(this.browser, async () => await verdict.getText() === status, `${run}'s Chat card shows ${status}`);
  }

  async emptySession(): Promise<void> {
    await holds(this.browser, async () => (await this.browser.$(LOG).getText()).trim() === "", "the other Chat conversation is empty");
  }

  async openRun(run: string): Promise<void> {
    await this.browser.$(`${LOG} [data-run=${JSON.stringify(run)}]`).$("button=Open in Verify").click();
  }

  /** Wait until the shown session's turn runs: Send reads Stop. */
  async working(): Promise<void> {
    await this.browser.$(`${MAIN} button[aria-label="Stop (Esc)"]`).waitForDisplayed({ timeoutMsg: "timed out: the turn runs and Send reads Stop" });
  }

  async finished(): Promise<void> {
    await this.browser.$(`${MAIN} button[aria-label="Send (Enter)"]`).waitForDisplayed({ timeoutMsg: "the turn ended and the composer shows Send again" });
  }

  async stop(kept: string): Promise<void> {
    await this.browser.$(`${MAIN} button[aria-label="Stop (Esc)"]`).click();
    await holds(this.browser, () => this.browser.execute((selector, partial) =>
      [...document.querySelectorAll<HTMLElement>(`${selector} .turn`)].some((turn) =>
        turn.querySelector("p.end")?.textContent?.trim() === "Stopped" && turn.innerText.includes(partial)),
    `${MAIN} ${LOG}`, kept), "the stopped turn keeps its partial response in the conversation");
  }

  /** The end-of-turn status lines (`Stopped`, `Interrupted · …`, error text) shown in the conversation. */
  turnEnds(): Promise<string[]> {
    return this.browser.execute((selector) => [...document.querySelectorAll<HTMLElement>(`${selector} .turn p.end`)]
      .map((line) => line.textContent?.replace(/\s+/g, " ").trim() ?? ""), `${MAIN} ${LOG}`);
  }
}
