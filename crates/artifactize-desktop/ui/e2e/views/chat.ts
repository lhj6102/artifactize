// Chat through the same visible controls a person uses.
import { clickRowGutter, scrollViewport } from "../input.ts";
import { fill } from "../keys.ts";
import { holds, until } from "./wait.ts";

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

  /** Click New chat, then Create once the dialog knows the model the session starts with: a
   * new, empty session shows. The starting model as the dialog said it (`{provider} · {model}`). */
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
    await create.waitForEnabled({ timeoutMsg: "timed out: the starting model is ready and Create is enabled" });
    const starting = (await dialog.$('[role="status"][aria-labelledby="new-chat-model"]').getText()).trim();
    await create.click();
    await dialog.waitForDisplayed({ reverse: true });
    await holds(this.browser, async () => await sessionCount() === before + 1, "Create added another session to the Chat Pane");
    await holds(this.browser, () => this.browser.execute((main, log) => {
      const shown = document.querySelector(main);
      return shown?.querySelector("header h2")?.textContent?.trim() === "New chat"
        && shown.querySelector(log)?.textContent?.trim() === "";
    }, MAIN, LOG), "the new session is selected with an empty conversation");
    return starting;
  }

  /** Select a view through the active session's header switch. */
  async show(view: "Conversation" | "Graph"): Promise<void> {
    const button = this.viewButton(view);
    await button.waitForDisplayed();
    if (await button.getAttribute("aria-pressed") !== "true") await button.click();
    await this.viewShown(view);
  }

  /** Observe only: the app or an in-app reference opened a session in this view. */
  async viewShown(view: "Conversation" | "Graph"): Promise<void> {
    const button = this.viewButton(view);
    await holds(this.browser, async () => await button.isDisplayed() && await button.getAttribute("aria-pressed") === "true", `the session shows its ${view} view`);
  }

  private viewButton(view: "Conversation" | "Graph") {
    return this.browser.$(`${MAIN} > header [aria-label="Session view"]`).$(`button=${view}`);
  }

  /** The model the shown session's selector names. */
  selected(): Promise<string> {
    return this.browser.$(`${MAIN} button[aria-haspopup="menu"]`).getText();
  }

  async model(name: string, exclude?: string): Promise<void> {
    await this.browser.$(`${MAIN} button[aria-haspopup="menu"]`).click();
    const menu = '[role="menu"][aria-label="Models"]';
    if (exclude) {
      const other = this.browser.$(menu).$(`[role="menuitemradio"]=${exclude}`);
      await other.waitForDisplayed({ timeoutMsg: `the unfiltered menu shows ${exclude}` });
      await fill(this.browser, `${menu} input[aria-label="Filter models"]`, name);
      await holds(this.browser, async () => !(await other.isDisplayed()), `filtering for ${name} excludes ${exclude}`);
    }
    const option = this.browser.$('[aria-label="Models"]').$(`[role="menuitemradio"]=${name}`);
    await option.waitForDisplayed();
    await option.click();
    await holds(this.browser, async () => !(await this.browser.$(menu).isDisplayed()) && await this.selected() === name, `${name} selected and its model menu closed`);
  }

  async filter(text: string): Promise<void> {
    await fill(this.browser, '[aria-label="Chats"] input[aria-label="Search chats"]', text);
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
    await this.browser.$(`${MAIN} button[aria-label="Send (Enter)"]`).click();
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
    await holds(this.browser, async () => (await this.reading(run)).top < before.top && !(await this.reading(run)).atEnd, "the conversation is scrolled up");
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
}
