// Choosing a command in the real command palette. An opened palette is not yet ready for
// input, and a typed query is not yet a selection: each step waits for the state the next
// one needs, and a timeout reports what the palette showed.
import { typeText } from "./keys.ts";

type Shown = { focused: boolean; query: string | null; options: { label: string; selected: boolean }[] };

function shown(browser: WebdriverIO.Browser): Promise<Shown> {
  return browser.execute(() => {
    const dialog = document.querySelector('[aria-label="Command palette"][data-state="open"]');
    const input = dialog?.querySelector("input") ?? null;
    return {
      focused: !!input && document.activeElement === input,
      query: input?.value ?? null,
      options: [...(dialog?.querySelectorAll('[role="option"]') ?? [])].map((option) => ({
        label: option.querySelector("span")?.textContent ?? "",
        selected: option.getAttribute("aria-selected") === "true",
      })),
    };
  });
}

async function paletteShows(browser: WebdriverIO.Browser, check: (palette: Shown) => boolean, what: string): Promise<void> {
  try {
    await browser.waitUntil(async () => check(await shown(browser)), { timeout: 10000, timeoutMsg: `timed out: ${what}` });
  } catch (error) {
    throw new Error(`${what}; the palette showed ${JSON.stringify(await shown(browser))}`, { cause: error });
  }
}

/** In the open palette, type `query` into its empty search and wait until the command
 * labelled `label` is the selected one; the caller then presses Enter for its outcome. */
export async function choosePaletteCommand(browser: WebdriverIO.Browser, query: string, label = query): Promise<void> {
  await paletteShows(browser, (palette) => palette.focused && palette.query === "", "the palette's empty search has focus");
  await typeText(browser, query);
  await paletteShows(browser, (palette) => palette.query === query && palette.options.some((option) => option.selected && option.label === label),
    `"${query}" is the typed query and "${label}" the selected command`);
}
