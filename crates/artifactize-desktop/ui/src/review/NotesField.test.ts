// The Notes field: the lines Point appended stand out over the draft's text, which the field
// keeps as one plain text area.
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import NotesField from "./NotesField.svelte";
import { pointedLines } from "./pointed";

afterEach(cleanup);

describe("pointedLines", () => {
  it("marks the lines that are a pointed text, whatever surrounds them", () => {
    expect(pointedLines("Looks fine.\n#checkout .total\nL4:C2", ["#checkout .total", " L4:C2 "])).toEqual([
      { text: "Looks fine.", pointed: false },
      { text: "#checkout .total", pointed: true },
      { text: "L4:C2", pointed: true },
    ]);
    expect(pointedLines("", [""])).toEqual([{ text: "", pointed: false }]);
  });
});

describe("NotesField", () => {
  it("draws no mirror while nothing is pointed", () => {
    const { container } = render(NotesField, { value: "Just text.", pointed: [], oninput: () => {} });
    expect(container.querySelector(".backdrop")).toBeNull();
    expect(screen.getByRole<HTMLTextAreaElement>("textbox").value).toBe("Just text.");
  });

  it("marks a pointed line in the mirror behind the area, which keeps the whole text", async () => {
    const oninput = vi.fn();
    const { container } = render(NotesField, { value: "Totals read right.\nrelease.md L2:C4", pointed: ["release.md L2:C4"], oninput });
    const marks = [...container.querySelectorAll(".backdrop mark")].map((mark) => mark.textContent);
    expect(marks).toEqual(["release.md L2:C4"]);
    expect(container.querySelector(".backdrop")?.getAttribute("aria-hidden")).toBe("true");
    const area = screen.getByRole<HTMLTextAreaElement>("textbox");
    expect(area.value).toBe("Totals read right.\nrelease.md L2:C4");
    await fireEvent.input(area, { target: { value: "Totals read right.\nrelease.md L2:C4 ok" } });
    expect(oninput).toHaveBeenCalledWith("Totals read right.\nrelease.md L2:C4 ok");
  });
});
