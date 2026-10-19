// Rich saved evidence, not headings alone: long names and words, owner-field findings,
// actual Agent conversation/runtime output, and the different tab and view treatments.
import assert from "node:assert/strict";
import { createRich, FINDING, LONG_NAME, LONG_WORD, MODEL, OWNER } from "../rich-fixture.ts";
import { aside, detail, graphShown, openArtifact, openTab, summary } from "../graph-view.ts";
import { press } from "../keys.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { subdued } from "../rendering.ts";

export const scenarios: Scenario[] = [
  // UX edges: E-MON-DETAIL-ARTIFACT E-MON-DETAIL-TAB E-MON-DETAIL-LOG E-MON-DETAIL-BACK
  scenario<{ runs: string[] }>(
    "graph-rich-detail",
    "contains long names, wrapped eval chips, rich Summary findings and actual saved Agent and runtime logs within its width",
    async () => {
      const { fixture, release, runs } = await createRich();
      return { fixture, release, data: { runs } };
    },
    async (browser, { data: { runs } }) => {
      const selectRun = async (id: string) => {
        await browser.$('[aria-label="Runs"]').waitForExist({ timeout: 15000 });
        const index = await browser.execute((id) => [...document.querySelectorAll<HTMLElement>('[aria-label="Runs"] [role="option"]')]
          .find((row) => row.querySelector(".id")?.textContent?.split(/\s/)[0] === id)?.dataset.index ?? null, id);
        assert.notEqual(index, null, "the requested fixture Run is in the Runs pane");
        await browser.$(`[aria-label="Runs"] [data-index="${index}"]`).click();
        await browser.waitUntil(() => browser.$('section.panel[aria-label="Run ' + id + '"]').isExisting(), { timeout: 15000, timeoutMsg: `Run ${id} selected` });
      };
      await selectRun(runs[1]);
      await graphShown(browser, `a:${LONG_NAME}`);
      const opened = await openArtifact(browser, `a:${LONG_NAME}`, `Artifact ${LONG_NAME}`);
      assert.equal(opened.tabs.length, 17, "the Artifact and every one of its sixteen criteria");
      const chips = () => browser.execute(() => {
        const dock = document.querySelector('aside[aria-label="Detail"]')!;
        const box = dock.getBoundingClientRect();
        const tabs = [...dock.querySelectorAll<HTMLElement>('.strip [role="tab"]')];
        const head = dock.querySelector<HTMLElement>(".strip .head")!;
        const label = head.querySelector<HTMLElement>(".label")!;
        const icon = head.querySelector<HTMLImageElement>("img")!;
        const slash = head.querySelector<HTMLElement>(".slash")!;
        return {
          clipped: label.scrollWidth > label.clientWidth && getComputedStyle(label).textOverflow === "ellipsis",
          icon: icon.getBoundingClientRect().width > 0,
          slash: slash.getBoundingClientRect().width > 0 && slash.getBoundingClientRect().top >= head.getBoundingClientRect().top
            && slash.getBoundingClientRect().bottom <= head.getBoundingClientRect().bottom,
          rows: new Set(tabs.map((tab) => Math.round(tab.getBoundingClientRect().top))).size,
          contained: tabs.every((tab) => { const at = tab.getBoundingClientRect(); return at.left >= box.left && at.right <= box.right; }),
          rounded: tabs.every((tab) => parseFloat(getComputedStyle(tab).borderRadius) > 0),
          fills: tabs.map((tab) => getComputedStyle(tab).backgroundColor),
          selected: tabs.findIndex((tab) => tab.getAttribute("aria-selected") === "true"),
        };
      });
      // Only the chip cuts the long name short: the title shows it whole, within the Detail.
      const heading = await browser.execute(() => {
        const dock = document.querySelector('aside[aria-label="Detail"]')!.getBoundingClientRect();
        const title = document.querySelector<HTMLElement>('aside[aria-label="Detail"] h2')!;
        const box = title.getBoundingClientRect();
        return {
          text: title.innerText,
          clipped: title.scrollWidth > title.clientWidth || title.scrollHeight > title.clientHeight,
          ellipsis: getComputedStyle(title).textOverflow === "ellipsis",
          within: box.left >= dock.left && box.right <= dock.right && box.width > 0 && box.height > 0,
        };
      });
      assert.ok(heading.text.replace(/\s+/g, "").includes(LONG_NAME) && !heading.clipped && !heading.ellipsis && heading.within,
        `the Detail title shows the long name whole: ${JSON.stringify(heading)}`);
      const initial = await chips();
      assert.ok(initial.clipped && initial.icon && initial.slash && initial.rows > 1 && initial.contained && initial.rounded, JSON.stringify(initial));
      assert.notEqual(initial.fills[0], initial.fills[1], "the chosen Artifact chip is filled, unlike the eval chips");
      await openTab(browser, `e:${LONG_NAME}/agent`);
      await browser.waitUntil(async () => (await summary(browser)).status.includes("REUSED"), { timeout: 15000, timeoutMsg: "the reused Agent Summary" });
      const record = () => browser.execute(() => {
        const shown = document.querySelector('aside[aria-label="Detail"] [role="document"]')!;
        const block = (title: string) => [...shown.querySelectorAll(".block")].find((block) => block.querySelector("h3")?.textContent === title)?.textContent ?? "";
        const cost = shown.querySelector<HTMLElement>(".cost")!;
        const status = shown.querySelector<HTMLElement>(".status")!;
        return {
          result: block("Result"), numbered: shown.querySelectorAll(".result ol > li").length,
          options: block("Execution options"), metadata: block("Execution metadata"), fingerprint: block("Fingerprint"),
          cost: cost.textContent ?? "",
          grey: getComputedStyle(cost).color.match(/[\d.]+/g)!.slice(0, 3).map(Number),
          ink: getComputedStyle(shown).color.match(/[\d.]+/g)!.slice(0, 3).map(Number),
          background: getComputedStyle(status.closest("section.panel")!).backgroundColor.match(/[\d.]+/g)!.slice(0, 3).map(Number),
        };
      });
      const saved = await record();
      assert.match(saved.result, new RegExp(OWNER));
      assert.match(saved.result, /owner/);
      assert.match(saved.result, /2 findings/);
      assert.match(saved.result, /R-RECEIPT · receipt.md:12/);
      assert.ok(saved.result.includes(FINDING));
      assert.equal(saved.numbered, 2, "findings render as independently expected numbered items");
      assert.match(saved.options, /openai/);
      assert.match(saved.options, new RegExp(MODEL));
      assert.match(saved.options, /60000/);
      assert.match(saved.metadata, /saved.*48\.2k tokens/);
      assert.match(saved.fingerprint, /artifactsum:/);
      assert.match(saved.cost, /saved.*48\.2k tokens/);
      assert.match(saved.cost, new RegExp(MODEL));
      assert.ok(subdued(saved.grey, saved.ink, saved.background), "saved time/token/model information is subdued neutral grey beside the verdict");
      const containment = () => browser.execute(() => {
        const dock = document.querySelector<HTMLElement>('aside[aria-label="Detail"]')!;
        const box = dock.getBoundingClientRect();
        return { width: Math.round(box.width), fits: dock.scrollWidth <= dock.clientWidth + 1,
          bounded: [...dock.querySelectorAll<HTMLElement>(".instruction, .result, .evidence, .transcript")].every((part) => {
            const at = part.getBoundingClientRect(); return at.left >= box.left && at.right <= box.right + 1 && part.scrollWidth <= part.clientWidth + 1;
          }) };
      });
      const before = await containment();
      assert.ok(before.fits && before.bounded, `long instruction and owner fields stay inside the Detail: ${JSON.stringify(before)}`);
      const choices = () => browser.execute(() => [...document.querySelectorAll<HTMLElement>('aside[aria-label="Detail"] [aria-label="Views"] [role="tab"]')].map((tab) => {
        const style = getComputedStyle(tab); return { selected: tab.getAttribute("aria-selected"), weight: parseInt(style.fontWeight), colour: style.color, fill: style.backgroundColor, radius: parseFloat(style.borderRadius) };
      }));
      const views = await choices();
      assert.ok(views[0].weight > views[1].weight && views[0].colour !== views[1].colour, "the selected Summary choice is darker and bold");
      assert.ok(views.every((view) => view.radius === 0 && (view.fill === "rgba(0, 0, 0, 0)" || view.fill === "transparent")), "view choices are plain text rather than chips");
      await aside(browser).$('[aria-label="Views"]').$('button=Agent log').click();
      await browser.waitUntil(async () => (await aside(browser).getText()).includes("Receipt review complete."), { timeout: 15000, timeoutMsg: "the actual saved Agent answer in its transcript" });
      const transcript = () => browser.execute(() => {
        const pane = document.querySelector<HTMLElement>('aside[aria-label="Detail"] .transcript')!;
        const content = pane.closest<HTMLElement>(".content")!;
        return { top: content.scrollTop, own: !!pane.querySelector(".track .thumb"), text: pane.innerText };
      });
      const bottom = await transcript();
      assert.ok(bottom.own, "the transcript has its own scrolling box");
      await aside(browser).$("button*=Top").click();
      await browser.waitUntil(async () => (await transcript()).text !== bottom.text, { timeout: 15000, timeoutMsg: "the Agent log scrolled independently to its beginning" });
      assert.equal((await transcript()).top, bottom.top, "the surrounding Detail did not scroll");
      assert.equal((await containment()).width, before.width, "long transcript lines do not expand the chosen Detail width");
      assert.ok((await containment()).bounded);
      await aside(browser).$('[aria-label="Views"]').$('button=Summary').click();
      assert.ok((await record()).result.includes(FINDING), "switching back restores the owner findings");
      await openTab(browser, `e:${LONG_NAME}/runtime`);
      await aside(browser).$('[aria-label="Views"]').$('button=Runtime log').click();
      await browser.waitUntil(async () => (await aside(browser).getText()).includes("fixture stdout 079") && (await aside(browser).getText()).includes("fixture stderr 079"),
        { timeout: 15000, timeoutMsg: "actual nonempty saved stdout and stderr" });
      assert.ok((await aside(browser).getText()).includes(LONG_WORD));
      assert.deepEqual(await containment(), before, "saved long output remains within the same Detail");
      const log = await browser.execute(() => {
        const pane = document.querySelector<HTMLElement>('aside[aria-label="Detail"] .evidence')!;
        return { own: getComputedStyle(pane).overflowY, long: pane.scrollHeight > pane.clientHeight };
      });
      assert.ok(["auto", "scroll"].includes(log.own) && log.long, "the runtime log scrolls in its own box");
      // While the old eval's log is still open, a different Artifact replaces it with its
      // own Artifact tab. No stale Summary or log is allowed to survive the switch.
      await graphShown(browser, "a:basis");
      const basis = await openArtifact(browser, "a:basis", "Artifact basis [basis]");
      assert.deepEqual(basis.tabs.map((tab) => tab.name), ["basis"]);
      assert.equal(basis.path, "basis", "an eval-free Artifact has no trailing slash");
      assert.ok(basis.tabs[0].selected && basis.evals.length === 0);
      assert.ok(!(await aside(browser).getText()).includes("fixture stdout"), "the previous Artifact's log was replaced");
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the replacement Detail closed back to the graph");
      // The other saved Run actually executed the Agent, rather than reusing its result.
      await selectRun(runs[0]);
      await graphShown(browser, `a:${LONG_NAME}`);
      await openArtifact(browser, `a:${LONG_NAME}`, `Artifact ${LONG_NAME}`);
      await openTab(browser, `e:${LONG_NAME}/agent`);
      await browser.waitUntil(async () => { const status = (await summary(browser)).status; return status.includes("GREEN") && !status.includes("REUSED"); },
        { timeout: 15000, timeoutMsg: "the executed, non-reused Agent Summary" });
      const executed = await record();
      assert.match(executed.cost, /\b\d+(?:ms|s|m)\b/);
      assert.match(executed.cost, /48\.2k tokens/);
      assert.match(executed.cost, new RegExp(MODEL));
      assert.doesNotMatch(executed.cost, /saved/);
      assert.ok(subdued(executed.grey, executed.ink, executed.background), "actual elapsed time, tokens and model are neutral grey");
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the executed Summary closed");
    },
  ),
];
