// Failure evidence only: no waits, application commands, input or recovery actions.
// Print to CI stdout because disposable runners remove their local output directory.
export async function failureEvidence(browser: WebdriverIO.Browser): Promise<Record<string, unknown>> {
  const evidence: Record<string, unknown> = {};
  const observations: [string, () => Promise<unknown>][] = [
    ["url", () => browser.getUrl()],
    ["document", () => browser.execute(() => ({
      ready: document.readyState,
      title: document.title,
      text: document.body?.innerText,
      errors: (window as Window & { __ARTIFACTIZE_E2E_ERRORS__?: unknown }).__ARTIFACTIZE_E2E_ERRORS__,
    }))],
    ["source", () => browser.getPageSource()],
  ];
  for (const [name, observe] of observations) {
    try { evidence[name] = await observe(); }
    catch (error) { evidence[name] = { observationFailure: String(error) }; }
  }
  return evidence;
}
