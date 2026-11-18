// Observe the actual app child opened by the graph tool. The runner config
// provides its reserved WebDriver endpoint and connects the observation session.
import type { Candidate } from "./build.ts";
import type { Fixture } from "./fixture.ts";
import { recordAdmission, sessionEndedByApp } from "./observation.ts";
import { branches, mainTitle } from "./pane.ts";
import { observe } from "./domain-fixtures/observe.ts";
import {
  asNativePath, canonicalPath, desktopLaunchRelay, embeddedReady, exists, nativePath, requestWindowClose, writeJson,
  type Environment,
} from "./platform.ts";

export const graphToolObservation = (fixture: Fixture) => nativePath(fixture.root, "graph-tool-window.json");

export async function graphToolSession(candidate: Candidate, fixture: Fixture, parent: Environment, port: number,
  handoff: () => Promise<void>, connect: () => Promise<WebdriverIO.Browser>) {
  const repo = canonicalPath(nativePath(fixture.root, "graph-worktree"));
  return desktopLaunchRelay(fixture.root, parent, candidate.app, port, async (args, cwd) => {
    if (args.length !== 3 || args[0] !== "graph" || args[1] !== "--repo"
      || canonicalPath(asNativePath(args[2])) !== repo || canonicalPath(cwd) !== repo) {
      throw new Error(`graph handoff passed unexpected arguments or working directory: ${JSON.stringify({ args, cwd })}`);
    }
    await handoff();
    const [owner] = recordAdmission(candidate.root, ["graph-tool-companion"]);
    return { owner, observe: async () => {
      await observe(() => exists(owner) ? embeddedReady(port) : false,
        () => "the tool-opened app did not start its embedded WebDriver");
      const window = await connect();
      let shown: { title: string; selection: string[]; artifacts: string[] } | undefined;
      let failure: unknown;
      try {
        const worktree = "repo  graph-tool";
        const marker = "graph-marker: would run (1 eval)";
        await window.waitUntil(async () => {
          const title = await mainTitle(window);
          const selection = (await branches(window)).filter((entry) => entry.selected).map((entry) => entry.name);
          const artifacts = await window.execute(() => [...document.querySelectorAll<HTMLElement>('section[aria-label="Current"] .svelte-flow__node')]
            .filter((node) => node.checkVisibility({ visibilityProperty: true }) && node.getBoundingClientRect().width > 0)
            .map((node) => node.getAttribute("aria-label") ?? ""));
          shown = { title, selection, artifacts };
          return title === `Current · ${worktree}` && selection.length === 1 && selection[0] === worktree && artifacts.includes(marker);
        }, { timeout: 15000, timeoutMsg: "the actual companion did not select and visibly draw the sibling worktree" });
      } catch (error) { failure = error; }
      try { await sessionEndedByApp(window, () => requestWindowClose(window)); }
      catch (error) {
        if (failure !== undefined) throw new AggregateError([failure, error], "observing and closing the companion failed");
        throw error;
      }
      if (failure !== undefined) throw failure;
      writeJson(graphToolObservation(fixture), shown);
    } };
  });
}
