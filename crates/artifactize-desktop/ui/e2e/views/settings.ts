// Settings as a person reads it: a modal over the screen with its tabs (Providers, Remote,
// Limits), each provider's setup, the shared review store and the limits; each action waits
// for what it shows.
import { clickAt, clickOption, visibleCenter } from "../input.ts";
import { fill } from "../keys.ts";
import { holds, until } from "./wait.ts";

const SETTINGS = '[role="dialog"].settings-dialog';

/** Settings as read: its tabs with the shown one, the providers listed with the chosen one,
 * and the text of what the shown tab shows. */
export type Shown = { tabs: { name: string; selected: boolean }[]; providers: { name: string; chosen: boolean }[]; text: string };

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
    await this.browser.$("header.bar").$("button=Settings").click();
    return this.until((shown) => shown.tabs.some((tab) => tab.selected), "Settings opened");
  }

  /** Close Settings by its Close, the header's Settings again, or a click outside it. */
  async close(by: "Close" | "Settings" | "outside"): Promise<void> {
    if (by === "Close") await this.browser.$(SETTINGS).$("button=Close").click();
    else if (by === "Settings") await this.browser.$("header.bar").$("button=Settings").click();
    else {
      const point = await this.browser.execute((selector) => {
        const box = document.querySelector(selector)!.getBoundingClientRect();
        return { x: Math.round(box.left / 2) || 4, y: Math.round((box.bottom + innerHeight) / 2) };
      }, SETTINGS);
      await clickAt(this.browser, point);
    }
    await holds(this.browser, async () => (await this.read()) === null, `Settings closed by ${by}`);
  }

  /** Click the tab `name`: it shows. */
  async tab(name: "Providers" | "Remote" | "Limits"): Promise<Shown> {
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

  /** Choose `option` of the choice field `id`. */
  async select(id: string, option: string): Promise<void> {
    for (const each of await this.browser.$$(`${SETTINGS} #${id} option`).getElements()) {
      if ((await each.getText()).trim() !== option) continue;
      await clickOption(this.browser, each);
      await holds(this.browser, async () => (await this.browser.$(`${SETTINGS} #${id} option:checked`).getText()).trim() === option, `${option} chosen`);
      return;
    }
    throw new Error(`the field ${id} offers no ${option}`);
  }

  /** Click the radio `option` of the group `group`. */
  async radio(group: string, option: string): Promise<void> {
    const input = this.browser.$(`${SETTINGS} [role="radiogroup"][aria-label="${group}"]`).$(`label*=${option}`).$("input");
    await input.click();
    await holds(this.browser, () => input.isSelected(), `${group} is ${option}`);
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

  /** The models listed, as they read. */
  models(): Promise<string[]> {
    return this.browser.execute((selector) => [...document.querySelectorAll(`${selector} ul[aria-label="Models"] li`)].map((item) => item.textContent?.replace(/\s+/g, " ").trim() ?? ""), SETTINGS);
  }

  /** What Settings says went wrong, as it reads. */
  errors(): Promise<string[]> {
    return this.browser.execute((selector) => [...document.querySelectorAll(`${selector} [role="alert"]`)].map((alert) => alert.textContent?.trim() ?? ""), SETTINGS);
  }

  /** The value the field `id` holds. */
  value(id: string): Promise<string> {
    return this.browser.$(SETTINGS).$(`#${id}`).getValue();
  }
}
