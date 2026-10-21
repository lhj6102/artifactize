// The actual context/IPC boundary: consumers react to one owner, roots remain isolated,
// and completion or unmount never writes into another window's observation projection.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, expect, it } from "vitest";
import { tick } from "svelte";
import type { CommandActivity } from "./activity";
import ActivityHost from "./ActivityHost.svelte";
import { browseView } from "../monitor/fixtures";

const results = browseView(1);

/** Pane searches wait for the test to answer them; any other browse answers at once. */
function backend() {
  const pending: { query: string; resolve: (value: unknown) => void; reject: (error: unknown) => void }[] = [];
  mockIPC((name, args) => {
    if (name !== "browse") throw new Error(`Unexpected command: ${name}`);
    const query = (args as { navigate: { query: string | null } }).navigate.query;
    if (query === null) return browseView(1);
    return new Promise((resolve, reject) => pending.push({ query, resolve, reject }));
  });
  return pending;
}

function shown(label: string): CommandActivity {
  return JSON.parse(screen.getByLabelText(`${label} activity`).textContent!);
}

afterEach(() => { cleanup(); clearMocks(); });

it("reacts through shared context to dispatch, reverse completion and failure without retriggering view attachments", async () => {
  const pending = backend();
  const view = render(ActivityHost, { label: "A", query: "public query" });
  await waitFor(() => expect(pending).toHaveLength(2));
  expect(shown("A first")).toEqual(shown("A second"));
  expect(shown("A first").commands.browse).toEqual({ started: 2, completed: 0, failed: 0, pending: 2 });
  const before = window.artifactizeCommands;
  expect(before.search).toEqual({ query: "public query", sequence: 2, completed: false });
  expect(Object.getOwnPropertyDescriptor(window, "artifactizeCommands")?.set).toBeUndefined();
  expect(Object.isFrozen(before)).toBe(true);
  expect(Object.isFrozen(before.commands.browse)).toBe(true);
  expect(Object.isFrozen(before.search)).toBe(true);

  pending[1].reject({ kind: "missing", message: "Native search failed" });
  await waitFor(() => expect(shown("A first").commands.browse.completed).toBe(1));
  expect(shown("A second").search?.completed).toBe(true);
  await waitFor(() => expect(screen.getAllByRole("alert").map((alert) => alert.textContent)).toEqual(["Native search failed", "Native search failed"]));
  pending[0].resolve(results);
  await waitFor(() => expect(shown("A first").pending).toBe(0));
  expect(shown("A first")).toEqual(shown("A second"));
  expect(window.artifactizeCommands.commands.browse).toEqual({ started: 2, completed: 2, failed: 0, pending: 0 });
  expect(before.commands.browse.pending).toBe(2);
  expect(before.search?.completed).toBe(false);
  expect(pending).toHaveLength(2);
  expect(JSON.stringify(window.artifactizeCommands)).not.toContain("Native search failed");

  // A helper created by AppState and a component captured from context update the same owner.
  await fireEvent.click(screen.getByRole("button", { name: "A first browse" }));
  await waitFor(() => expect(shown("A second").commands.browse).toEqual({ started: 3, completed: 3, failed: 0, pending: 0 }));
  expect(window.artifactizeCommands.revision).toBe(6);
  view.unmount();
  expect(Object.hasOwn(window, "artifactizeCommands")).toBe(false);
});

it("keeps roots and captured async commands isolated when an older root unmounts", async () => {
  const pending = backend();
  const aSubmissions: Promise<void>[] = [];
  const a = render(ActivityHost, {
    label: "A", query: "A", onsubmit: (completion) => { aSubmissions.push(completion); },
  });
  await waitFor(() => expect(pending).toHaveLength(2));
  const b = render(ActivityHost, { label: "B", query: "B" });
  await waitFor(() => expect(pending).toHaveLength(4));
  const bProjection = window.artifactizeCommands;
  expect(bProjection.search?.query).toBe("B");
  expect(bProjection.commands.browse.started).toBe(2);

  // A's captured client must still dispatch to A after B becomes the current root.
  await fireEvent.click(screen.getByRole("button", { name: "A first search" }));
  await waitFor(() => expect(shown("A second").commands.browse.started).toBe(3));
  expect(shown("B first")).toEqual(bProjection);
  expect(window.artifactizeCommands).toBe(bProjection);
  expect(pending.map((entry) => entry.query)).toEqual(["A", "A", "B", "B", "A"]);
  a.unmount();
  expect(window.artifactizeCommands).toBe(bProjection);
  pending[4].resolve(results);
  pending[1].reject({ kind: "missing", message: "Disposed A failed" });
  pending[0].resolve(results);
  // Wait for every disposed A submission, including its normal UI catch, before flushing
  // the DOM. A Svelte tick alone does not mean a pending native command has completed.
  expect(aSubmissions).toHaveLength(3);
  await Promise.all(aSubmissions);
  await tick();
  expect(screen.queryByText("Disposed A failed")).toBeNull();
  expect(shown("B first")).toEqual(bProjection);
  expect(shown("B second")).toEqual(bProjection);
  expect(window.artifactizeCommands).toBe(bProjection);
  pending[3].resolve(results);
  pending[2].resolve(results);
  await waitFor(() => expect(shown("B second").pending).toBe(0));
  expect(window.artifactizeCommands.commands.browse).toEqual({ started: 2, completed: 2, failed: 0, pending: 0 });
  expect(window.artifactizeCommands.search).toEqual({ query: "B", sequence: 2, completed: true });
  b.unmount();
  expect(Object.hasOwn(window, "artifactizeCommands")).toBe(false);

  const c = render(ActivityHost, { label: "C", query: "C" });
  await waitFor(() => expect(pending).toHaveLength(7));
  expect(window.artifactizeCommands.commands.browse).toEqual({ started: 2, completed: 0, failed: 0, pending: 2 });
  expect(window.artifactizeCommands.search).toEqual({ query: "C", sequence: 2, completed: false });
  pending[5].resolve(results);
  pending[6].resolve(results);
  await waitFor(() => expect(shown("C first").pending).toBe(0));
  c.unmount();
  expect(Object.hasOwn(window, "artifactizeCommands")).toBe(false);
});
