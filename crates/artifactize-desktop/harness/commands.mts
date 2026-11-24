// The harness waits for the app's native work through its read-only IPC activity record, never a replacement
// invoke or a command dispatched by the observer.
import type { CommandActivity, SearchActivity } from "../ui/src/ipc/activity.ts";
import { afterRender } from "../ui/e2e/keys.ts";

export type Checkpoint = { activity: CommandActivity; query: string | null; reviewQuery: string | null; queryFailure?: string | null; reviewFailure?: string | null };

export function checkpoint(browser: WebdriverIO.Browser): Promise<Checkpoint> {
  return browser.execute(() => {
    const input = document.querySelector<HTMLInputElement>('input[aria-label="Search the branches"]');
    const shown = !!input && input.getBoundingClientRect().height > 0;
    const filter = document.querySelector<HTMLInputElement>('input[aria-label="Filter the reviews"]');
    return {
      reviewQuery: filter && filter.getBoundingClientRect().height > 0 ? filter.value : null,
      activity: window.artifactizeCommands,
      query: shown ? input.value : null,
      reviewFailure: filter && filter.getBoundingClientRect().height > 0 ? document.querySelector('main[aria-label="Review"] header .notice[role="status"]')?.textContent?.trim() ?? null : null,
      queryFailure: shown ? document.querySelector('[aria-label="Branches"] .empty.error')?.textContent ?? null : null,
    };
  });
}

/** Query-changing actions need a *new* matching search, not an older A after A → B → A.
 * A background read of the new query also qualifies: it rendered that exact query. */
function queryRendered(query: string | null, search: SearchActivity | null, failure: string | null | undefined, before?: { query: string | null; search: SearchActivity | null }): boolean {
  if (query === null) return true;
  const changed = before?.query != null && before.query !== query;
  // A field that just appeared may show its starting query without dispatching another search.
  if (search === null) return !changed && !failure;
  if (!search.completed || search.query !== query) return false;
  // An invalid query has a terminal visible error rather than result rows. A rejected
  // command with no rendered explanation is not a terminal render.
  if (search.failed && !failure) return false;
  return !changed || search.sequence > (before.search?.sequence ?? 0);
}

export function completed(current: Checkpoint, before?: Checkpoint): boolean {
  if (current.activity.pending !== 0) return false;
  return queryRendered(current.query, current.activity.search, current.queryFailure, before && { query: before.query, search: before.activity.search })
    && queryRendered(current.reviewQuery, current.activity.reviewSearch, current.reviewFailure, before && { query: before.reviewQuery, search: before.activity.reviewSearch });
}

/** Completion of native work followed by its render. A search debounce is not native work
 * yet, so the rendered field must also have a matching, completed search generation. */
export async function commandsRendered(browser: WebdriverIO.Browser, before?: Checkpoint, timeout = 15000): Promise<void> {
  await browser.waitUntil(async () => {
    const first = await checkpoint(browser);
    if (!completed(first, before)) return false;
    await afterRender(browser);
    const after = await checkpoint(browser);
    return completed(after, before) && after.activity.revision === first.activity.revision && after.query === first.query && after.reviewQuery === first.reviewQuery
      && after.queryFailure === first.queryFailure && after.reviewFailure === first.reviewFailure;
  }, { timeout, timeoutMsg: "the current query's native commands and render did not complete" });
}
