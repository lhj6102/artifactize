import { dispatchInput } from "./input.ts";

/** Complete primary-pointer input at the actual page hit target. */
export function clickAt(browser: WebdriverIO.Browser, point: { x: number; y: number }): Promise<void> {
  return dispatchInput(browser, { kind: "click", ...point });
}
