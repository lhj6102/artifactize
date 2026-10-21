// Native command observations use the app's read-only IPC boundary, never a replacement
// invoke or a command dispatched by the observer.
import assert from "node:assert/strict";
import type { CommandActivity } from "../src/ipc/activity.ts";
import { afterRender } from "./keys.ts";

export type Checkpoint = { activity: CommandActivity; query: string | null; queryFailure?: string | null };

export function checkpoint(browser: WebdriverIO.Browser): Promise<Checkpoint> {
  return browser.execute(() => {
    const input = document.querySelector<HTMLInputElement>('input[aria-label="Search branches and reviews"]');
    const shown = !!input && input.getBoundingClientRect().height > 0;
    return {
      activity: window.artifactizeCommands,
      query: shown ? input.value : null,
      queryFailure: shown ? document.querySelector('[aria-label="Branches"] .empty.error')?.textContent ?? null : null,
    };
  });
}

/** Correlate a manual refresh (F5, Ctrl+R or the palette's Refresh) with its successful read
 * shown in the window. This is only a completion barrier; callers must also assert what shows. */
export async function refreshed(browser: WebdriverIO.Browser, before: Checkpoint): Promise<void> {
  assert.notEqual(before.activity.refresh, undefined, "manual Refresh observation requires an E2E test build");
  const generation = before.activity.refresh?.generation ?? 0;
  await browser.waitUntil(async () => {
    const completed = (await checkpoint(browser)).activity.refresh;
    assert.notEqual(completed, undefined, "the E2E manual Refresh observation remains available");
    return completed !== null && completed !== undefined && completed.generation === generation + 1;
  }, { timeout: 15000, timeoutMsg: "manual Refresh did not complete (failed or pending)" });
}

/** Query-changing actions need a *new* matching search, not an older A after A → B → A.
 * A background read of the new query also qualifies: it rendered that exact query. */
export function completed(current: Checkpoint, before?: Checkpoint): boolean {
  if (current.activity.pending !== 0 || current.queryFailure) return false;
  if (current.query === null) return true;
  const search = current.activity.search;
  // Nothing typed yet: the field shows the starting query the first read already used.
  if (search === null) return !before || before.query === current.query;
  if (!search.completed || search.query !== current.query) return false;
  return !before || before.query === current.query || search.sequence > (before.activity.search?.sequence ?? 0);
}

/** Completion of native work followed by its render. A search debounce is not native work
 * yet, so the rendered field must also have a matching, completed search generation. */
export async function commandsRendered(browser: WebdriverIO.Browser, before?: Checkpoint, timeout = 15000): Promise<void> {
  await browser.waitUntil(async () => {
    const first = await checkpoint(browser);
    if (!completed(first, before)) return false;
    await afterRender(browser);
    const after = await checkpoint(browser);
    return completed(after, before) && after.activity.revision === first.activity.revision && after.query === first.query;
  }, { timeout, timeoutMsg: "the current query's native commands and render did not complete" });
}
