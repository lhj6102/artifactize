// One WebDriver page-input adapter for standard events omitted by the pristine provider.
// This delivers only input to actual UI targets: no app handlers, commands or state access.
type Input = { kind: "key"; key: "PageUp" | "PageDown" } | { kind: "click"; x: number; y: number };

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
      const common = { clientX: input.x, clientY: input.y, button: 0, detail: 1, bubbles: true, cancelable: true, composed: true };
      const pointer = { ...common, pointerId: 1, pointerType: "mouse", isPrimary: true, width: 1, height: 1 };
      target.dispatchEvent(new PointerEvent("pointerdown", { ...pointer, buttons: 1, pressure: 0.5 }));
      target.dispatchEvent(new MouseEvent("mousedown", { ...common, buttons: 1 }));
      target.dispatchEvent(new PointerEvent("pointerup", { ...pointer, buttons: 0, pressure: 0 }));
      target.dispatchEvent(new MouseEvent("mouseup", { ...common, buttons: 0 }));
      target.dispatchEvent(new MouseEvent("click", { ...common, buttons: 0 }));
    }
  }, input);
}
