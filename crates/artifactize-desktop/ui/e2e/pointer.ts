// The pristine embedded provider delivers compatibility MouseEvents only. This
// WebDriver page command supplies the PointerEvent fields used by outside-dismiss logic.
export async function clickAt(browser: WebdriverIO.Browser, point: { x: number; y: number }): Promise<void> {
  await browser.execute(({ x, y }) => {
    const target = document.elementFromPoint(x, y);
    if (!target) throw new Error("the pointer's page position has no hit target");
    const common = { clientX: x, clientY: y, button: 0, bubbles: true, cancelable: true, composed: true };
    target.dispatchEvent(new PointerEvent("pointerdown", { ...common, pointerId: 1, pointerType: "mouse", isPrimary: true, buttons: 1 }));
    target.dispatchEvent(new MouseEvent("mousedown", { ...common, buttons: 1 }));
    target.dispatchEvent(new PointerEvent("pointerup", { ...common, pointerId: 1, pointerType: "mouse", isPrimary: true, buttons: 0 }));
    target.dispatchEvent(new MouseEvent("mouseup", { ...common, buttons: 0 }));
    target.dispatchEvent(new MouseEvent("click", { ...common, buttons: 0 }));
  }, point);
}
