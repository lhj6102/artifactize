// The monitor's Run graph in the real app on CLI-made state: release waits for its Human,
// ship waits for release, docs is GREEN. The graph lights them, follows a submission made by
// another process live without moving any node, zooms into an Artifact's evals and opens the
// Detail of one, and moves between connected Artifacts with the keyboard.
import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { artifactize, create, type Fixture, signoff } from "./fixture.ts";
import { type Session, start } from "./driver.ts";

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
    fixture = create();
    session = await start(fixture);
    browser = session.browser;
    await eventually(async () => (await browser.execute(() => document.body.innerText)).includes("signoff"), "the Run tree");
  });

  after(async () => {
    await session?.stop();
  });

  it("lights Artifacts and edges and recolours them in place after a submission", async () => {
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

    // Another process signs the release off; the change hint redraws the graph.
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
  });

  it("zooms into an Artifact, lists its evals and opens the Detail of one", async () => {
    await browser.$('.svelte-flow__node[data-id="a:release"]').click();
    const evals = await browser.$('[role="listbox"][aria-label="Evals"]');
    await evals.waitForExist({ timeout: 10000 });
    await eventually(
      async () => (await browser.execute((list) => (list as HTMLElement).innerText, evals)).includes("signoff"),
      "the release evals",
    );
    await eventually(() => browser.execute(() => document.activeElement?.getAttribute("aria-label") === "Evals"), "focus in the evals");
    await browser.keys(["Enter"]);
    await browser.$('[aria-label="Human review"]').waitForExist({ timeout: 10000 });
    // Back to the graph: Esc leaves Detail, a click on the list and Esc zooms out.
    await browser.keys(["Escape"]);
    await browser.$('[aria-label="Human review"]').waitForExist({ reverse: true, timeout: 10000 });
    await evals.click();
    await browser.keys(["Escape"]);
    await evals.waitForExist({ reverse: true, timeout: 10000 });
  });

  it("moves along edges with the arrow keys and switches back to the tree with v", async () => {
    await eventually(
      () => browser.execute(() => document.activeElement?.closest(".svelte-flow__node")?.getAttribute("data-id") === "a:release"),
      "focus back on release",
    );
    await browser.keys(["ArrowRight"]);
    await eventually(
      () => browser.execute(() => document.activeElement?.closest(".svelte-flow__node")?.getAttribute("data-id") === "a:ship"),
      "focus on ship, downstream",
    );
    await browser.keys(["ArrowLeft"]);
    await eventually(
      () => browser.execute(() => document.activeElement?.closest(".svelte-flow__node")?.getAttribute("data-id") === "a:release"),
      "focus on release, upstream",
    );
    await browser.keys(["v"]);
    await browser.$('[role="tree"]').waitForExist({ timeout: 10000 });
  });
});
