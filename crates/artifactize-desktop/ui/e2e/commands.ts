// Native command observations use the app's read-only IPC boundary, never a replacement
// invoke or a command dispatched by the observer.
import assert from "node:assert/strict";
import type { CommandActivity } from "../src/ipc/activity.ts";
import { afterRender } from "./keys.ts";

export type Checkpoint = { activity: CommandActivity; query: string | null; error?: string | null };

export function checkpoint(browser: WebdriverIO.Browser): Promise<Checkpoint> {
  return browser.execute(() => {
    const input = document.querySelector<HTMLInputElement>('input[aria-label="Search requests"]');
    return {
      activity: window.artifactizeCommands,
      query: input && input.getBoundingClientRect().height > 0 ? input.value : null,
      error: input && input.getBoundingClientRect().height > 0
        ? document.querySelector(".review > nav .notice.error, .review .empty.error")?.textContent ?? null : null,
    };
  });
}

/** Correlate the selected manual action with a successful reload committed to the UI.
 * This is only a completion barrier; callers must also assert the actual rows and selection. */
export async function reviewRefreshed(browser: WebdriverIO.Browser, before: Checkpoint, start = 0): Promise<void> {
  assert.notEqual(before.activity.reviewRefresh, undefined, "manual Refresh observation requires an E2E test build");
  const generation = before.activity.reviewRefresh?.generation ?? 0;
  const query = before.query;
  assert.ok(query !== null, "the Review query is visible before selecting Refresh");
  await browser.waitUntil(async () => {
    const after = await checkpoint(browser);
    const completed = after.activity.reviewRefresh;
    assert.notEqual(completed, undefined, "the E2E manual Refresh observation remains available");
    return completed !== null && completed !== undefined && completed.generation === generation + 1
      && completed.query === query && completed.start === start && after.query === query;
  }, { timeout: 15000, timeoutMsg: "manual Refresh did not complete (failed or pending)" });
}

/** Query-changing actions need a *new* matching search, not an older A after A → B → A.
 * A background reload of the new query also qualifies: it rendered that exact query. */
export function completed(current: Checkpoint, before?: Checkpoint): boolean {
  if (current.activity.pending !== 0 || current.error) return false;
  if (current.query === null) return true;
  const search = current.activity.search;
  if (!search?.completed || search.query !== current.query) return false;
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
