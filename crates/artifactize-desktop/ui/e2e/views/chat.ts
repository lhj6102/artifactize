// Chat through the same visible controls a person uses.
import { fill } from "../keys.ts";
import { holds, until } from "./wait.ts";

const MAIN = `section[aria-label="Chat's main area"]`;
const LOG = '[role="log"][aria-label="Conversation"]';

export class Chat {
  private readonly browser: WebdriverIO.Browser;

  constructor(browser: WebdriverIO.Browser) { this.browser = browser; }

  async create(): Promise<void> {
    await this.browser.$('[aria-label="Chats"]').$("button=New chat").click();
    await this.browser.$('[role="dialog"].new-chat').$("button=Create").click();
    await this.browser.$(`${MAIN} textarea[aria-label="Message"]`).waitForDisplayed();
  }

  async model(name: string): Promise<void> {
    await this.browser.$(`${MAIN} button[aria-haspopup="menu"]`).click();
    const option = this.browser.$('[aria-label="Models"]').$(`[role="menuitemradio"]=${name}`);
    await option.waitForDisplayed();
    await option.click();
    await holds(this.browser, async () => (await this.browser.$(`${MAIN} button[aria-haspopup="menu"]`).getText()) === name, `${name} selected`);
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

  async openRun(run: string): Promise<void> {
    await this.browser.$(`${LOG} [data-run=${JSON.stringify(run)}]`).$("button=Open in Verify").click();
  }

  async stop(): Promise<void> {
    await this.browser.$(`${MAIN} button[aria-label="Stop (Esc)"]`).click();
    await this.says("Stopped");
  }
}
