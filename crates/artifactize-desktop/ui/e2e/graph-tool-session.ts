// Observe the actual product-launched window; only the runner config supplies its endpoint.
import type { Fixture } from "./fixture.ts";
import { sessionEndedByApp } from "./observation.ts";
import { MAIN } from "./pane.ts";
import { nativePath, requestWindowClose, writeJson } from "./platform.ts";

export const graphToolObservation = (fixture: Fixture) => nativePath(fixture.root, "graph-tool-window.json");
export const graphToolFailure = (fixture: Fixture) => nativePath(fixture.root, "graph-tool-failure.txt");

export async function observeGraphTool(fixture: Fixture, window: WebdriverIO.Browser): Promise<void> {
  let shown: { title: string; selection: string[]; artifacts: string[] } | undefined;
  let failure: unknown;
  try {
    const worktree = "repo  graph-tool";
    const marker = "graph-marker: would run (1 eval)";
    await window.waitUntil(async () => {
      try {
        // WKWebView may accept a session while its new window is still starting.
        // Read one snapshot per poll instead of racing three native script calls.
        shown = await window.execute((main) => {
          const words = (element: Element | null) => element?.textContent?.trim() ?? "";
          const title = words(document.querySelector(`${main} > header h2`));
          const selection = [...document.querySelectorAll('[aria-label="Branches"] > [role="treeitem"]')]
            .filter((branch) => branch.getAttribute("aria-selected") === "true" || branch.querySelector('[role="treeitem"][aria-selected="true"]'))
            .map((branch) => `${words(branch.querySelector(".project")).replace(/ \/$/, "")}  ${words(branch.querySelector(".branch-name"))}`);
          const artifacts = [...document.querySelectorAll<HTMLElement>('section[aria-label="Current"] .svelte-flow__node')]
            .filter((node) => node.checkVisibility({ visibilityProperty: true }) && node.getBoundingClientRect().width > 0)
            .map((node) => node.getAttribute("aria-label") ?? "");
          return { title, selection, artifacts };
        }, MAIN);
      } catch (error) {
        if (!(error instanceof Error) || !/script execution timed out|request timed out/i.test(error.message)) throw error;
        return false;
      }
      return shown.title === `Current · ${worktree}` && shown.selection.length === 1 && shown.selection[0] === worktree && shown.artifacts.includes(marker);
    }, { timeout: 15000, timeoutMsg: "the actual companion did not select and visibly draw the sibling worktree" });
  } catch (error) { failure = error; }
  try { await sessionEndedByApp(window, () => requestWindowClose(window)); }
  catch (error) {
    if (failure !== undefined) throw new AggregateError([failure, error], "observing and closing the companion failed");
    throw error;
  }
  if (failure !== undefined) throw failure;
  writeJson(graphToolObservation(fixture), shown);
}
