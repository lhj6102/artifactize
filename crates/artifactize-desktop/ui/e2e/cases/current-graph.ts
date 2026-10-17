// J-GRAPH-CURRENT: the graph launch reads current declarations, not saved Run state.
import assert from "node:assert/strict";
import { createGraph, type Fixture, savedRuns, verifyWaiting } from "../fixture.ts";
import { press } from "../keys.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { command, script } from "../native-fixtures.ts";
import { nativePath, primaryModifier, readText, writeText } from "../platform.ts";
import { graphAppearance, visibleColour } from "./graph-appearance.ts";

function currentGraph(browser: WebdriverIO.Browser, fixture: Fixture) {
  const structure = () => browser.$(".current pre").getText();

  const expanded = () => browser.execute(() => document.querySelector(".current details")?.hasAttribute("open") ?? false);

  /** Refresh also resets the camera. Wait for a stable, hit-testable real pointer
   * target before one WebDriver click, rather than clicking during that transition. */
  async function clickArtifact(name: string) {
    const selector = `.current .svelte-flow__node[data-id="a:${name}"]`;
    // Where the node, the canvas and the camera were, read only when the node was no target.
    const placed = () => browser.execute((selector) => {
      const node = document.querySelector(selector);
      const box = node?.getBoundingClientRect();
      const canvas = node?.closest(".svelte-flow")?.getBoundingClientRect();
      const hit = box && document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2);
      return {
        window: [innerWidth, innerHeight],
        node: box && [box.left, box.top, box.right, box.bottom].map(Math.round),
        canvas: canvas && [canvas.left, canvas.top, canvas.right, canvas.bottom].map(Math.round),
        camera: document.querySelector(".current .svelte-flow__viewport")?.getAttribute("style"),
        hit: hit ? `${hit.tagName}.${hit.getAttribute("class") ?? ""}` : null,
      };
    }, selector);
    await browser.waitUntil(() => browser.executeAsync<boolean, [string]>((selector, done) => {
      const node = document.querySelector(selector);
      if (!node) return done(false);
      const before = node.getBoundingClientRect();
      requestAnimationFrame(() => {
        const after = node.getBoundingClientRect();
        const canvas = node.closest(".svelte-flow")?.getBoundingClientRect();
        const hit = document.elementFromPoint(after.x + after.width / 2, after.y + after.height / 2);
        done(node.isConnected && before.x === after.x && before.y === after.y && before.width === after.width && before.height === after.height &&
          !!canvas && after.left >= canvas.left && after.right <= canvas.right && after.top >= canvas.top && after.bottom <= canvas.bottom &&
          hit?.closest(".svelte-flow__node") === node);
      });
    }, selector), { timeout: 15000, timeoutMsg: `${name} is a stable, visible graph pointer target` }).catch(async (error: unknown) => {
      throw new Error(`${name} is a stable, visible graph pointer target: ${JSON.stringify(await placed())}`, { cause: error });
    });
    await browser.$(selector).click();
  }

  /** Observe the actual fixture graph, not just its declarations-only caption. */
  async function assertDeclarationsOnly() {
    await browser.$('.current .svelte-flow__edge[data-id="a:release>a:ship"]').waitForExist({ timeout: 15000 });
    const drawn = await browser.execute(() => ({
      nodes: [...document.querySelectorAll<HTMLElement>(".current .svelte-flow__node")].map((node) => ({
        id: node.dataset.id,
        label: node.getAttribute("aria-label"),
        neutral: node.dataset.light === "waiting" && node.querySelector(".artifact")?.classList.contains("light-waiting"),
      })).sort((a, b) => (a.id ?? "").localeCompare(b.id ?? "")),
      edges: [...document.querySelectorAll<SVGElement>(".current .svelte-flow__edge")].map((edge) => ({
        id: edge.dataset.id,
        neutral: edge.dataset.light === "waiting" && edge.classList.contains("light-waiting"),
      })),
    }));
    assert.deepEqual(drawn, {
      nodes: ["docs", "release", "ship"].map((name) => ({ id: `a:${name}`, label: `${name}: current structure`, neutral: true })),
      edges: [{ id: "a:release>a:ship", neutral: true }],
    }, "the drawn declarations and relation are neutral, without Run verdict colors or status labels");
    await browser.waitUntil(async () => {
      const appearance = await graphAppearance(browser);
      const nodes = Object.values(appearance.nodes);
      const edges = Object.values(appearance.edges);
      const neutral = nodes[0]?.dot ?? "";
      return nodes.length === 3 && edges.length === 1 && visibleColour(neutral) &&
        nodes.every((node) => node.painted && node.border === neutral && node.dot === neutral) &&
        edges.every((edge) => edge.painted && edge.stroke === neutral);
    }, { timeout: 15000, timeoutMsg: "current declarations and their relation are actually painted in one neutral colour, without verdict differentiation" });
    assert.deepEqual(savedRuns(fixture), [], "current graph does not create a saved Run, whether or not an empty database exists");
  }
  return { structure, expanded, clickArtifact, assertDeclarationsOnly };
}

export const scenarios: Scenario[] = [
  // J-GRAPH-CURRENT: E-CURRENT-LAUNCH → E-CURRENT-EVALS → E-CURRENT-STRUCTURE → E-CURRENT-CLOSE → E-CURRENT-REFRESH
  scenario<Record<string, never>>(
    "current-graph-refresh",
    "opens without a saved Run, docks an Artifact's evals on a click, expands complete criteria and relations, closes the evals, then refreshes changed declarations",
    async () => {
      const fixture = createGraph();
      return { fixture, args: ["graph", "--repo", fixture.repo], data: {} };
    },
    async (browser, { fixture }) => {
      const { structure, expanded, clickArtifact, assertDeclarationsOnly } = currentGraph(browser, fixture);
      await browser.$('.svelte-flow__node[data-id="a:release"]').waitForExist({ timeout: 15000 });
      const text = await browser.execute(() => document.body.innerText);
      assert.match(text, /Current Artifact graph/);
      assert.match(text, /Declarations only · no Run verdicts/);
      assert.equal(await browser.$(".legend").getText(), "Current structure · arrows: input → consumer");
      await assertDeclarationsOnly();
      await clickArtifact("release");
      const evals = await browser.$('aside[aria-label="Detail"] [aria-label="Evals of release"]');
      await evals.waitForExist({ timeout: 10000 });
      assert.match(await evals.getText(), /A person approves the release/);
      // The evals dock over the graph's right side; a click on one exposes its criteria, not a
      // historical request Detail.
      const docked = await browser.execute(() => {
        const dock = document.querySelector('.current aside[aria-label="Detail"]')!.getBoundingClientRect();
        const canvas = document.querySelector(".current .canvas")!.getBoundingClientRect();
        return dock.left > canvas.left && dock.right < canvas.right && dock.top >= canvas.top && dock.bottom <= canvas.bottom;
      });
      assert.ok(docked, "the evals float within the graph, docked toward its right edge with a gap");
      assert.equal(await expanded(), false, "the structure text starts folded");
      await evals.$('button[data-eval="e:release/signoff"]').click();
      await browser.waitUntil(expanded, { timeout: 15000, timeoutMsg: "structure text expanded from the clicked eval" });
      const shown = await structure();
      const criteria: Record<string, unknown> = {};
      let evalId = "";
      for (const line of shown.split("\n")) {
        const heading = /├─ ([^:]+):/.exec(line);
        if (heading) evalId = heading[1];
        const payload = /criteria: (\{.*\})$/.exec(line);
        if (payload) criteria[evalId] = JSON.parse(payload[1]);
      }
      assert.deepEqual(criteria, {
        "release/signoff": {
          target: "release", title: "A person approves the release", gate: null,
          criteria: { kind: "human" }, payload: { instruction: "Read the release notes and approve them." },
          passSchema: { type: "object", properties: { approved: { const: true },
            comment: { type: "string", description: "What you checked." } },
            required: ["approved", "comment"], additionalProperties: false },
          failSchema: { type: "object", properties: { reason: { type: "string", minLength: 3 },
            findings: { type: "array", items: { type: "object", properties: { problem: { type: "string", minLength: 1 }, severity: { enum: ["low", "high"] } },
              required: ["problem"], additionalProperties: false } } },
            required: ["reason"], additionalProperties: false },
        },
        "ship/check": { target: "ship", title: "Ship the release", gate: null,
          criteria: { kind: "runtime", command, args: [script, "pass"] },
          payload: { instruction: "Ship {release}." }, passSchema: null, failSchema: null },
        "docs/lint": { target: "docs", title: "Docs build", gate: null,
          criteria: { kind: "runtime", command, args: [script, "pass"] },
          payload: { instruction: "Build the docs." }, passSchema: null, failSchema: null },
      }, "every independently declared criterion and owner schema is exposed");
      const relations = shown.split("\n").filter((line) => line.includes(" → "));
      assert.deepEqual(relations, [
        'release → ship {"kind":"instruction","eval_id":"ship/check","name":"release"}',
      ], "the concrete input-to-consumer relation carries its instruction label");
      assert.equal(await browser.$('[aria-label="Human review"]').isExisting(), false);
      // Esc closes the evals; Close does too.
      const dock = () => browser.$('.current aside[aria-label="Detail"]');
      await press(browser, ["Escape"], async () => !(await dock().isExisting()), "Esc closed release's evals");
      await clickArtifact("release");
      await dock().waitForExist({ timeout: 15000, timeoutMsg: "release's evals docked again" });
      await dock().$("button*=Close").click();
      await dock().waitForExist({ reverse: true, timeout: 15000, timeoutMsg: "Close closed release's evals" });

      // Continue the same launch, evals and complete-structure journey: the text remains
      // expanded while each real refresh reloads this window's changed declaration.
      const declaration = nativePath(fixture.repo, "docs", "index.artf");
      let previousTitle = "Docs build";
      for (const [index, keys] of [null, [primaryModifier, "r"], ["F5"]].entries()) {
        const title = `Current docs criterion ${index}`;
        writeText(declaration, readText(declaration).replace(/title = "[^"]*"/, `title = "${title}"`));
        if (keys) await press(browser, keys, async () => (await structure()).includes(title), `declarations refreshed with ${keys.join("+")}`);
        else {
          await browser.$("button=Refresh").click();
          await browser.waitUntil(async () => (await structure()).includes(title), { timeout: 15000 });
        }
        await browser.$("button=Refresh").waitForEnabled({ timeout: 15000 });
        // Use the real node control after refresh; observation never installs focus.
        await clickArtifact("docs");
        await browser.waitUntil(() => browser.execute((expected) =>
          document.querySelector('[aria-label="Evals of docs"]')?.textContent?.includes(expected) ?? false,
        title), { timeout: 15000, timeoutMsg: `the refreshed docs node exposes ${title}` });
        const currentEvals = await browser.$('[aria-label="Evals of docs"]').getText();
        assert.match(currentEvals, new RegExp(title));
        assert.ok(!currentEvals.includes(previousTitle), "the graph's evals replaced the previous declaration title");
        const currentStructure = await structure();
        assert.match(currentStructure, new RegExp(title));
        assert.ok(!currentStructure.includes(previousTitle), "the structure text replaced the previous declaration title");
        assert.match(await browser.execute(() => document.body.innerText), /Declarations only · no Run verdicts/);
        await assertDeclarationsOnly();
        // Close docs' evals so the next refresh's camera has every node in view and reachable.
        await press(browser, ["Escape"], async () => !(await dock().isExisting()), "docs' evals closed");
        previousTitle = title;
      }
      // Only after every declarations-only/no-Run check, create a real Run in this
      // same state. Its independently submitted identity proves the absence probe
      // would have detected a Run had the graph launch or refresh created one.
      const controlRun = await verifyWaiting(fixture);
      assert.deepEqual(savedRuns(fixture).map((run) => run.id), [controlRun], "the same saved-Run probe detects this fixture's real CLI-created Run");
    },
  ),
];
