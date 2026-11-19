// Waiting for what the window shows: each read is repeated until it holds, and a timeout says
// what the window showed last.

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
