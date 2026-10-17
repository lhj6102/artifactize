// The Monitor's Run graph as a person sees it: the Artifact that has the keyboard, the eval
// popup beside an Artifact, and the summary beside the node it describes. Every reading is
// read-only; a timeout reports what the graph showed instead.

/** The Artifact node with keyboard focus (`a:<name>`), or null. */
export function focusedNode(browser: WebdriverIO.Browser): Promise<string | null> {
  return browser.execute(() => document.activeElement?.closest(".svelte-flow__node")?.getAttribute("data-id") ?? null);
}

/** Wait until the Run's graph has drawn the node `id`. */
export async function graphShown(browser: WebdriverIO.Browser, id: string, timeout = 15000): Promise<void> {
  await browser.waitUntil(() => browser.execute((want) => {
    const node = document.querySelector(`.svelte-flow__node[data-id="${want}"]`);
    const box = node?.getBoundingClientRect();
    return !!box && box.width > 0 && box.height > 0;
  }, id), { timeout, timeoutMsg: `the Run graph draws ${id}` });
}

/** The node selector of the Artifact `id`, and the popup row selector of its eval `path`. */
export const nodeAt = (id: string) => `.svelte-flow__node[data-id="${id}"]`;
export const rowAt = (path: string) => `[aria-label="Evals"] [role="option"][data-path="${path}"]`;

export type Popup = { artifact: string; evals: string[]; selected: string | null; focused: boolean };

/** The open eval popup: its Artifact's name, its evals' names, the selected eval's path and
 * whether its list has the keyboard; null while no popup is open. */
export function popup(browser: WebdriverIO.Browser): Promise<Popup | null> {
  return browser.execute(() => {
    const section = document.querySelector('section[aria-label^="Evals of "]');
    if (!section) return null;
    const options = [...section.querySelectorAll<HTMLElement>('[role="option"][data-path]')];
    return {
      artifact: section.getAttribute("aria-label")?.slice("Evals of ".length) ?? "",
      evals: options.map((option) => option.querySelector(".name")?.firstChild?.textContent?.trim() ?? ""),
      selected: options.find((option) => option.getAttribute("aria-selected") === "true")?.dataset.path ?? null,
      focused: document.activeElement?.getAttribute("aria-label") === "Evals" && section.contains(document.activeElement),
    };
  });
}

type Shown = { text: string; beside: boolean; anchor: boolean };

/** The summary's text, and whether it floats beside the element `anchor` finds: visible, not
 * covering it, and at most a small gap away. */
function summary(browser: WebdriverIO.Browser, anchor: string): Promise<Shown | null> {
  return browser.execute((selector) => {
    const box = document.querySelector<HTMLElement>('[aria-label="Summary"]');
    if (!box) return null;
    const target = document.querySelector(selector)?.getBoundingClientRect();
    const near = box.getBoundingClientRect();
    if (!target) return { text: box.innerText, beside: false, anchor: false };
    const across = Math.max(0, target.left - near.right, near.left - target.right);
    const along = Math.max(0, target.top - near.bottom, near.top - target.bottom);
    const visible = getComputedStyle(box).visibility === "visible" && near.width > 0 && near.height > 0;
    return { text: box.innerText, beside: visible && (across > 0 || along > 0) && Math.max(across, along) <= 24, anchor: true };
  }, anchor);
}

/** Wait until the summary shows `text` beside the element `anchor` finds. A timeout reports
 * the summary, the focused node and the popup. */
export async function summaryShows(browser: WebdriverIO.Browser, anchor: string, text: RegExp): Promise<void> {
  try {
    await browser.waitUntil(async () => {
      const shown = await summary(browser, anchor);
      return !!shown && shown.beside && text.test(shown.text);
    }, { timeout: 15000, timeoutMsg: `the summary shows ${text} beside ${anchor}` });
  } catch (error) {
    const state = { summary: await summary(browser, anchor), focused: await focusedNode(browser), popup: await popup(browser) };
    throw new Error(`the summary shows ${text} beside ${anchor}: ${JSON.stringify(state)}`, { cause: error });
  }
}

/** Whether a summary is shown at all. */
export function summaryOpen(browser: WebdriverIO.Browser): Promise<boolean> {
  return browser.$('[aria-label="Summary"]').isExisting();
}
