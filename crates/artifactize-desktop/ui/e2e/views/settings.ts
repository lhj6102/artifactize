// Settings as a person reads it: a modal over the screen with its tabs (Providers, Chat Agents,
// Remote, Limits), each provider's and chat agent's setup, the shared review store and the
// limits; each action waits for what it shows.
import { clickAt, pointerGapClick, visibleCenter } from "../input.ts";
import { fill } from "../keys.ts";
import { chooseOption, holds, until } from "./wait.ts";

const SETTINGS = '[role="dialog"].settings-dialog';
/** The Registry install's consent, over Settings. */
const CONSENT = '[role="dialog"].registry-consent';

/** Settings as read: its tabs with the shown one, the providers and chat agents listed with the
 * chosen one, and the text of what the shown tab shows. */
export type Shown = {
  tabs: { name: string; selected: boolean }[];
  providers: { name: string; chosen: boolean }[];
  chatAgents: { name: string; chosen: boolean }[];
  text: string;
};

export class Settings {
  private readonly browser: WebdriverIO.Browser;

  constructor(browser: WebdriverIO.Browser) {
    this.browser = browser;
  }

  /** Settings, or null while it is closed. */
  read(): Promise<Shown | null> {
    return this.browser.execute((selector) => {
      const dialog = document.querySelector<HTMLElement>(selector);
      if (!dialog) return null;
      return {
        tabs: [...dialog.querySelectorAll('[role="tablist"][aria-label="Settings"] [role="tab"]')].map((tab) => ({
          name: tab.textContent?.trim() ?? "", selected: tab.getAttribute("aria-selected") === "true",
        })),
        providers: [...dialog.querySelectorAll('nav[aria-label="Providers"] button')].map((button) => ({
          name: button.textContent?.replace(/\s+/g, " ").trim() ?? "", chosen: button.getAttribute("aria-current") === "true",
        })),
        chatAgents: [...dialog.querySelectorAll('nav[aria-label="Chat agents"] button')].map((button) => ({
          name: button.textContent?.replace(/\s+/g, " ").trim() ?? "", chosen: button.getAttribute("aria-current") === "true",
        })),
        text: (dialog.querySelector<HTMLElement>('[role="tabpanel"]:not([hidden])')?.innerText ?? "").replace(/\s+/g, " ").trim(),
      };
    }, SETTINGS);
  }

  /** Settings once `ok` holds for it. */
  async until(ok: (shown: Shown) => boolean, what: string, timeout = 15000): Promise<Shown> {
    return (await until(this.browser, () => this.read(), (shown) => !!shown && ok(shown), what, timeout))!;
  }

  /** Click the header's Settings: Settings opens on its Providers tab. */
  async open(): Promise<Shown> {
    await this.browser.$('footer[aria-label="Bottom bar"] button[aria-label="Settings"]').click();
    return this.until((shown) => shown.tabs.some((tab) => tab.selected), "Settings opened");
  }

  /** Close Settings by its Close, the header's Settings again, or a click outside it. */
  async close(by: "Close" | "Settings" | "outside"): Promise<void> {
    if (by === "Close") await this.browser.$(SETTINGS).$("button=Close").click();
    else if (by === "Settings") await this.browser.$('footer[aria-label="Bottom bar"] button[aria-label="Settings"]').click();
    else {
      // A point of the screen beneath, outside the panel and below the header, which stays above it.
      const point = await this.browser.execute((selector) => {
        const box = document.querySelector(selector)!.getBoundingClientRect();
        const y = Math.round(box.top + box.height / 2);
        return box.left > 24 ? { x: Math.round(box.left / 2), y } : { x: 12, y: Math.round((box.bottom + innerHeight) / 2) };
      }, SETTINGS);
      await pointerGapClick(this.browser, point);
    }
    await holds(this.browser, async () => (await this.read()) === null, `Settings closed by ${by}`);
  }

  async openUpdate(version: string): Promise<void> {
    const notice = this.browser.$(SETTINGS).$(`button=${version} available`);
    await notice.waitForDisplayed();
    await notice.click();
  }

  /** Click the tab `name`: it shows. */
  async tab(name: "Providers" | "Chat Agents" | "Remote" | "Limits"): Promise<Shown> {
    await this.browser.$(SETTINGS).$(`[role="tab"]=${name}`).click();
    return this.until((shown) => shown.tabs.some((tab) => tab.name === name && tab.selected), `the ${name} tab shown`);
  }

  /** Click the provider `name` in the list: its setup shows. */
  async provider(name: string): Promise<Shown> {
    const buttons = await this.browser.$$(`${SETTINGS} nav[aria-label="Providers"] button`).getElements();
    for (const button of buttons) {
      if ((await button.getText()).replace(/\s+/g, " ").trim() !== name) continue;
      // A dialog that just closed can keep the page from taking the pointer for a moment.
      await clickAt(this.browser, await visibleCenter(this.browser, button));
      return this.until((shown) => shown.providers.some((each) => each.name === name && each.chosen), `${name}'s setup shown`);
    }
    throw new Error(`Settings lists no provider ${name}`);
  }

  /** Click the chat agent `name` in the Chat Agents list: its setup shows. */
  async chatAgent(name: string): Promise<Shown> {
    const buttons = await this.browser.$$(`${SETTINGS} nav[aria-label="Chat agents"] button`).getElements();
    for (const button of buttons) {
      if ((await button.getText()).replace(/\s+/g, " ").trim() !== name) continue;
      await clickAt(this.browser, await visibleCenter(this.browser, button));
      return this.until((shown) => shown.chatAgents.some((each) => each.name === name && each.chosen), `${name}'s setup shown`);
    }
    throw new Error(`Settings lists no chat agent ${name}`);
  }

  /** Click Install… beside the Registry entry `name`: its consent dialog opens, and its facts
   * (Agent, Pinned version, Platform, Distribution, Source, Environment, SHA-256) are read. */
  async registryInstall(name: string): Promise<Record<string, string>> {
    const item = this.browser.$(SETTINGS).$(".registry-list").$(`li*=${name}`);
    await item.waitForDisplayed({ timeoutMsg: `the Registry lists ${name}` });
    await item.$("button=Install…").click();
    await this.browser.$(CONSENT).waitForDisplayed({ timeoutMsg: "the install consent dialog opened" });
    return this.browser.execute((selector) => {
      const facts: Record<string, string> = {};
      for (const term of document.querySelectorAll(`${selector} dl.facts > dt`)) {
        facts[term.textContent?.trim() ?? ""] = term.nextElementSibling?.textContent?.replace(/\s+/g, " ").trim() ?? "";
      }
      return facts;
    }, CONSENT);
  }

  /** Click Install in the consent dialog: it closes once the agent is installed. */
  async confirmRegistryInstall(): Promise<void> {
    const dialog = this.browser.$(CONSENT);
    await dialog.$("button=Install").click();
    await dialog.waitForDisplayed({ reverse: true, timeout: 30000, timeoutMsg: "the consent dialog closed once the agent was installed" });
  }

  /** The text of what the test id `id` marks (`key-openai`, `codex-status`, `remote-status`). */
  fact(id: string): Promise<string> {
    return this.browser.execute((selector, testid) => document.querySelector(`${selector} [data-testid="${testid}"]`)?.textContent?.replace(/\s+/g, " ").trim() ?? "", SETTINGS, id);
  }

  /** Wait until the fact `id` satisfies `ok`. */
  async factUntil(id: string, ok: (text: string) => boolean, what: string, timeout = 15000): Promise<string> {
    return until(this.browser, () => this.fact(id), ok, what, timeout);
  }

  /** Type `text` into the field `id`. */
  async enter(id: string, text: string): Promise<void> {
    await fill(this.browser, `${SETTINGS} #${id}`, text);
  }

  async enterCode(text: string): Promise<void> {
    await fill(this.browser, `${SETTINGS} input[aria-label="Code"]`, text);
  }

  /** Choose `option` of the choice field `id`. */
  async select(id: string, option: string): Promise<void> {
    await chooseOption(this.browser, `${SETTINGS} #${id}`, option);
  }


  /** Whether Settings shows the usable button `name`. */
  async usable(name: string): Promise<boolean> {
    const button = this.browser.$(SETTINGS).$(`button=${name}`);
    return await button.isExisting() && await button.isEnabled();
  }

  /** Click Settings' button `name`, once it is usable. */
  async button(name: string): Promise<void> {
    const button = this.browser.$(SETTINGS).$(`button=${name}`);
    await button.waitForEnabled({ timeout: 15000, timeoutMsg: `${name} is usable` });
    await button.click();
  }

  /** The models listed, each as its name and its ID beside it (`name · id`). */
  models(): Promise<string[]> {
    return this.browser.execute((selector) => [...document.querySelectorAll(`${selector} ul[aria-label="Models"] li`)]
      .map((item) => `${item.firstChild?.textContent?.trim() ?? ""} · ${item.querySelector("code")?.textContent?.trim() ?? ""}`), SETTINGS);
  }

  /** The models listed once `ok` holds for them. */
  async modelsUntil(ok: (models: string[]) => boolean, what: string): Promise<string[]> {
    return until(this.browser, () => this.models(), ok, what);
  }

  /** The sign-in's address Finish by hand shows. */
  async shownAddress(): Promise<string> {
    const shown = this.browser.$(`${SETTINGS} pre.url`);
    await shown.waitForDisplayed({ timeout: 15000, timeoutMsg: "the sign-in's address shown" });
    return (await shown.getText()).trim();
  }

  /** The value the field `id` holds. */
  value(id: string): Promise<string> {
    return this.browser.$(`${SETTINGS} #${id}`).getValue();
  }

  /** Whether Settings shows the field `id`. */
  hasField(id: string): Promise<boolean> {
    return this.browser.$(`${SETTINGS} #${id}`).isExisting();
  }

  /** Click the radio `value` of the inputs named `name` (a custom provider's kind, a store's
   * share level). */
  async choose(name: string, value: string): Promise<void> {
    const input = this.browser.$(`${SETTINGS} input[name="${name}"][value="${value}"]`);
    await input.click();
    await holds(this.browser, () => input.isSelected(), `${name} is ${value}`);
  }

  /** The shared review store's facts by name (Store URL, Share level, Status), or what it says
   * while signed out. */
  remote(): Promise<Record<string, string>> {
    return this.browser.execute((selector) => {
      const section = document.querySelector(`${selector} section[aria-labelledby="remote-title"]`);
      const facts: Record<string, string> = {};
      for (const term of section?.querySelectorAll('dl[aria-label="Remote store"] > dt') ?? []) {
        facts[term.textContent?.trim() ?? ""] = (term.nextElementSibling as HTMLElement | null)?.innerText.replace(/\s+/g, " ").trim() ?? "";
      }
      const said = section?.querySelector<HTMLElement>('p[aria-label="Remote store"]')?.innerText.trim();
      if (said) facts.Said = said;
      return facts;
    }, SETTINGS);
  }

  /** The store's facts once `ok` holds for them. */
  async remoteUntil(ok: (facts: Record<string, string>) => boolean, what: string): Promise<Record<string, string>> {
    return until(this.browser, () => this.remote(), ok, what);
  }
}
