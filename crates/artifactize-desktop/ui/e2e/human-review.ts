// The Human review as the app shows it to its reviewer.

/** The Human review shows the reviewer's own claim as REVIEW and runs no job. The store and
 * the rendered stage can show a claim before the app's own claim answer has arrived; until
 * then the review is busy and ignores keys and tool clicks. */
export function reviewing(browser: WebdriverIO.Browser, selector = '[aria-label="Human review"]'): Promise<boolean> {
  return browser.execute((query) => {
    const detail = document.querySelector(query);
    return !!detail && (detail.textContent ?? "").includes("REVIEW (yours)") && !detail.querySelector(".busy");
  }, selector);
}

/** The Human review waits for a claim: its stage reads CLAIM and its Claim button is usable. */
export function claimable(browser: WebdriverIO.Browser, selector = '[aria-label="Human review"]'): Promise<boolean> {
  return browser.execute((query) => {
    const detail = document.querySelector(query);
    const claim = [...(detail?.querySelectorAll("button") ?? [])].find((button) => button.textContent?.trim().startsWith("Claim to review"));
    return !!detail && detail.querySelector(".stage")?.textContent?.trim() === "CLAIM" && !!claim && !claim.disabled && !detail.querySelector(".busy");
  }, selector);
}

/** Choose a verdict in the current form; each verdict rebuilds its own fields. */
export async function chooseVerdict(browser: WebdriverIO.Browser, verdict: "GREEN" | "RED"): Promise<void> {
  await browser.$('[aria-label="Human review"] [role="radiogroup"][aria-label="Verdict"]').$(`[role="radio"]=${verdict}`).click();
}

/** Unfold the review's Debug section with a click on its header row, once the review shows it;
 * a Debug already unfolded is left as it is. */
export async function showDebug(browser: WebdriverIO.Browser, selector = '[aria-label="Human review"]'): Promise<void> {
  const head = browser.$(selector).$("button.head=Debug");
  await head.waitForExist({ timeout: 15000, timeoutMsg: "the review's Debug section" });
  if ((await head.getAttribute("aria-expanded")) === "false") await head.click();
  await browser.waitUntil(async () => (await browser.$(selector).$("button.head=Debug").getAttribute("aria-expanded")) === "true",
    { timeout: 15000, timeoutMsg: "the review's Debug unfolded" });
}

/** The request's state as the review's unfolded Debug section says it (`Request {id} · {status}
 * · …`); "" while Debug is folded or no review is shown. */
export function requestState(browser: WebdriverIO.Browser, selector = '[aria-label="Human review"]'): Promise<string> {
  return browser.execute((query) => {
    const term = [...document.querySelectorAll(`${query} dt`)].find((dt) => dt.textContent?.trim() === "State");
    return term?.nextElementSibling?.textContent?.trim() ?? "";
  }, selector);
}
