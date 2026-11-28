// The app's window as a person sees it: its header, the command palette, Help, the dialogs it
// opens over the screen, and its parts (the pane, the main area's graph, the Detail, a Human
// review, Settings). Cases reach the window only through these views (E2E-REAL-APP).
import { asNativePath, callWindow, exists, readJson, type NativePath } from "../portable.ts";
import { isPid, waitForProcessExit } from "../platform.ts";
import { pointerGapClick } from "../input.ts";
import { fill, press, typeText } from "../keys.ts";
import { Chat } from "./chat.ts";
import { Detail } from "./detail.ts";
import { Graph } from "./graph.ts";
import { Pane } from "./pane.ts";
import { Review } from "./review.ts";
import { Settings } from "./settings.ts";
import { holds, until } from "./wait.ts";

const PALETTE = '[aria-label="Command palette"]';
const HELP = '[role="dialog"].help';
const DIALOG = '[role="dialog"], [role="alertdialog"]';

export class App {
  readonly pane: Pane;
  readonly chat: Chat;
  readonly graph: Graph;
  readonly detail: Detail;
  /** A Human review in the main area. */
  readonly review: Review;
  /** A Human review in the Detail of a graph. */
  readonly detailReview: Review;
  readonly settings: Settings;
  /** Where the window is kept as an image just before an action ends the app (exitsBy), so a
   * failure found once the window is gone still shows what it showed last. */
  lastWindow: NativePath | null = null;

  private readonly browser: WebdriverIO.Browser;

  constructor(browser: WebdriverIO.Browser) {
    this.browser = browser;
    this.pane = new Pane(browser);
    this.chat = new Chat(browser);
    this.graph = new Graph(browser);
    this.detail = new Detail(browser);
    this.review = new Review(browser, "main");
    this.detailReview = new Review(browser, "detail");
    this.settings = new Settings(browser);
  }

  /** Wait until the app runs its own frontend: anything else fails here, once, by name. */
  async loaded(): Promise<void> {
    await this.browser.switchToWindow("main");
    await holds(this.browser, () => this.browser.execute(() => (document.getElementById("app")?.childElementCount ?? 0) > 0),
      "setup failed: the test app did not load its frontend", 30000);
  }

  async mode(mode: "Chat" | "Verify" | "Review"): Promise<void> {
    const button = this.browser.$(`[aria-label="Modes"] button[aria-label^="${mode}"]`);
    await button.click();
    await holds(this.browser, async () => (await button.getAttribute("aria-current")) === "page", `${mode} selected in the bottom bar`);
  }

  /** The mode the bottom bar marks as shown. */
  async currentMode(): Promise<string> {
    const label = await this.browser.$('[aria-label="Modes"] button[aria-current="page"]').getAttribute("aria-label");
    return (label ?? "").split(",")[0];
  }

  async waitingBadge(count: number): Promise<void> {
    const button = this.browser.$('[aria-label="Modes"] button[aria-label^="Review"]');
    await holds(this.browser, async () => (await button.getAttribute("aria-label")) === (count ? `Review, ${count} waiting` : "Review"), `${count} Human reviews waiting`);
  }

  /** The shared bar's top, once it spans the bottom edge of the visible viewport. */
  async bottomBar(): Promise<number> {
    return (await until(this.browser, () => this.browser.execute(() => {
      const box = document.querySelector('footer[aria-label="Bottom bar"]')?.getBoundingClientRect();
      return box && Math.abs(box.bottom - innerHeight) < 1 && Math.abs(box.width - innerWidth) < 1 ? box.top : null;
    }), (top) => top !== null && top > 0, "the shared bar spans the window's bottom edge"))!;
  }

  /** The window's text. */
  text(): Promise<string> {
    return this.browser.execute(() => document.body.innerText);
  }

  /** The native window's own title, as Tauri's window API says it. */
  async windowTitle(): Promise<string> {
    return String(await callWindow(this.browser, "title"));
  }

  /** The header bar's controls and text, left to right, and whether the Pane switch reads as on. */
  header(): Promise<{ brand: string; items: string[]; paneOn: boolean }> {
    return this.browser.execute(() => {
      const bar = document.querySelector('footer[aria-label="Bottom bar"]');
      return {
        brand: bar?.querySelector(".brand")?.textContent?.trim() ?? "",
        items: [...(bar?.querySelectorAll("button, .reviewer") ?? [])].map((item) => item.textContent?.replace(/\s+/g, " ").trim() ?? ""),
        paneOn: bar?.querySelector("button.switch")?.getAttribute("aria-pressed") === "true",
      };
    });
  }

  /** The header once `ok` holds for it. */
  async headerUntil(ok: (shown: Awaited<ReturnType<App["header"]>>) => boolean, what: string): Promise<Awaited<ReturnType<App["header"]>>> {
    return until(this.browser, () => this.header(), ok, what);
  }

  /** Wait until `check`, an effect outside the window (a file a stand-in wrote, a saved
   * request), holds. */
  async eventually(check: () => Promise<boolean> | boolean, what: string, timeout = 15000): Promise<void> {
    await holds(this.browser, async () => check(), what, timeout);
  }

  /** Wait until `check`, a reading of the window through the views, holds. */
  async until(check: () => Promise<boolean>, what: string, timeout = 15000): Promise<void> {
    await holds(this.browser, check, what, timeout);
  }

  /** How wide the window's page is. */
  windowWidth(): Promise<number> {
    return this.browser.execute(() => document.documentElement.clientWidth);
  }

  /** Click the header's `name` (Settings, Commands, Pane, a release notice). */
  async headerButton(name: string): Promise<void> {
    await this.browser.$(`button*=${name}`).click();
  }

  /** Press `keys` wherever the keyboard is. */
  async press(keys: string[], done: () => Promise<boolean>, what: string): Promise<void> {
    await press(this.browser, keys, done, what);
  }

  /** Type `text` where the keyboard is, a key at a time. */
  async type(text: string): Promise<void> {
    await typeText(this.browser, text);
  }

  // -------------------------------------------------------------------------------
  // The command palette

  /** The open palette: whether it is open (vs still unmounting), its commands in order, each
   * with its key, which is chosen, and whether its search has the keyboard; null once it has
   * left the page. */
  palette(): Promise<{ open: boolean; focused: boolean; query: string; commands: { label: string; keys: string; chosen: boolean }[] } | null> {
    return this.browser.execute((selector) => {
      const dialog = document.querySelector(selector);
      const input = dialog?.querySelector("input");
      if (!dialog || !input) return null;
      return {
        open: dialog.getAttribute("data-state") === "open",
        focused: document.activeElement === input,
        query: input.value,
        commands: [...dialog.querySelectorAll('[role="option"]')].map((option) => ({
          label: option.querySelector("span")?.textContent?.trim() ?? "",
          keys: option.querySelector("kbd")?.textContent?.trim() ?? "",
          chosen: option.getAttribute("aria-selected") === "true",
        })),
      };
    }, PALETTE);
  }

  /** Click Commands: the palette opens with its search holding the keyboard. */
  async openPalette(): Promise<NonNullable<Awaited<ReturnType<App["palette"]>>>> {
    await this.browser.$('footer[aria-label="Bottom bar"] button.brand').click();
    return (await until(this.browser, () => this.palette(), (shown) => !!shown?.open && shown.focused && shown.commands.length > 0, "the palette opened"))!;
  }

  /** In the open palette, type `query` and click the command `label`; the palette closes. */
  async runCommand(query: string, label = query): Promise<void> {
    const unfiltered = (await this.palette())?.commands.length ?? 0;
    await typeText(this.browser, query);
    await until(this.browser, () => this.palette(), (shown) => !!shown?.open && shown.query === query && shown.commands.length < unfiltered && shown.commands.some((command) => command.label === label),
      `the palette narrowed to ${query} and lists ${label}`);
    for (const option of await this.browser.$$(`${PALETTE} [role="option"]`).getElements()) {
      if ((await option.$("span").getText()).trim() !== label) continue;
      await option.click();
      await holds(this.browser, async () => (await this.palette()) === null, `${label} ran and the palette closed`);
      return;
    }
    throw new Error(`the palette does not list ${label}`);
  }

  /** Open the palette and run `label` from it. */
  async command(label: string): Promise<void> {
    await this.openPalette();
    await this.runCommand(label);
  }

  // -------------------------------------------------------------------------------
  // Help

  /** The open Help: the keys it lists with what each does, and the marks it pairs with their
   * words; null while it is closed. */
  help(): Promise<{ keys: { keys: string; action: string }[]; marks: { icon: string; word: string }[] } | null> {
    return this.browser.execute((selector) => {
      const dialog = document.querySelector(selector);
      if (!dialog) return null;
      return {
        keys: [...dialog.querySelectorAll("dt")].map((key) => ({ keys: key.textContent?.trim() ?? "", action: key.nextElementSibling?.textContent?.trim() ?? "" })),
        marks: [...dialog.querySelectorAll('[aria-label="Marks"] li')].map((item) => ({
          icon: item.querySelector("svg")?.getAttribute("data-icon") ?? "", word: item.textContent?.trim() ?? "",
        })),
      };
    }, HELP);
  }

  /** Close the open Help with its Close, or with a click outside it. */
  async closeHelp(by: "Close" | "outside"): Promise<void> {
    if (by === "Close") await this.browser.$(HELP).$("button=Close").click();
    else await this.clickOutside(HELP);
    await holds(this.browser, async () => (await this.help()) === null, `Help closed by ${by}`);
  }

  /** Click a point of the window outside the dialog `selector`. */
  async clickOutside(selector: string): Promise<void> {
    const point = await this.browser.execute((dialog) => {
      const box = document.querySelector(dialog)!.getBoundingClientRect();
      return [{ x: 8, y: innerHeight - 8 }, { x: innerWidth - 8, y: innerHeight - 8 }, { x: 8, y: 60 }]
        .find(({ x, y }) => x < box.left || x >= box.right || y < box.top || y >= box.bottom) ?? null;
    }, selector);
    if (!point) throw new Error("the dialog leaves no point of the window outside it");
    await pointerGapClick(this.browser, point);
  }

  // -------------------------------------------------------------------------------
  // Dialogs

  /** The open dialog's title and text, or null. */
  dialog(): Promise<{ title: string; text: string } | null> {
    return this.browser.execute((selector) => {
      const dialog = [...document.querySelectorAll<HTMLElement>(selector)].find((each) => !each.matches(".help, .palette, .settings-dialog"));
      return dialog ? { title: dialog.querySelector(".dialog-title")?.textContent?.trim() ?? "", text: dialog.innerText.replace(/\s+/g, " ").trim() } : null;
    }, DIALOG);
  }

  /** Wait until a dialog titled `title` is open. */
  async dialogTitled(title: string): Promise<{ title: string; text: string }> {
    return (await until(this.browser, () => this.dialog(), (shown) => shown?.title === title, `the dialog ${title} opened`))!;
  }

  /** Click the open dialog's `button`. */
  async dialogButton(button: string): Promise<void> {
    await this.browser.$(`${DIALOG}`).$(`button*=${button}`).click();
  }

  /** Wait until no dialog is open. */
  async dialogClosed(): Promise<void> {
    await holds(this.browser, async () => (await this.dialog()) === null, "the dialog closed");
  }

  /** In the open "Open a request by ID" dialog, type `id` and click Open. */
  async openById(id: string): Promise<void> {
    await this.dialogTitled("Open a request by ID");
    await fill(this.browser, 'input[aria-label="Request ID"]', id);
    await this.dialogButton("Open");
    await this.dialogClosed();
  }

  /** The release dialog's versions line, its update command and the address it links. */
  releaseDialog(): Promise<{ versions: string; command: string; link: string } | null> {
    return this.browser.execute(() => {
      const dialog = [...document.querySelectorAll('[role="dialog"]')].find((each) => each.querySelector(".dialog-title")?.textContent?.trim() === "Update artifactize");
      if (!dialog) return null;
      return {
        versions: dialog.querySelector(".versions")?.textContent?.trim() ?? "",
        command: dialog.querySelector("pre.command")?.textContent?.trim() ?? "",
        link: dialog.querySelector("button.link")?.textContent?.trim() ?? "",
      };
    });
  }

  async closeRelease(): Promise<void> {
    const link = await this.browser.$('[role="dialog"] button.link').getElement();
    await link.$("./ancestor::*[@role='dialog']").$("button=Close").click();
    await holds(this.browser, async () => (await this.releaseDialog()) === null, "the update dialog closed");
  }

  /** Click the release dialog's link: the system browser is handed it. */
  async followReleaseLink(): Promise<void> {
    await this.browser.$('[role="dialog"] button.link').click();
  }

  // -------------------------------------------------------------------------------
  // The window itself

  /** Ask the window to close, as its frame's close button does (Tauri's window API). */
  async closeWindow(): Promise<void> {
    await callWindow(this.browser, "close");
  }

  /** Do `action`, then confirm the exact app that was in this session quit in order. The
   * command's own response may be lost on exit, but a failed native observation is never
   * success, and neither is an exit the app did not record as orderly: a crash also ends it. */
  async exitsBy(action: () => Promise<unknown>): Promise<void> {
    const shown = await this.browser.execute(() =>
      (window as Window & { __ARTIFACTIZE_E2E_PROCESS__?: { pid: number; exitReceipt: string | null } }).__ARTIFACTIZE_E2E_PROCESS__);
    const pid = shown?.pid;
    if (!isPid(pid)) throw new Error("the app did not provide its native process ID before the close action");
    if (typeof shown?.exitReceipt !== "string") throw new Error("the app did not say where it records an orderly quit");
    const receipt = asNativePath(shown.exitReceipt);
    if (this.lastWindow) await this.browser.saveScreenshot(this.lastWindow).catch(() => undefined);
    const failures: unknown[] = [];
    try { await action(); } catch (error) { failures.push(error); }
    try { await waitForProcessExit(pid); }
    catch (error) {
      if (failures.length) throw new AggregateError([...failures, error], `the close action failed and app PID ${pid} did not confirm exit`);
      throw error;
    }
    Reflect.set(this.browser, "sessionId", undefined);
    const recorded = exists(receipt) ? (readJson(receipt) as { pid?: unknown }) : null;
    if (recorded?.pid !== pid) {
      throw new AggregateError(failures, `app PID ${pid} exited without recording an orderly quit, as a crash does`);
    }
  }
}
