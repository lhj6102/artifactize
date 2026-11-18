// One WebDriver page-input adapter for standard events omitted by the pristine provider.
// This delivers only input to actual UI targets: no app handlers, commands or state access.
// The provider's special keys carry no modifier: Shift+Enter arrives as a plain Enter.
type Input = { kind: "key"; key: "PageUp" | "PageDown" } | { kind: "key"; key: "Enter"; shift: true } | { kind: "click" | "text-click"; x: number; y: number }
  | { kind: "pointer"; phase: "down" | "move" | "up"; x: number; y: number };

/** WebDriver Element Click's option path targets its containing select. The
 * upstream generic click skips these steps, so deliver the option sequence once. */
export async function clickOption(browser: WebdriverIO.Browser, target: WebdriverIO.Element): Promise<void> {
  await target.waitForExist();
  await browser.execute((option: HTMLOptionElement) => {
    if (option.tagName !== "OPTION") throw new Error("option-click requires an option element");
    const select = option.closest("select");
    if (!select || select.multiple) throw new Error("option-click requires a single-choice select");
    const { left, top, right, bottom } = select.getBoundingClientRect();
    const common = {
      view: window, clientX: (Math.max(left, 0) + Math.min(right, innerWidth)) / 2,
      clientY: (Math.max(top, 0) + Math.min(bottom, innerHeight)) / 2,
      button: 0, bubbles: true, cancelable: true, composed: true,
    };
    const mouse = (type: string, buttons: number, detail: number) =>
      select.dispatchEvent(new MouseEvent(type, { ...common, buttons, detail }));
    mouse("mouseover", 0, 0);
    mouse("mousemove", 0, 0);
    if (mouse("mousedown", 1, 1)) {
      select.focus();
      if (!select.disabled && !option.disabled && !option.closest("optgroup")?.disabled && !option.selected) {
        select.dispatchEvent(new Event("input", { bubbles: true, composed: true }));
        option.selected = true;
        select.dispatchEvent(new Event("change", { bubbles: true }));
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

/** The provider sends synthetic keys, which never perform a native button's keyboard
 * activation. Supply that default only if the page does not cancel the key. */
export async function buttonKey(browser: WebdriverIO.Browser, key: "Enter" | "Space"): Promise<void> {
  await browser.execute((key) => {
    const target = document.activeElement;
    if (!(target instanceof HTMLButtonElement) || target.disabled) throw new Error("button key requires an enabled focused button");
    const keyCode = key === "Enter" ? 13 : 32;
    const input = { key: key === "Enter" ? "Enter" : " ", code: key, keyCode, which: keyCode, bubbles: true, cancelable: true, composed: true };
    const down = target.dispatchEvent(new KeyboardEvent("keydown", input));
    if (down && key === "Enter") target.click();
    const up = (document.activeElement ?? document.body).dispatchEvent(new KeyboardEvent("keyup", input));
    if (down && up && key === "Space" && document.activeElement === target) target.click();
  }, key);
}

export async function dispatchInput(browser: WebdriverIO.Browser, input: Input): Promise<void> {
  await browser.execute((input) => {
    if (input.kind === "key") {
      const target = document.activeElement ?? document.body;
      const keyCode = input.key === "PageUp" ? 33 : input.key === "PageDown" ? 34 : 13;
      const shiftKey = "shift" in input && input.shift;
      for (const type of ["keydown", "keyup"]) target.dispatchEvent(new KeyboardEvent(type, {
        key: input.key, code: input.key, keyCode, which: keyCode, shiftKey, bubbles: true, cancelable: true,
      }));
    } else {
      const target = document.elementFromPoint(input.x, input.y);
      if (!target) throw new Error("the pointer's page position has no hit target");
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
      const pointerDown = target.dispatchEvent(new PointerEvent("pointerdown", { ...pointer, buttons: 1, pressure: 0.5 }));
      const mouseDown = pointerDown && target.dispatchEvent(new MouseEvent("mousedown", { ...common, buttons: 1 }));
      if (input.kind === "text-click" && pointerDown && mouseDown && !(target as HTMLElement).isContentEditable && !target.closest("input, textarea")) {
        let selecting: Element | null = target;
        while (selecting.parentElement && getComputedStyle(selecting).userSelect === "auto") selecting = selecting.parentElement;
        const selection = getSelection();
        if (selection && getComputedStyle(selecting).userSelect !== "none") {
          if (getComputedStyle(selecting).userSelect === "all") {
            target.closest<HTMLElement>("[tabindex], button, a[href]")?.focus();
            selection.selectAllChildren(selecting);
          } else {
            // A real text mousedown places its caret. el.click()/focus() alone leaves
            // Blink's old textarea selection context active, making Select All a no-op.
            const engine = document as Document & { caretPositionFromPoint?: (x: number, y: number) => { offsetNode: Node; offset: number } | null };
            const caret = engine.caretPositionFromPoint?.(input.x, input.y);
            const range = caret ? null : document.caretRangeFromPoint?.(input.x, input.y);
            const node = caret?.offsetNode ?? range?.startContainer;
            const offset = caret?.offset ?? range?.startOffset;
            if (!node || offset === undefined || !target.contains(node)) throw new Error("the clicked text has no native caret position");
            // Focus can scroll its container. Capture the clicked caret before that
            // native focus step, rather than hit-testing the old point after a scroll.
            target.closest<HTMLElement>("[tabindex], button, a[href]")?.focus();
            selection.collapse(node, offset);
          }
        }
      }
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
