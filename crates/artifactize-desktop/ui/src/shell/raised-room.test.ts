import { expect, it, vi } from "vitest";
import { raisedRoom } from "./raised-room";

it("sizes both upward and downward menus inside the nearest clipping panel, even after scrolling", () => {
  const outer = document.createElement("div");
  const inner = document.createElement("section");
  const anchor = document.createElement("div");
  const menu = document.createElement("div");
  outer.append(inner);
  inner.append(anchor);
  anchor.append(menu);
  document.body.append(outer);
  outer.style.overflowX = outer.style.overflowY = "hidden";
  inner.style.overflowX = inner.style.overflowY = "auto";
  const box = (left: number, top: number, width: number, height: number) => new DOMRect(left, top, width, height);
  const place = (element: HTMLElement, rect: DOMRect) => {
    vi.spyOn(element, "getBoundingClientRect").mockReturnValue(rect);
    Object.defineProperties(element, { clientWidth: { value: rect.width }, clientHeight: { value: rect.height } });
  };
  place(outer, box(0, 0, 600, 500));
  place(inner, box(80, 100, 320, 380));
  const location = vi.spyOn(anchor, "getBoundingClientRect").mockReturnValue(box(120, 360, 140, 24));
  const above = raisedRoom("above")(menu);
  try {
    expect(menu.style.getPropertyValue("--raised-height")).toBe("256px");
    expect(menu.style.getPropertyValue("--raised-width")).toBe("276px");
    location.mockReturnValue(box(120, 310, 140, 24));
    inner.dispatchEvent(new Event("scroll"));
    expect(menu.style.getPropertyValue("--raised-height")).toBe("206px");
  } finally { above?.(); }
  const below = raisedRoom("below")(menu);
  try {
    expect(menu.style.getPropertyValue("--raised-height")).toBe("142px");
  } finally {
    below?.();
    outer.remove();
  }
});
