// Chat through the same visible controls a person uses.
import { fill } from "../keys.ts";
import { holds, until } from "./wait.ts";

const MAIN = `section[aria-label="Chat's main area"]`;
const LOG = '[role="log"][aria-label="Conversation"]';

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
  async createFromDialog(): Promise<string> {
    const dialog = this.browser.$('[role="dialog"].new-chat');
    const create = dialog.$("button=Create");
    await create.waitForEnabled({ timeoutMsg: "timed out: the starting model is ready and Create is enabled" });
    const starting = (await dialog.$('[role="status"][aria-labelledby="new-chat-model"]').getText()).trim();
    await create.click();
    await this.browser.$(`${MAIN} textarea[aria-label="Message"]`).waitForDisplayed();
    return starting;
  }

  /** Show the active session's Graph through its header switch. */
  async graph(): Promise<void> {
    const graph = this.browser.$(`${MAIN} > header`).$("button=Graph");
    await graph.waitForDisplayed();
    if (await graph.getAttribute("aria-pressed") !== "true") await graph.click();
    await holds(this.browser, async () => (await graph.getAttribute("aria-pressed")) === "true", "the session shows its Graph view");
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
