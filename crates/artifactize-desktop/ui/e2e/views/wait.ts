// Waiting for what the window shows: each read is repeated until it holds, and a timeout says
// what the window showed last.
import { clickOption } from "../input.ts";

/** Read `read` until `ok` holds; the reading that held. */
export async function until<T>(browser: WebdriverIO.Browser, read: () => Promise<T>, ok: (value: T) => boolean, what: string, timeout = 15000): Promise<T> {
  let last: T | undefined;
  let held = false;
  try {
    await browser.waitUntil(async () => {
      last = await read();
      held = ok(last);
      return held;
    }, { timeout, timeoutMsg: `timed out: ${what}` });
  } catch (error) {
    throw new Error(`${what}: the window showed ${JSON.stringify(last)}`, { cause: error });
  }
  return last as T;
}

/** Wait until `check` holds. */
export async function holds(browser: WebdriverIO.Browser, check: () => Promise<boolean>, what: string, timeout = 15000): Promise<void> {
  await until(browser, check, (value) => value, what, timeout);
}

/** Choose the option `option` (as it reads) of the choice field `select`, as a click on it does. */
export async function chooseOption(browser: WebdriverIO.Browser, select: string, option: string): Promise<void> {
  const options = await browser.execute((field) => [...(document.querySelector<HTMLSelectElement>(field)?.options ?? [])].map((each) => each.text.trim()), select);
  const at = options.indexOf(option);
  if (at < 0) throw new Error(`${select} offers no ${option}: ${options.join(", ")}`);
  await clickOption(browser, await browser.$(`${select} option:nth-child(${at + 1})`).getElement());
  await holds(browser, () => browser.execute((field, index) => document.querySelector<HTMLSelectElement>(field)?.selectedIndex === index, select, at), `${option} chosen`);
}
