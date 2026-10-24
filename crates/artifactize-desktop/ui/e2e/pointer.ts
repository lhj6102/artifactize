import { dispatchInput } from "./input.ts";

/** An already visible target's viewport centre, without WebDriver's element-origin
 * scrollIntoView. Scrolling a transformed graph moves its hit target; centring a tool
 * button also scrolls the review before an assertion about the log's own scrolling. */
export function visibleCenter(browser: WebdriverIO.Browser, element: WebdriverIO.Element): Promise<{ x: number; y: number }> {
  return browser.execute((target: HTMLElement) => {
    const box = target.getBoundingClientRect();
    const point = { x: Math.floor(box.left + box.width / 2), y: Math.floor(box.top + box.height / 2) };
    const hit = document.elementFromPoint(point.x, point.y);
    if (!box.width || !box.height || !hit || !target.contains(hit)) {
      // What lies at the centre instead, so a failure says what covers the target.
      const covering = hit && `${hit.tagName.toLowerCase()}.${hit.getAttribute("class") ?? ""}[${hit.getAttribute("aria-label") ?? ""}] ${(hit.textContent ?? "").trim().slice(0, 60)}`;
      throw new Error(`the pointer target's centre is not visible: ${JSON.stringify({
        point, box: [box.left, box.top, box.width, box.height], viewport: [innerWidth, innerHeight], hit: covering,
      })}`);
    }
    return point;
  }, element as unknown as HTMLElement);
}

/** Complete primary-pointer input at the actual page hit target. */
export function clickAt(browser: WebdriverIO.Browser, point: { x: number; y: number }): Promise<void> {
  return dispatchInput(browser, { kind: "click", ...point });
}
