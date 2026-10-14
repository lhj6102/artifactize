// The Run graph with every kind of verdict, and the Detail of a finished eval: node and edge
// colours for RED, ERROR and blocked Artifacts, the arrows between nodes of one column, `v`
// between the tree and the graph, and the instruction, request, result and technical record
// of a completed eval.
import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { createStates, requests, type Fixture } from "./fixture.ts";
import { checkBuild, type Session, start } from "./driver.ts";
import { press } from "./keys.ts";

async function eventually(check: () => Promise<boolean> | boolean, what: string, ms = 15000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await check()) return;
    await new Promise((done) => setTimeout(done, 100));
  }
  assert.fail(`timed out: ${what}`);
}

describe("a Run with every verdict", () => {
  let fixture: Fixture;
  let session: Session;
  let browser: WebdriverIO.Browser;
  const text = () => browser.execute(() => document.body.innerText);
  const lights = () =>
    browser.execute(() => ({
      nodes: Object.fromEntries([...document.querySelectorAll<HTMLElement>(".svelte-flow__node")].map((node) => [node.dataset.id, node.dataset.light])),
      edges: Object.fromEntries([...document.querySelectorAll<SVGElement>(".svelte-flow__edge")].map((edge) => [edge.dataset.id, edge.dataset.light])),
    }));
  const focused = () => browser.execute(() => document.activeElement?.closest(".svelte-flow__node")?.getAttribute("data-id") ?? null);
  const tree = () => browser.$('[role="tree"]');

  before(async () => {
    checkBuild();
    fixture = await createStates();
    session = await start(fixture);
    browser = session.browser;
    await eventually(async () => (await text()).includes("crash"), "the Run tree");
  });

  after(async () => {
    await session?.stop();
  });

  // UX edges: E-GRAPH-OPEN E-GRAPH-CLOSE
  it("switches between the tree and the graph with v", async () => {
    await tree().click();
    await press(browser, ["v"], () => browser.$(".svelte-flow__node").isExisting(), "the graph");
    assert.ok(!(await tree().isExisting()), "the graph replaces the tree");
    // The tree's list left with the switch; the graph's own node takes the keys.
    await browser.execute(() => document.querySelector<HTMLElement>(".svelte-flow__node")?.focus());
    await press(browser, ["v"], () => tree().isExisting(), "the tree again");
    await tree().click();
    await press(browser, ["v"], () => browser.$(".svelte-flow__node").isExisting(), "the graph once more");
  });

  // UX edges: E-GRAPH-OPEN
  it("colours RED, ERROR, blocked and GREEN Artifacts and the edge between a RED one and its dependent", async () => {
    // Svelte Flow can mount nodes before its asynchronous layout renders the edges.
    await eventually(async () => {
      const graph = await lights();
      return Object.keys(graph.nodes).length === 7 && Object.keys(graph.edges).length === 3;
    }, "seven Artifact nodes and their three rendered edges");
    const { nodes, edges } = await lights();
    assert.deepEqual(nodes, {
      "a:red": "red",
      "a:crash": "error",
      "a:down": "blocked",
      "a:after": "blocked",
      "a:fine": "green",
      "a:gate": "active",
      "a:later": "waiting",
    });
    // Each edge is lit as what its source passes on: RED, ERROR, or work in progress.
    assert.deepEqual(edges, { "a:red>a:down": "red", "a:crash>a:after": "error", "a:gate>a:later": "active" });
    const legend = await browser.$('[aria-label="Legend"]').getText();
    for (const word of ["GREEN", "RED", "ERROR", "blocked"]) assert.match(legend, new RegExp(word));
  });

  // UX edges: E-GRAPH-MOVE
  it("moves along edges with ← and →, and between the nodes of one column with ↑ and ↓", async () => {
    const on = (id: string) => async () => (await focused()) === id;
    await browser.$('.svelte-flow__node[data-id="a:red"]').click();
    // A click zooms in; step back out to the whole graph with the node focused.
    await press(browser, ["Escape"], async () => !(await browser.$('[role="listbox"][aria-label="Evals"]').isExisting()), "zoomed out");
    await eventually(on("a:red"), "focus on red");
    // → the dependent, ← back upstream.
    await press(browser, ["ArrowRight"], on("a:down"), "focus on down, downstream");
    await press(browser, ["ArrowLeft"], on("a:red"), "focus on red, upstream");
    // ↑ ↓ walk the nodes of a column from top to bottom and back.
    const placed = await browser.execute(() =>
      [...document.querySelectorAll<HTMLElement>(".svelte-flow__node")].map((node) => {
        const at = /translate\(([-\d.]+)px,\s*([-\d.]+)px\)/.exec(node.style.transform);
        return { id: node.dataset.id ?? "", x: Number(at?.[1] ?? 0), y: Number(at?.[2] ?? 0) };
      }),
    );
    const byColumn = new Map<number, { id: string; y: number }[]>();
    for (const node of placed) byColumn.set(Math.round(node.x), [...(byColumn.get(Math.round(node.x)) ?? []), node]);
    const order = [...byColumn.values()]
      .find((nodes) => nodes.length >= 2)!
      .sort((a, b) => a.y - b.y)
      .map((node) => node.id);
    assert.ok(order.length >= 2, "a column of at least two nodes");
    await browser.execute((id) => document.querySelector<HTMLElement>(`.svelte-flow__node[data-id="${id}"]`)?.focus(), order[0]);
    await eventually(on(order[0]), `focus on ${order[0]}`);
    for (const next of order.slice(1)) {
      await press(browser, ["ArrowDown"], on(next), `focus on ${next}, below`);
      const crumbs = await browser.$('nav[aria-label="Breadcrumb"]').getText();
      assert.ok(crumbs.includes(next.slice(2)), `the breadcrumb names ${next}: ${crumbs}`);
    }
    await press(browser, ["ArrowUp"], on(order[order.length - 2]), `focus on ${order[order.length - 2]}, above`);
  });
});

describe("the Detail of a finished eval", () => {
  let fixture: Fixture;
  let session: Session;
  let browser: WebdriverIO.Browser;
  const text = () => browser.execute(() => document.body.innerText);

  before(async () => {
    checkBuild();
    fixture = await createStates();
    session = await start(fixture);
    browser = session.browser;
    await eventually(async () => (await text()).includes("fine"), "the Run tree");
  });

  after(async () => {
    await session?.stop();
  });

  // UX edges: E-MON-DETAIL-TECHNICAL E-MON-TREE-DETAIL
  it("shows the instruction, the request and the result, and the technical record only on demand", async () => {
    const fine = '//li[@role="treeitem"][@aria-level="1"][.//span[contains(@class,"name")][starts-with(normalize-space(.), "fine")]]';
    await browser.$(fine).click();
    // A finished Artifact is folded: unfold it, then its eval is the row after it.
    await press(browser, ["l"], async () => (await browser.$(fine).getAttribute("aria-expanded")) === "true", "fine unfolded");
    const check = await browser.$(`${fine}/following-sibling::li[@role="treeitem"][1]`);
    await check.click();
    await press(browser, ["Enter"], () => browser.$("button*=Close").isExisting(), "the Detail opened");
    const shown = await text();
    assert.match(shown, /Pass\./, `the instruction in ${shown.slice(0, 1500)}`);
    assert.match(shown, /Source\s+executed in this Run/, "where the result came from");
    assert.match(shown, /GREEN/, "the result");
    const request = requests(fixture).find((saved) => saved.evalId === "fine/check");
    assert.ok(request, "the real CLI saved the completed fine/check request");
    // A per-Run TMPDIR can itself contain run-... and fixture-<digits>; only the
    // actual request ID tests Technical disclosure, not a regex across any path.
    assert.equal(shown.includes(request.id), false, "the request is part of the technical record");
    const technical = () => browser.$("button*=Technical").getAttribute("aria-expanded");
    await press(browser, ["t"], async () => (await technical()) === "true", "the technical record shown");
    assert.equal((await text()).includes(request.id), true, "the technical record shows the exact request");
    await press(browser, ["t"], async () => (await technical()) === "false", "the technical record hidden");
    assert.equal((await text()).includes(request.id), false, "the exact request disappears again");
  });
});

describe("a blocked eval", () => {
  let session: Session;
  let browser: WebdriverIO.Browser;

  before(async () => {
    checkBuild();
    session = await start(await createStates());
    browser = session.browser;
    await eventually(async () => (await browser.execute(() => document.body.innerText)).includes("down"), "the Run tree");
  });

  after(async () => {
    await session?.stop();
  });

  // UX edges: E-MON-BLOCKER
  it("jumps from the eval blocked by a RED Artifact to that Artifact with b", async () => {
    const down = '//li[@role="treeitem"][@aria-level="1"][.//span[contains(@class,"name")][starts-with(normalize-space(.), "down")]]';
    // `down` waits on `red`, which failed: its eval is the row after it.
    await browser.$(`${down}/following-sibling::li[@role="treeitem"][1]`).click();
    const selected = () => browser.$('[role="treeitem"][aria-selected="true"]');
    assert.match(await selected().getText(), /blocked by red/);
    await press(
      browser,
      ["b"],
      async () => (await selected().getAttribute("aria-level")) === "1" && /\bred\b/.test(await selected().getText()),
      "the RED Artifact selected",
    );
  });
});
