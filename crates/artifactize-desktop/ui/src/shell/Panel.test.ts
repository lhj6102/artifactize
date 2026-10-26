// A pane's notice: one line in its header until a click shows it in full, and a click folds it.
// That it never widens the pane is layout, which the end-to-end tests measure in the real app.
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { createRawSnippet } from "svelte";
import { afterEach, expect, it } from "vitest";
import Panel from "./Panel.svelte";

afterEach(cleanup);

it("shows the whole notice on a click and folds it on the next", async () => {
  const notice = "Unreadable run run-a: unknown variant `dependency`; Unreadable run run-b: unknown variant `dependency`";
  render(Panel, {
    title: "Run run-c",
    focused: false,
    notice,
    noticeOpen: false,
    children: createRawSnippet(() => ({ render: () => "<p>graph</p>" })),
  });
  const status = screen.getByRole("status");
  const toggle = screen.getByRole("button", { name: notice });
  expect(status.contains(toggle)).toBe(true);
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  expect(status.classList.contains("open")).toBe(false);
  await fireEvent.click(toggle);
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
  expect(status.classList.contains("open")).toBe(true);
  await fireEvent.click(toggle);
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
});
