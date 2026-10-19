// One WebDriver page-input adapter for standard events omitted by the pristine provider.
// This delivers only input to actual UI targets: no app handlers, commands or state access.
type Input = { kind: "key"; key: "PageUp" | "PageDown" } | { kind: "click" | "text-click"; x: number; y: number }
  | { kind: "pointer"; phase: "down" | "move" | "up"; x: number; y: number };

/** Click the actual option through WebDriver, then supply only its missing HTML default
 * action: W3C Element Click's selectedness, input, change steps. A conforming provider
 * already selected it and needs no second input. App state and handlers are never touched. */
export async function clickOption(browser: WebdriverIO.Browser, target: WebdriverIO.Element): Promise<void> {
  const eligible = await browser.execute((option: HTMLOptionElement) => {
    if (option.tagName !== "OPTION") throw new Error("option-click requires an option element");
    const select = option.closest("select");
    if (!select || select.multiple) throw new Error("option-click requires a single-choice select");
    if (select.disabled || option.disabled || option.closest("optgroup")?.disabled || option.selected) return false;
    const page = window as unknown as { optionClicks?: WeakMap<HTMLOptionElement, Event> };
    page.optionClicks ??= new WeakMap();
    option.addEventListener("click", (event) => page.optionClicks!.set(option, event), { once: true, capture: true });
    return true;
  }, target as unknown as HTMLOptionElement);
  await target.click();
  if (!eligible) return;
  await browser.execute((option: HTMLOptionElement) => {
    const page = window as unknown as { optionClicks?: WeakMap<HTMLOptionElement, Event> };
    const event = page.optionClicks?.get(option);
    page.optionClicks?.delete(option);
    if (!event) throw new Error("the option click did not reach its target");
    const select = option.closest("select");
    if (!select || event.defaultPrevented || select.disabled || option.disabled || option.closest("optgroup")?.disabled || option.selected) return;
    option.selected = true;
    select.dispatchEvent(new Event("input", { bubbles: true }));
    select.dispatchEvent(new Event("change", { bubbles: true }));
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
      const keyCode = input.key === "PageUp" ? 33 : 34;
      for (const type of ["keydown", "keyup"]) target.dispatchEvent(new KeyboardEvent(type, {
        key: input.key, code: input.key, keyCode, which: keyCode, bubbles: true, cancelable: true,
      }));
    } else {
      const target = document.elementFromPoint(input.x, input.y);
      if (!target) throw new Error("the pointer's page position has no hit target");
      const common = { view: window, clientX: input.x, clientY: input.y, button: 0, detail: 1, bubbles: true, cancelable: true, composed: true };
      const pointer = { ...common, pointerId: 1, pointerType: "mouse", isPrimary: true, width: 1, height: 1 };
      if (input.kind === "pointer") {
        const buttons = input.phase === "up" ? 0 : 1;
        target.dispatchEvent(new PointerEvent(`pointer${input.phase}`, { ...pointer, buttons, pressure: buttons ? 0.5 : 0 }));
        target.dispatchEvent(new MouseEvent(`mouse${input.phase}`, { ...common, buttons }));
        return;
      }
      const pointerDown = target.dispatchEvent(new PointerEvent("pointerdown", { ...pointer, buttons: 1, pressure: 0.5 }));
      const mouseDown = target.dispatchEvent(new MouseEvent("mousedown", { ...common, buttons: 1 }));
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
      target.dispatchEvent(new PointerEvent("pointerup", { ...pointer, buttons: 0, pressure: 0 }));
      target.dispatchEvent(new MouseEvent("mouseup", { ...common, buttons: 0 }));
      target.dispatchEvent(new MouseEvent("click", { ...common, buttons: 0 }));
    }
  }, input);
}
