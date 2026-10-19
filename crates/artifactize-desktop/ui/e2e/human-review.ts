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
    const claim = [...(detail?.querySelectorAll("button") ?? [])].find((button) => button.textContent?.trim() === "Claim to review");
    return !!detail && detail.querySelector(".stage")?.textContent?.trim() === "CLAIM" && !!claim && !claim.disabled && !detail.querySelector(".busy");
  }, selector);
}
