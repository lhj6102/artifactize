// The one input helper (E2E-INPUT): what a user's input does that the pristine provider
// omits, delivered to the actual UI target as the engine would (an option click as WebDriver
// specifies it, Tab's focus move, pointer events at a point), and the test-only native
// text-input device. It never calls app handlers or commands or reads app state.
// The provider's special keys carry no modifier: Shift+Enter arrives as a plain Enter.
type Input = { kind: "key"; key: "PageUp" | "PageDown" } | { kind: "key"; key: "Enter"; shift: true } | { kind: "click"; x: number; y: number }
  | { kind: "pointer"; phase: "down" | "move" | "up"; x: number; y: number; target?: string };

/** WebDriver Element Click's option path targets its containing select. The
 * upstream generic click skips these steps, so deliver the option sequence once. */
export async function clickOption(browser: WebdriverIO.Browser, target: WebdriverIO.Element): Promise<void> {
  await target.waitForExist();
  await browser.execute((option: HTMLOptionElement) => {
    if (option.tagName !== "OPTION") throw new Error("option-click requires an option element");
    const select = option.closest("select");
    if (!select || select.multiple) throw new Error("option-click requires a single-choice select");
    const { left, top, right, bottom } = select.getBoundingClientRect();
    const x0 = Math.max(left, 0), x1 = Math.min(right, innerWidth);
    const y0 = Math.max(top, 0), y1 = Math.min(bottom, innerHeight);
    const clientX = (x0 + x1) / 2, clientY = (y0 + y1) / 2;
    const hit = document.elementFromPoint(clientX, clientY);
    if (x0 >= x1 || y0 >= y1 || !hit || !select.contains(hit) || getComputedStyle(select).visibility !== "visible") {
      throw new Error("the option's select is outside the viewport or obscured");
    }
    const common = {
      view: window, clientX, clientY,
      button: 0, bubbles: true, cancelable: true, composed: true,
    };
    const mouse = (type: string, buttons: number, detail: number) =>
      select.dispatchEvent(new MouseEvent(type, { ...common, buttons, detail }));
    mouse("mouseover", 0, 0);
    mouse("mousemove", 0, 0);
    if (mouse("mousedown", 1, 1)) {
      select.focus();
      if (!select.disabled && !option.disabled && !option.closest("optgroup")?.disabled) {
        // As a user's choice: the selection changes first, and an unchanged one fires nothing.
        if (!option.selected) {
          option.selected = true;
          select.dispatchEvent(new Event("input", { bubbles: true, composed: true }));
          select.dispatchEvent(new Event("change", { bubbles: true }));
        }
      }
    }
    mouse("mouseup", 0, 1);
    select.dispatchEvent(new PointerEvent("click", {
      ...common, detail: 1, pointerId: 1, pointerType: "mouse", isPrimary: true, width: 1, height: 1,
    }));
  }, target as unknown as HTMLOptionElement);
}

/** Supply only Tab's omitted browser default, after the page has had its keydown. */
export async function tab(browser: WebdriverIO.Browser, shift = false): Promise<void> {
  await browser.execute((shift) => {
    const active = document.activeElement as HTMLElement | null;
    const key = { key: "Tab", code: "Tab", keyCode: 9, which: 9, shiftKey: shift, bubbles: true, cancelable: true, composed: true };
    const allowed = (active ?? document.body).dispatchEvent(new KeyboardEvent("keydown", key));
    if (allowed) {
      const visible = (element: HTMLElement) => {
        if (element.matches(":disabled") || element.closest("[inert], [hidden], dialog:not([open])") || element.getClientRects().length === 0) return false;
        const style = getComputedStyle(element);
        if (style.visibility === "hidden" || style.visibility === "collapse" || style.display === "none") return false;
        for (let parent = element.parentElement; parent; parent = parent.parentElement) {
          if (parent.tagName === "DETAILS" && !(parent as HTMLDetailsElement).open
            && ![...parent.children].find((child) => child.tagName === "SUMMARY")?.contains(element)) return false;
        }
        return true;
      };
      const ordered = [...document.querySelectorAll<HTMLElement>("a[href], area[href], input, select, textarea, button, iframe, object, embed, summary, [tabindex], [contenteditable]")]
        .map((element) => ({ element, order: element.hasAttribute("tabindex") ? element.tabIndex : element.isContentEditable ? 0 : element.tabIndex }))
        .filter(({ element, order }) => order >= 0 && visible(element))
        // sort is stable, so equal tab indices retain document tree order.
        .sort((a, b) => a.order > 0 && b.order > 0 ? a.order - b.order : a.order > 0 ? -1 : b.order > 0 ? 1 : 0)
        .map(({ element }) => element);
      const index = ordered.indexOf(active!);
      const after = active && active !== document.body && index < 0 ? ordered.filter((element) => active.compareDocumentPosition(element)
        & (shift ? Node.DOCUMENT_POSITION_PRECEDING : Node.DOCUMENT_POSITION_FOLLOWING)) : [];
      const next = index >= 0 ? ordered[(index + (shift ? -1 : 1) + ordered.length) % ordered.length]
        : shift ? after.at(-1) ?? ordered.at(-1) : after[0] ?? ordered[0];
      next?.focus();
    }
    (document.activeElement ?? document.body).dispatchEvent(new KeyboardEvent("keyup", key));
  }, shift);
}

export async function dispatchInput(browser: WebdriverIO.Browser, input: Input): Promise<void> {
  await browser.execute((input) => {
    if (input.kind === "key") {
      const target = document.activeElement ?? document.body;
      const keyCode = input.key === "PageUp" ? 33 : input.key === "PageDown" ? 34 : 13;
      const shiftKey = "shift" in input && input.shift;
      const send = (type: string, key: string, code: string, keyCode: number, shift: boolean) => target.dispatchEvent(new KeyboardEvent(type, {
        key, code, keyCode, which: keyCode, shiftKey: shift, bubbles: true, cancelable: true,
      }));
      // A chord as WebDriver's key actions deliver it: the modifier goes down first and up last.
      if (shiftKey) send("keydown", "Shift", "ShiftLeft", 16, true);
      for (const type of ["keydown", "keyup"]) send(type, input.key, input.key, keyCode, shiftKey);
      if (shiftKey) send("keyup", "Shift", "ShiftLeft", 16, false);
    } else {
      const target = document.elementFromPoint(input.x, input.y);
      if (!target) throw new Error(`the pointer has no hit target at (${input.x}, ${input.y}) in a ${innerWidth}×${innerHeight} viewport`);
      if (input.kind === "pointer" && input.phase === "down" && input.target && !target.matches(input.target)) {
        throw new Error(`expected the pointer to hit ${input.target} at (${input.x}, ${input.y}); hit ${target.tagName}.${target.getAttribute("class") ?? ""}`);
      }
      const common = { view: window, clientX: input.x, clientY: input.y, button: 0, detail: 1, bubbles: true, cancelable: true, composed: true };
      const pointer = { ...common, pointerId: 1, pointerType: "mouse", isPrimary: true, width: 1, height: 1 };
      // A cancelled pointerdown suppresses the gesture's compatibility mouse events until its
      // pointerup, as the engine does for a real primary pointer; its click still follows.
      const gesture = window as Window & { e2eMouseSuppressed?: boolean; e2ePressed?: Element | null };
      if (input.kind === "pointer") {
        const buttons = input.phase === "up" ? 0 : 1;
        const move = input.phase === "move";
        const allowed = target.dispatchEvent(new PointerEvent(`pointer${input.phase}`, {
          ...pointer, button: move ? -1 : 0, detail: 0, buttons, pressure: buttons ? 0.5 : 0,
        }));
        if (input.phase === "down") {
          gesture.e2eMouseSuppressed = !allowed;
          gesture.e2ePressed = target;
        }
        if (!gesture.e2eMouseSuppressed) target.dispatchEvent(new MouseEvent(`mouse${input.phase}`, { ...common, detail: move ? 0 : 1, buttons }));
        if (input.phase === "up") {
          gesture.e2eMouseSuppressed = false;
          // As the engine does, the release clicks the nearest element holding both where the
          // button went down and where it came up, still in the page, however far it moved.
          let shared: Element | null = gesture.e2ePressed?.isConnected ? gesture.e2ePressed : null;
          while (shared && !shared.contains(target)) shared = shared.parentElement;
          gesture.e2ePressed = null;
          shared?.dispatchEvent(new PointerEvent("click", { ...pointer, buttons: 0, pressure: 0 }));
        }
        return;
      }
      const pointerDown = target.dispatchEvent(new PointerEvent("pointerdown", { ...pointer, detail: 0, buttons: 1, pressure: 0.5 }));
      if (pointerDown) target.dispatchEvent(new MouseEvent("mousedown", { ...common, buttons: 1 }));
      const released = document.elementFromPoint(input.x, input.y);
      if (!released) return;
      released.dispatchEvent(new PointerEvent("pointerup", { ...pointer, detail: 0, buttons: 0, pressure: 0 }));
      if (pointerDown) released.dispatchEvent(new MouseEvent("mouseup", { ...common, buttons: 0 }));
      let shared: Element | null = target.isConnected ? target : null;
      while (shared && !shared.contains(released)) shared = shared.parentElement;
      shared?.dispatchEvent(new PointerEvent("click", { ...pointer, buttons: 0, pressure: 0 }));
    }
  }, input);
}

/** The evaluated test-build device reaches only the engine's text-input API. This
 * test-only IPC command is not an application action and is absent from production. */
export async function nativeInput(browser: WebdriverIO.Browser, input:
  | { type: "set" | "commit"; text: string }
  | { type: "cancel" | "copy" | "paste" }): Promise<void> {
  // Through the page's own `invoke`, as the app reaches its commands; the same in a session
  // the Tauri service started and in one of an app a scenario started again.
  const answer = await browser.executeAsync((operation: typeof input, done: (answer: { error?: string }) => void) => {
    type Api = { __TAURI__: { core: { invoke: (command: string, args: unknown) => Promise<unknown> } } };
    (window as unknown as Api).__TAURI__.core.invoke("e2e_input", { input: operation }).then(() => done({}), (error: unknown) => done({ error: String(error) }));
  }, input);
  if (answer.error !== undefined) throw new Error(`native engine input failed: ${answer.error}`);
}

/** An already visible target's viewport centre, without WebDriver's element-origin
 * scrollIntoView. Scrolling a transformed graph moves its hit target; centring a tool
 * button also scrolls the review before an assertion about the log's own scrolling.
 * Waits until the centre takes the pointer: a closed modal's body lock lifts on a timer
 * after the dialog has left the page, and until then nothing under it is hit. */
export async function visibleCenter(browser: WebdriverIO.Browser, element: WebdriverIO.Element, ms = 15000): Promise<{ x: number; y: number }> {
  let found: Awaited<ReturnType<typeof centre>> = {};
  try {
    await browser.waitUntil(async () => {
      found = await centre(browser, element);
      return found.point !== undefined;
    }, { timeout: ms, interval: 50, timeoutMsg: "timed out: the pointer target's centre takes the pointer" });
  } catch (error) {
    throw new Error(`the pointer target's centre is not visible: ${found.failure}`, { cause: error });
  }
  return found.point!;
}

function centre(browser: WebdriverIO.Browser, element: WebdriverIO.Element): Promise<{ point?: { x: number; y: number }; failure?: string }> {
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
    if (box.width && box.height && hit && target.contains(hit)) return { point };
    // What lies at the centre instead, so a failure says what covers the target or why
    // nothing there takes the pointer: every element at the point, the target's ancestors that
    // refuse pointer events, and its own state attributes.
    const name = (node: Element) => `${node.tagName.toLowerCase()}.${node.getAttribute("class") ?? ""}[${node.getAttribute("aria-label") ?? ""}]`;
    const refusing: string[] = [];
    for (let node: Element | null = target; node; node = node.parentElement) {
      if (getComputedStyle(node).pointerEvents === "none") refusing.push(name(node));
    }
    return {
      failure: JSON.stringify({
        point, box: [box.left, box.top, box.width, box.height], viewport: [innerWidth, innerHeight],
        hit: hit && `${name(hit)} ${(hit.textContent ?? "").trim().slice(0, 60)}`,
        stack: document.elementsFromPoint(point.x, point.y).map(name),
        refusing, attributes: [...target.attributes].map((attribute) => `${attribute.name}=${attribute.value}`),
      }),
    };
  }, element as unknown as HTMLElement);
}

/** Supply the omitted wheel default for a nested viewport (upstream scrolls only window). */
export async function scrollViewport(browser: WebdriverIO.Browser, viewport: WebdriverIO.Element, dy: number): Promise<void> {
  const point = await visibleCenter(browser, viewport);
  await browser.execute((target, { x, y }, dy) => {
    const hit = document.elementFromPoint(x, y);
    if (!hit || !target.contains(hit)) throw new Error("the viewport is obscured");
    if (hit.dispatchEvent(new WheelEvent("wheel", { clientX: x, clientY: y, deltaY: dy, bubbles: true, cancelable: true }))) {
      target.scrollBy(0, dy);
    }
  }, viewport as unknown as HTMLElement, point, dy);
}

/** Click an unlabeled part of the row's trailing gutter, not its text or another control. */
export async function clickRowGutter(browser: WebdriverIO.Browser, row: WebdriverIO.Element, paneSelector: string): Promise<void> {
  await row.waitForDisplayed();
  const point = await browser.execute((target: HTMLElement, selector) => {
    const box = target.getBoundingClientRect();
    const pane = target.closest<HTMLElement>(selector);
    const frame = pane?.getBoundingClientRect();
    const scroller = [".branches", ".sessions", '[role="listbox"]'].map((part) => target.closest<HTMLElement>(part)).find(Boolean);
    // Exclude the scroll gutter, not the rest of the Pane's clickable width.
    const scrollbar = scroller ? scroller.offsetWidth - scroller.clientWidth : 0;
    if (!frame || box.left > frame.left + 2 || box.right < frame.right - scrollbar - 2) {
      throw new Error(`the row does not span its Pane: row ${box.left}–${box.right}, pane ${frame?.left}–${frame?.right}, scroll gutter ${scrollbar}`);
    }
    const right = frame.right - scrollbar;
    const text = document.createTreeWalker(target, NodeFilter.SHOW_TEXT);
    const bounds: DOMRect[] = [];
    for (let node = text.nextNode(); node; node = text.nextNode()) {
      if (!node.textContent?.trim()) continue;
      const range = document.createRange();
      range.selectNodeContents(node);
      bounds.push(...range.getClientRects());
    }
    for (const offset of [4, 8, 12, 24, 40, 56]) {
      for (const portion of [0.5, 0.75, 0.25, 0.9, 0.1]) {
        const x = Math.floor(right - offset), y = Math.floor(box.top + box.height * portion);
        const hit = document.elementFromPoint(x, y);
        const control = hit?.closest("button, a, input");
        if (!hit || !target.contains(hit) || (control && control !== target && target.contains(control))) continue;
        if (!bounds.some((rect) => x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom)) return { x, y };
      }
    }
    return null;
  }, row as unknown as HTMLElement, paneSelector);
  if (!point) throw new Error("no unlabeled point in the row's trailing gutter is visible");
  await clickAt(browser, point);
}

/** Use the provider's ordinary WebDriver mouse action at a viewport point. */
export async function clickAt(browser: WebdriverIO.Browser, point: { x: number; y: number }): Promise<void> {
  await browser.action("pointer").move({ ...point, origin: "viewport" }).down().up().perform();
}

/** Use only for controls whose press handler needs the omitted pointerdown. */
export function pointerGapClick(browser: WebdriverIO.Browser, point: { x: number; y: number }): Promise<void> {
  return dispatchInput(browser, { kind: "click", ...point });
}
