// Chat through the same visible controls a person uses.
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
    const create = dialog.$("button=Create");
    await create.waitForEnabled({ timeoutMsg: "timed out: the starting model is ready and Create is enabled" });
    const starting = (await dialog.$('[role="status"][aria-labelledby="new-chat-model"]').getText()).trim();
    await create.click();
    await dialog.waitForDisplayed({ reverse: true });
    await this.browser.$(`${MAIN} textarea[aria-label="Message"]`).waitForDisplayed();
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

  async model(name: string): Promise<void> {
    await this.browser.$(`${MAIN} button[aria-haspopup="menu"]`).click();
    const option = this.browser.$('[aria-label="Models"]').$(`[role="menuitemradio"]=${name}`);
    await option.waitForDisplayed();
    await option.click();
    await holds(this.browser, async () => (await this.browser.$(`${MAIN} button[aria-haspopup="menu"]`).getText()) === name, `${name} selected`);
  }

  async openSaved(title: string): Promise<void> {
    const saved = this.browser.$('[aria-label="Chats"]').$(`button.session*=${title}`);
    await saved.waitForDisplayed();
    await saved.click();
    await holds(this.browser, async () => (await saved.getAttribute("aria-current")) === "true", `the saved session ${title} is selected`);
  }

  async send(text: string): Promise<void> {
    await fill(this.browser, `${MAIN} textarea[aria-label="Message"]`, text);
    await this.browser.$(`${MAIN} button[aria-label="Send (Enter)"]`).click();
    await this.says(text);
  }

  async says(text: string): Promise<void> {
    await holds(this.browser, async () => (await this.browser.$(LOG).getText()).includes(text), `Chat shows ${text}`);
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

  async stop(): Promise<void> {
    await this.browser.$(`${MAIN} button[aria-label="Stop (Esc)"]`).click();
    await this.says("Stopped");
  }
}
