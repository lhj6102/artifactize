// The search field's completion dropdown: candidates follow typing, ↑/↓ choose, Tab and Enter
// take one, Esc closes it, then clears the query, then leaves; compositions are left alone.
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import SearchField from "./SearchField.svelte";
import type { Completion } from "../ipc/client";

afterEach(cleanup);

/** Keys complete to every key name; `kind:` to its values. */
function complete(query: string): Promise<Completion> {
  const space = query.lastIndexOf(" ") + 1;
  const keep = query.slice(0, space);
  const term = query.slice(space);
  const all = term.startsWith("kind:")
    ? ["kind:agent", "kind:dependency", "kind:human", "kind:runtime"]
    : ["repo:", "eval:", "kind:", "status:"];
  return Promise.resolve({ keep, candidates: all.filter((candidate) => candidate.startsWith(term)) });
}

function setup(value = "") {
  const onleave = vi.fn();
  render(SearchField, { value, complete, onleave });
  const field = screen.getByLabelText<HTMLInputElement>("Search requests");
  field.focus();
  return { field, onleave };
}

async function type(field: HTMLInputElement, value: string) {
  await fireEvent.input(field, { target: { value } });
}

describe("SearchField", () => {
  it("offers keys, then values of a taken key, and Tab or Enter take the chosen one", async () => {
    const { field } = setup();
    await type(field, "k");
    expect((await screen.findAllByRole("option")).map((option) => option.textContent?.trim())).toEqual(["kind:"]);
    expect(field.getAttribute("aria-expanded")).toBe("true");
    await fireEvent.keyDown(field, { key: "Tab" });
    expect(field.value).toBe("kind:");
    // A key keeps the dropdown open for its values.
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(4));
    await fireEvent.keyDown(field, { key: "ArrowDown" });
    await fireEvent.keyDown(field, { key: "ArrowDown" });
    expect(screen.getAllByRole("option")[1].getAttribute("aria-selected")).toBe("true");
    await fireEvent.keyDown(field, { key: "ArrowUp" });
    await fireEvent.keyDown(field, { key: "Enter" });
    expect(field.value).toBe("kind:agent ");
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("Esc closes the dropdown, then clears the query, then leaves for the list", async () => {
    const { field, onleave } = setup();
    await type(field, "kind:h");
    await screen.findByRole("listbox");
    await fireEvent.keyDown(field, { key: "Escape" });
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(field.value).toBe("kind:h");
    await fireEvent.keyDown(field, { key: "Escape" });
    expect(field.value).toBe("");
    expect(onleave).not.toHaveBeenCalled();
    await fireEvent.keyDown(field, { key: "Escape" });
    expect(onleave).toHaveBeenCalledOnce();
  });

  it("Enter without suggestions leaves, and keys mid-composition belong to the input method", async () => {
    const { field, onleave } = setup("eval:");
    await fireEvent.keyDown(field, { key: "Escape", isComposing: true });
    await fireEvent.keyDown(field, { key: "Enter", keyCode: 229 });
    expect(field.value).toBe("eval:");
    expect(onleave).not.toHaveBeenCalled();
    await fireEvent.keyDown(field, { key: "Enter" });
    expect(onleave).toHaveBeenCalledOnce();
  });
});

describe("SearchField after a space", () => {
  it("offers keys for an empty term only when asked with ↓", async () => {
    const { field } = setup();
    await type(field, "kind:human ");
    await new Promise((done) => setTimeout(done, 0));
    expect(screen.queryByRole("listbox")).toBeNull();
    await fireEvent.keyDown(field, { key: "ArrowDown" });
    await waitFor(() => expect(screen.getAllByRole("option")).toHaveLength(4));
  });
});

describe("SearchField while typing", () => {
  it("a space ends the term and closes the suggestions at once", async () => {
    const { field, onleave } = setup();
    await type(field, "kind:h");
    await screen.findByRole("listbox");
    await type(field, "kind:h ");
    expect(screen.queryByRole("listbox")).toBeNull();
    await fireEvent.keyDown(field, { key: "Enter" });
    expect(onleave).toHaveBeenCalledOnce();
  });
});
