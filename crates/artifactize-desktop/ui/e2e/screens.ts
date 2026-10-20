// The screen switch in the window's header, as the app shows it.

/** The chosen screen's name. Review's waiting count after its name is not part of it. */
export function activeScreen(browser: WebdriverIO.Browser): Promise<string> {
  return browser.execute(() => {
    const tab = document.querySelector('[role="tablist"][aria-label="Screens"] [role="tab"][data-state="active"]');
    return (tab?.textContent ?? "").replace(/ \d+$/, "");
  });
}

/** The Review tab as it reads: "Review", or "Review N" while N Human reviews wait. */
export function reviewTab(browser: WebdriverIO.Browser): Promise<string> {
  return browser.execute(() =>
    [...document.querySelectorAll('[role="tablist"][aria-label="Screens"] [role="tab"]')]
      .map((tab) => tab.textContent ?? "")
      .find((text) => /^Review(?: |$)/.test(text)) ?? "");
}
