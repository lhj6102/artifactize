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
