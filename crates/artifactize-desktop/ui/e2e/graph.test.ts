// The monitor's Run graph in the real app on CLI-made state: release waits for its Human,
// ship waits for release, docs is GREEN. The graph lights them, follows a submission made by
// another process live without moving any node while zoomed into release, still opens the
// highlighted eval's Detail afterwards, and moves between connected Artifacts by keyboard.
import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { artifactize, create, createChain, createEvals, type Fixture, signoff } from "./fixture.ts";
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

describe("run graph", () => {
  let fixture: Fixture;
  let session: Session;
  let browser: WebdriverIO.Browser;
  /** Each shown node's light and position, and each edge's light. */
  const graph = () =>
    browser.execute(() => ({
      nodes: Object.fromEntries(
        [...document.querySelectorAll<HTMLElement>(".svelte-flow__node")].map((node) => [
          node.dataset.id,
          { light: node.dataset.light, at: node.style.transform },
        ]),
      ),
      edges: Object.fromEntries(
        [...document.querySelectorAll<SVGElement>(".svelte-flow__edge")].map((edge) => [edge.dataset.id, edge.dataset.light]),
      ),
    }));

  before(async () => {
    checkBuild();
    fixture = await create();
    session = await start(fixture);
    browser = session.browser;
    await eventually(async () => (await browser.execute(() => document.body.innerText)).includes("signoff"), "the Run tree");
  });

  after(async () => {
    await session?.stop();
  });

  // UX edges: E-GRAPH-OPEN E-GRAPH-RECOLOUR E-GRAPH-ZOOM-IN E-GRAPH-EVAL-OPEN
  it("lights Artifacts and edges, recolours them in place and opens a zoomed eval after a submission", async () => {
    await browser.$("button*=Graph").click();
    await eventually(async () => Object.keys((await graph()).nodes).length === 3, "three Artifact nodes");
    await eventually(async () => {
      const { nodes, edges } = await graph();
      return (
        nodes["a:release"]?.light === "active" &&
        nodes["a:ship"]?.light === "waiting" &&
        nodes["a:docs"]?.light === "green" &&
        edges["a:release>a:ship"] === "active"
      );
    }, "release in progress, ship waiting, docs GREEN, the edge lit as release");
    const before = (await graph()).nodes;

    // Zoom into release: its evals, the sign-off highlighted and focused.
    await browser.$('.svelte-flow__node[data-id="a:release"]').click();
    const evals = await browser.$('[role="listbox"][aria-label="Evals"]');
    await evals.waitForExist({ timeout: 10000 });
    await eventually(
      async () => (await browser.execute((list) => (list as HTMLElement).innerText, evals)).includes("signoff"),
      "the release evals",
    );
    await eventually(() => browser.execute(() => document.activeElement?.getAttribute("aria-label") === "Evals"), "focus in the evals");

    // Another process signs the release off; the change hint recolours the graph.
    const id = signoff(fixture).id;
    assert.equal(artifactize(fixture, "request", "claim", id, "--reviewer", "alice"), 0);
    assert.equal(
      artifactize(fixture, "request", "submit", id, "--reviewer", "alice", "--verdict", "GREEN", "--fields", '{"approved":true,"comment":"ok"}'),
      0,
    );
    await eventually(async () => {
      const { nodes, edges } = await graph();
      return nodes["a:release"]?.light === "green" && edges["a:release>a:ship"] === "green";
    }, "release and its edge GREEN");
    const after = (await graph()).nodes;
    for (const node of ["a:release", "a:ship", "a:docs"]) {
      assert.equal(after[node].at, before[node].at, `${node} kept its place`);
    }

    // The tree folded the completed Artifact; Enter still opens the highlighted sign-off's
    // Detail (an Artifact's Detail has no Human review).
    const review = () => browser.$('[aria-label="Human review"]').isExisting();
    await press(browser, ["Enter"], review, "the sign-off's Human review");
    // Back to the graph: Esc leaves Detail, a click on the list and Esc zooms out.
    await press(browser, ["Escape"], async () => !(await review()), "Detail left");
    await evals.click();
    await press(browser, ["Escape"], async () => !(await evals.isExisting()), "zoomed out");
  });

  // UX edges: E-GRAPH-MOVE E-GRAPH-CLOSE
  it("moves along edges with the arrow keys and switches back to the tree with v", async () => {
    await eventually(
      () => browser.execute(() => document.activeElement?.closest(".svelte-flow__node")?.getAttribute("data-id") === "a:release"),
      "focus back on release",
    );
    const on = (id: string) => () =>
      browser.execute((want) => document.activeElement?.closest(".svelte-flow__node")?.getAttribute("data-id") === want, id);
    await press(browser, ["ArrowRight"], on("a:ship"), "focus on ship, downstream");
    await press(browser, ["ArrowLeft"], on("a:release"), "focus on release, upstream");
    await press(browser, ["v"], () => browser.$('[role="tree"]').isExisting(), "the tree");
  });
});

describe("run graph of many Artifacts", () => {
  let session: Session;
  let browser: WebdriverIO.Browser;
  const focused = () => browser.execute(() => document.activeElement?.closest(".svelte-flow__node")?.getAttribute("data-id") ?? null);
  const mounted = () => browser.execute(() => document.querySelectorAll(".svelte-flow__node").length);

  before(async () => {
    checkBuild();
    session = await start(createChain(85));
    browser = session.browser;
    await eventually(async () => (await browser.execute(() => document.body.innerText)).includes("n000"), "the Run tree", 30000);
  });

  after(async () => {
    await session?.stop();
  });

  // UX edges: E-GRAPH-BEYOND E-GRAPH-ZOOM-OUT
  it("walks the whole chain beyond the drawn nodes, through a zoom in and out, focus following", async () => {
    await browser.$("button*=Graph").click();
    await eventually(async () => (await mounted()) > 0, "the graph", 30000);
    // Zoom in until only part of the chain is drawn.
    const zoomIn = await browser.$(".svelte-flow__controls-zoomin");
    for (let step = 0; step < 12; step += 1) await zoomIn.click();
    await eventually(async () => (await mounted()) < 85, "nodes outside the view are not drawn");
    await browser.execute(() => {
      const node = [...document.querySelectorAll<HTMLElement>(".svelte-flow__node")].sort((a, b) =>
        (a.dataset.id ?? "").localeCompare(b.dataset.id ?? ""),
      )[0];
      node.focus();
    });
    const name = (index: number) => `a:n${String(index).padStart(3, "0")}`;
    /** Press `key` and wait for focus on `want`. */
    const step = (key: string, want: string) => press(browser, [key], async () => (await focused()) === want, `focus on ${want}`);
    // Back to the start of the chain, then along all of it.
    let at = Number(((await focused()) ?? "").slice(3));
    while (at > 0) await step("ArrowLeft", name((at -= 1)));
    for (let index = 1; index < 85; index += 1) {
      await step("ArrowRight", name(index));
      if (index === 40) {
        // Into the revealed Artifact's evals and back out: focus returns to it and stays
        // through the camera's move, and the keys go on from it.
        await press(
          browser,
          ["Enter"],
          () => browser.execute(() => document.activeElement?.getAttribute("aria-label") === "Evals"),
          "the evals",
        );
        const zoomOut = await browser.$(".svelte-flow__controls-zoomout");
        assert.ok(await zoomOut.isEnabled(), "the camera is zoomed in before Escape");
        await step("Escape", name(40));
        // This chain cannot fit even at minimum zoom. All 85 nodes may briefly mount
        // during the camera move, but culling leaves only the visible portion afterwards.
        // The disabled zoom-out control marks the actual zoom endpoint, not a quiet frame
        // or an elapsed delay; the selected Artifact must still be in view and focused.
        await eventually(async () => {
          const visible = await browser.execute(() => {
            const node = document.querySelector('[data-id="a:n040"].svelte-flow__node');
            const box = node?.getBoundingClientRect();
            const canvas = document.querySelector(".graph .canvas")?.getBoundingClientRect();
            return !document.querySelector('[role="listbox"][aria-label="Evals"]') && document.activeElement === node &&
              !!box && !!canvas && box.width > 0 && box.left >= canvas.left && box.right <= canvas.right && box.top >= canvas.top && box.bottom <= canvas.bottom;
          });
          return visible && !(await zoomOut.isEnabled());
        }, "the zoomed-out graph with n040 visible and focused at the camera's endpoint");
        assert.match(await browser.$('nav[aria-label="Breadcrumb"]').getText(), /n040/, "selection held through the transition");
      }
    }
    // The tree's selection is the focused node: the breadcrumb names it.
    const crumbs = await browser.$('nav[aria-label="Breadcrumb"]').getText();
    assert.match(crumbs, /n084/);
    assert.ok((await mounted()) < 85, "still drawing only part of the chain");
  });
});

describe("run graph of an Artifact with several evals", () => {
  let session: Session;
  let browser: WebdriverIO.Browser;
  const options = () =>
    browser.execute(() =>
      [...document.querySelectorAll('[role="listbox"][aria-label="Evals"] [role="option"]')].map(
        (option) => `${option.getAttribute("aria-selected")}:${(option as HTMLElement).innerText.split("\n")[1] ?? ""}`,
      ),
    );
  const selected = async () => (await options()).findIndex((option) => option.startsWith("true"));
  const evals = () => browser.$('[role="listbox"][aria-label="Evals"]');
  const detailClosed = async () => !(await browser.$("button*=Close").isExisting());

  before(async () => {
    checkBuild();
    session = await start(createEvals(["alpha", "beta", "gamma"]));
    browser = session.browser;
    await eventually(async () => (await browser.execute(() => document.body.innerText)).includes("alpha"), "the Run tree");
    await browser.$("button*=Graph").click();
    await browser.$('.svelte-flow__node[data-id="a:app"]').waitForExist({ timeout: 15000 });
  });

  after(async () => {
    await session?.stop();
  });

  // UX edges: E-GRAPH-ZOOM-IN E-GRAPH-EVAL-SELECT E-GRAPH-EVAL-OPEN E-GRAPH-ZOOM-OUT
  it("selects an eval with the arrows, opens its Detail with Enter and zooms out with Esc", async () => {
    await browser.$('.svelte-flow__node[data-id="a:app"]').click();
    await evals().waitForExist({ timeout: 10000 });
    await eventually(() => browser.execute(() => document.activeElement?.getAttribute("aria-label") === "Evals"), "focus in the evals");
    const first = await selected();
    await press(browser, ["ArrowDown"], async () => (await selected()) === first + 1, "the next eval selected");
    // The Detail follows the selection without being opened.
    const chosen = (await options())[first + 1].split(":")[1].trim();
    // Search only the Monitor's fourth pane: the graph's Evals list already contains
    // every title and cannot establish that the Detail followed the selection.
    const peek = () => browser.execute(() => (document.querySelectorAll(".monitor section.panel")[3] as HTMLElement).innerText);
    await eventually(async () => (await peek()).includes(`Check ${chosen}`), "the Detail pane follows the selected eval");
    assert.match(await peek(), new RegExp(`app/${chosen}`));
    await press(browser, ["Enter"], async () => !(await detailClosed()), "the Detail opened");
    await press(browser, ["Escape"], detailClosed, "the Detail left");
    await evals().click();
    await press(browser, ["Escape"], async () => !(await evals().isExisting()), "zoomed out");
  });
});
