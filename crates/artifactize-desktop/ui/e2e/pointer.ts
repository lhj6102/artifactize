import { dispatchInput } from "./input.ts";

/** An already visible target's viewport centre, without WebDriver's element-origin
 * scrollIntoView. Scrolling a transformed graph moves its hit target; centring a tool
 * button also scrolls the review before an assertion about the log's own scrolling. */
export function visibleCenter(browser: WebdriverIO.Browser, element: WebdriverIO.Element): Promise<{ x: number; y: number }> {
  return browser.execute((target: HTMLElement) => {
    const box = target.getBoundingClientRect();
    // A whole pixel inside the box: flooring the centre of a 1px divider at x 126.33 gives 126,
    // just outside it.
    const inside = (start: number, size: number) => {
      const centre = Math.floor(start + size / 2);
      return centre >= start ? centre : Math.ceil(start);
    };
    const point = { x: inside(box.left, box.width), y: inside(box.top, box.height) };
    const hit = document.elementFromPoint(point.x, point.y);
    if (!box.width || !box.height || !hit || !target.contains(hit)) {
      // What lies at the centre instead, so a failure says what covers the target or why
      // nothing there takes the pointer: every element at the point, the target's ancestors that
      // refuse pointer events, and its own state attributes.
      const name = (node: Element) => `${node.tagName.toLowerCase()}.${node.getAttribute("class") ?? ""}[${node.getAttribute("aria-label") ?? ""}]`;
      const refusing: string[] = [];
      for (let node: Element | null = target; node; node = node.parentElement) {
        if (getComputedStyle(node).pointerEvents === "none") refusing.push(name(node));
      }
      throw new Error(`the pointer target's centre is not visible: ${JSON.stringify({
        point, box: [box.left, box.top, box.width, box.height], viewport: [innerWidth, innerHeight],
        hit: hit && `${name(hit)} ${(hit.textContent ?? "").trim().slice(0, 60)}`,
        stack: document.elementsFromPoint(point.x, point.y).map(name),
        refusing, attributes: [...target.attributes].map((attribute) => `${attribute.name}=${attribute.value}`),
      })}`);
    }
    return point;
  }, element as unknown as HTMLElement);
}

/** Complete primary-pointer input at the actual page hit target. */
export function clickAt(browser: WebdriverIO.Browser, point: { x: number; y: number }): Promise<void> {
  return dispatchInput(browser, { kind: "click", ...point });
}
