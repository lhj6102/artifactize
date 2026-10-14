// J-GRAPH-CURRENT: the graph launch reads current declarations, not saved Run state.
import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { after, before, describe, it } from "node:test";
import { createGraph, type Fixture } from "./fixture.ts";
import { start, type Session } from "./driver.ts";
import { press } from "./keys.ts";

describe("current declaration graph", () => {
  let fixture: Fixture;
  let session: Session;
  let browser: WebdriverIO.Browser;
  const structure = () => browser.$(".current pre").getText();
  const expanded = () => browser.execute(() => document.querySelector(".current details")?.hasAttribute("open") ?? false);

  before(async () => {
    fixture = createGraph();
    session = await start(fixture, "graph", "--repo", fixture.repo);
    browser = session.browser;
    await browser.$('.svelte-flow__node[data-id="a:release"]').waitForExist({ timeout: 15000 });
  });
  after(async () => { await session?.stop(); });

  // UX edges: E-CURRENT-LAUNCH E-CURRENT-ZOOM E-CURRENT-STRUCTURE
  it("opens without a saved Run, zooms into evals and exposes complete criteria and labeled relations", async () => {
    const text = await browser.execute(() => document.body.innerText);
    assert.match(text, /Current Artifact graph/);
    assert.match(text, /Declarations only · no Run verdicts/);
    assert.equal(await browser.$(".legend").getText(), "Current structure · arrows: input → consumer");
    assert.equal(existsSync(join(fixture.state, "state.sqlite")), false, "no saved Run database was created");
    await browser.$('.svelte-flow__node[data-id="a:release"]').click();
    const evals = await browser.$('[role="listbox"][aria-label="Evals"]');
    await evals.waitForExist({ timeout: 10000 });
    assert.match(await evals.getText(), /A person approves the release/);
    // Opening the selected eval exposes criteria, not a historical request Detail.
    await evals.click();
    await press(browser, ["Enter"], expanded, "structure text expanded from the selected eval");
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
        failSchema: { type: "object", properties: { reason: { type: "string", minLength: 3 } },
          required: ["reason"], additionalProperties: false },
      },
      "ship/check": { target: "ship", title: "Ship the release", gate: null,
        criteria: { kind: "runtime", command: "true", args: [] },
        payload: { instruction: "Ship {release}." }, passSchema: null, failSchema: null },
      "docs/lint": { target: "docs", title: "Docs build", gate: null,
        criteria: { kind: "runtime", command: "true", args: [] },
        payload: { instruction: "Build the docs." }, passSchema: null, failSchema: null },
    }, "every independently declared criterion and owner schema is exposed");
    const relations = shown.split("\n").filter((line) => line.includes(" → "));
    assert.deepEqual(relations, [
      'release → ship {"kind":"instruction","eval_id":"ship/check","name":"release"}',
    ], "the concrete input-to-consumer relation carries its instruction label");
    assert.equal(await browser.$('[aria-label="Human review"]').isExisting(), false);
  });

  // UX edge: E-CURRENT-REFRESH
  it("reloads changed declarations through Refresh, Ctrl+R and F5", async () => {
    const declaration = join(fixture.repo, "docs", "index.artf");
    for (const [index, keys] of [null, ["Control", "r"], ["F5"]].entries()) {
      const title = `Current docs criterion ${index}`;
      writeFileSync(declaration, readFileSync(declaration, "utf8").replace(/title = "[^"]*"/, `title = "${title}"`));
      if (keys) await press(browser, keys, async () => (await structure()).includes(title), `declarations refreshed with ${keys.join("+")}`);
      else {
        await browser.$("button=Refresh").click();
        await browser.waitUntil(async () => (await structure()).includes(title), { timeout: 15000 });
      }
      await browser.$("button=Refresh").waitForEnabled({ timeout: 15000 });
      // Refresh replaces the graph while its previous zoom camera can still be moving.
      // Focus the current node and use the confirmed keyboard action, not coordinates
      // computed before that camera move; await the new eval list before reading it.
      await browser.$('.svelte-flow__node[data-id="a:docs"]').execute((node) => (node as HTMLElement).focus());
      await press(browser, ["Enter"], () => browser.execute((expected) =>
        document.querySelector('[role="listbox"][aria-label="Evals"]')?.textContent?.includes(expected) ?? false,
      title), "the refreshed docs node exposes its current eval title");
      assert.match(await browser.$('[role="listbox"][aria-label="Evals"]').getText(), new RegExp(title));
      assert.match(await browser.execute(() => document.body.innerText), /Declarations only · no Run verdicts/);
    }
  });
});
