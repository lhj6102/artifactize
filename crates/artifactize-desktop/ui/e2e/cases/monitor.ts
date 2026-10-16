// The monitor end to end, in the real app on fixture state made by the real CLI: catalog →
// Run tree → Detail, the shared Human Detail's claim, release, tool, RED validation, Korean
// text and GREEN submission, and a live update from another process.
import assert from "node:assert/strict";
import { artifactize, create, createNavigationFixture, type Fixture, launched, signoff, slowTool } from "../fixture.ts";
import { reviewing } from "../human-review.ts";
import { sessionEndedByApp } from "../observation.ts";
import { afterRender, fill, press } from "../keys.ts";
import { checkpoint, commandsRendered } from "../commands.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { primaryModifier } from "../platform.ts";

async function monitorUI(browser: WebdriverIO.Browser, fixture: Fixture) {
  const text = () => browser.execute(() => document.body.innerText);
  const human = () => browser.$('[aria-label="Human review"][tabindex="0"]');
  const toolOutput = () => browser.execute(() => {
    const detail = document.querySelector('[aria-label="Human review"]');
    const notice = detail?.querySelector(".notice");
    const output = detail?.querySelector('[aria-label="Tool output"]');
    const title = output?.querySelector(".box-title");
    return {
      notice: notice?.textContent?.trim() ?? "",
      noticeError: notice?.classList.contains("error") ?? false,
      title: title?.textContent?.trim() ?? "",
      outputError: title?.classList.contains("error") ?? false,
      text: output?.querySelector("pre")?.textContent ?? "",
    };
  });
  /** Clicking a selected tool runs it. Click only the focusable list container, then
   * select with arrows: neither action runs a tool; only the later Enter does. */
  async function selectTool(name: string) {
    const runsBefore = (await checkpoint(browser)).activity.commands.humanRun?.started ?? 0;
    const tools = () => browser.$('[aria-label="Human tools"]');
    await tools().click();
    await browser.waitUntil(() => tools().isFocused(), { timeout: 15000, timeoutMsg: "the Human tools focused without running a tool" });
    const rows = await browser.execute(() => [...document.querySelectorAll('[aria-label="Human tools"] [role="option"] .name')]
      .map((row) => row.textContent?.trim() ?? ""));
    const target = rows.indexOf(name);
    assert.ok(target >= 0, `this review declares ${name}`);
    const selected = () => browser.execute(() => [...document.querySelectorAll('[aria-label="Human tools"] [role="option"]')]
      .findIndex((row) => row.getAttribute("aria-selected") === "true"));
    while ((await selected()) !== target) {
      const before = await selected();
      const direction = before < target ? 1 : -1;
      await press(browser, [direction > 0 ? "ArrowDown" : "ArrowUp"], async () => (await selected()) === before + direction, "the next tool selected without running it");
    }
    await afterRender(browser);
    assert.equal((await checkpoint(browser)).activity.commands.humanRun?.started ?? 0, runsBefore, "focusing and selecting Tools starts no tool command");
  }

  async function openReview(claim = true) {
    await browser.$('[role="tree"]').click();
    await browser.$('//li[@role="treeitem"][@aria-level="2"][.//span[starts-with(normalize-space(.), "signoff")]]').click();
    await press(browser, ["Enter"], () => human().isExisting(), "this case's Human Detail");
    const id = signoff(fixture).id;
    await browser.waitUntil(async () => (await human().$(".meta").getText()).startsWith(`Request ${id} · WAITING_HUMAN · unclaimed · waiting `), {
      timeout: 15000, timeoutMsg: "the default Human Detail identifies this case's exact waiting request without Technical",
    });
    assert.equal(await human().$(".instruction .text").getText(), "Read the release notes and approve them.", "the selected Human eval's default instruction");
    assert.equal(await human().$("button*=Technical").getAttribute("aria-expanded"), "false", "request and instruction are visible before Technical");
    if (claim) {
      await press(browser, ["c"], async () => signoff(fixture).claim?.reviewer === "alice" && await reviewing(browser), "this case's claim");
      assert.equal((await checkpoint(browser)).activity.commands.humanRun?.started ?? 0, 0, "this fresh session has dispatched no tool");
      const initial = await toolOutput();
      assert.equal(initial.title, "Output", "this case has no prior tool title");
      assert.equal(initial.text, "", "this case has no prior tool output");
      assert.equal(initial.noticeError, false, "this case's claim did not inherit a tool error");
    }
  }
  await browser.waitUntil(async () => (await text()).includes("signoff"), { timeout: 15000, timeoutMsg: "the Run tree" });
  return { text, human, toolOutput, selectTool, openReview };
}

type NavigationData = Omit<Awaited<ReturnType<typeof createNavigationFixture>>, "fixture">;

export const scenarios: Scenario[] = [
  // UX edges: E-LAUNCH-MONITOR
  scenario<null>(
    "monitor-catalog",
    "catalogs the repository and its Run, and peeks at the selected eval",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      const { text } = await monitorUI(browser, fixture);
      const shown = await text();
      assert.equal(await browser.$(".brand").getText(), "Artifactize");
      await browser.waitUntil(() => browser.execute(() => {
        const mark = document.querySelector<HTMLImageElement>(".brand img");
        return !!mark && mark.complete && mark.naturalWidth > 0;
      }), { timeout: 15000, timeoutMsg: "the approved brand image finished loading successfully" });
      assert.equal(await browser.$(".brand img").execute((image) => {
        const mark = image as HTMLImageElement;
        return mark.complete && mark.naturalWidth > 0;
      }), true, "the approved brand mark actually loaded");
      assert.match(shown, /ALL/);
      assert.match(shown, /master · repo/);
      assert.match(shown, /Runs \(1\)/i);
      assert.match(shown, /RUNNING/);
      assert.match(shown, /validation pending/);
      // A new Run starts on its first in-progress eval; the peek follows it.
      const selected = await browser.$('[role="treeitem"][aria-selected="true"]').getText();
      assert.match(selected, /signoff/);
      assert.match(shown, /WAITING_HUMAN — waiting for a Human submission/);
    },
  ),

  // UX edges: E-HUMAN-CLAIMED-ELSEWHERE E-HUMAN-RELEASED-ELSEWHERE E-HUMAN-CLAIM E-HUMAN-RELEASE E-MON-LIVE
  scenario<null>(
    "monitor-external-claim",
    "follows a claim another process makes, then claims and releases explicitly",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      const { text, human, openReview } = await monitorUI(browser, fixture);
      await openReview(false);
      const id = signoff(fixture).id;
      const meta = () => human().$(".meta").getText();
      assert.equal(artifactize(fixture, "request", "claim", id, "--reviewer", "bob"), 0);
      await browser.waitUntil(async () => (await meta()).startsWith(`Request ${id} · WAITING_HUMAN · claimed by bob · read-only · waiting `) &&
        !(await human().$("button*=Claim").isEnabled()), { timeout: 12000, timeoutMsg: "the opened Human Detail displays bob's exact claim and is read-only" });
      assert.equal(artifactize(fixture, "request", "unclaim", id, "--reviewer", "bob"), 0);
      await browser.waitUntil(async () => (await meta()).startsWith(`Request ${id} · WAITING_HUMAN · unclaimed · waiting `) &&
        await human().$("button*=Claim").isEnabled(), { timeout: 12000, timeoutMsg: "bob's release is rendered in the same open Human Detail and Claim is offered again" });

      // A composing keystroke (an input method's) is never a shortcut.
      await human().click();
      const claimBefore = (await checkpoint(browser)).activity.commands.humanClaim;
      const dispatched = claimBefore?.started ?? 0;
      await browser.execute(() => {
        const target = document.querySelector('[aria-label="Human review"][tabindex="0"]');
        target?.dispatchEvent(new KeyboardEvent("keydown", { key: "c", isComposing: true, bubbles: true }));
      });
      await afterRender(browser);
      assert.equal((await checkpoint(browser)).activity.commands.humanClaim?.started ?? 0, dispatched,
        "a composition dispatches no claim, including one whose native completion is still pending");
      assert.equal(signoff(fixture).claim, null, "a composition claims nothing");

      await press(browser, ["c"], async () => (await text()).includes("claimed by you (alice)") && await reviewing(browser), "claimed");
      assert.equal(signoff(fixture).claim?.reviewer, "alice");
      const claimed = (await checkpoint(browser)).activity.commands.humanClaim;
      assert.ok(claimed && claimed.completed > (claimBefore?.completed ?? 0), "the same read-only probe observes this successful claim");
      assert.equal(claimed.pending, 0);
      await press(browser, ["u"], async () => (await text()).includes("Claim released."), "released");
      assert.equal(signoff(fixture).claim, null);
      await press(browser, ["c"], () => signoff(fixture).claim?.reviewer === "alice", "claimed again");
      await browser.waitUntil(() => reviewing(browser), { timeout: 15000, timeoutMsg: "the review stage" });
    },
  ),

  // UX edges: E-HUMAN-TOOL
  scenario<null>(
    "monitor-tool-output",
    "runs an output tool and shows its output",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      const { human, toolOutput, selectTool, openReview } = await monitorUI(browser, fixture);
      await openReview();
      await selectTool("notes_release");
      assert.doesNotMatch((await toolOutput()).text, /Ready to ship\./, "this case has not produced the notes output yet");
      await press(browser, ["Enter"], async () => {
        const shown = await toolOutput();
        return shown.title === "Output · notes_release" && shown.notice === "notes_release finished." && /Ready to ship\./.test(shown.text);
      }, "the selected notes tool's completed output and notice");
      const shown = await toolOutput();
      assert.equal(shown.title, "Output · notes_release");
      assert.match(shown.text, /Ready to ship\./);
      assert.equal(shown.notice, "notes_release finished.");
      assert.equal(shown.outputError, false);
      assert.equal(shown.noticeError, false);
      const saved = signoff(fixture);
      assert.equal(saved.status, "WAITING_HUMAN", "tool output is not a Human verdict");
      assert.equal(saved.claim?.reviewer, "alice");
      assert.ok(await human().isExisting(), "the same Human Detail remains open");
    },
  ),

  scenario<null>(
    "monitor-tool-click",
    "runs a selected tool with a click, without an Enter",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      const { toolOutput, selectTool, openReview } = await monitorUI(browser, fixture);
      await openReview();
      await selectTool("notes_release");
      // What the click reached, for a run that does not start.
      await browser.execute(() => {
        const clicks: string[] = [];
        (window as unknown as { clicks: string[] }).clicks = clicks;
        const describe = (node: Element | null) => node ? `${node.tagName}${node.id ? `#${node.id}` : ""} ${node.textContent?.trim().slice(0, 30) ?? ""}` : "none";
        window.addEventListener("pointerdown", (event) => clicks.push(`pointerdown ${describe(event.target as Element)}`), true);
        window.addEventListener("click", (event) => clicks.push(`click ${describe(event.target as Element)}; focus ${describe(document.activeElement)}`), true);
      });
      await browser.$('//li[@role="option"][.//span[text()="notes_release"]]').click();
      try {
        await browser.waitUntil(async () => {
          const shown = await toolOutput();
          return shown.title === "Output · notes_release" && shown.notice === "notes_release finished." && /Ready to ship\./.test(shown.text);
        }, { timeout: 15000, timeoutMsg: "the selected tool's click alone produced its output" });
      } catch (error) {
        const commands = (await checkpoint(browser)).activity.commands.humanRun;
        const clicks = await browser.execute(() => (window as unknown as { clicks?: string[] }).clicks ?? []);
        throw new Error(`the selected tool's click alone produced its output; tool commands ${JSON.stringify(commands)}, output ${JSON.stringify(await toolOutput())}, events ${JSON.stringify(clicks)}`, { cause: error });
      }
      assert.equal((await checkpoint(browser)).activity.commands.humanRun?.started, 1, "the pointer activation dispatched exactly one tool command");
      assert.equal(signoff(fixture).status, "WAITING_HUMAN", "clicking a tool is not a verdict");
    },
  ),

  // UX edges: E-HUMAN-TOOL-LAUNCH
  scenario<null>(
    "monitor-tool-launch",
    "runs a launch tool and says the application was opened",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      const { text, selectTool, openReview } = await monitorUI(browser, fixture);
      await openReview();
      await selectTool("open_release");
      assert.equal(launched(fixture), false, "selection did not launch this case's application");
      await press(browser, ["Enter"], async () => /open_release (launched|started|opened)/i.test(await text()), "the launch notice");
      // The application stand-in really ran.
      await browser.waitUntil(() => launched(fixture), { timeout: 15000, timeoutMsg: "the launched application" });
    },
  ),

  // UX edges: E-HUMAN-TOOL-FAIL
  scenario<null>(
    "monitor-tool-failure",
    "shows the error of a tool that fails",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      const { human, toolOutput, selectTool, openReview } = await monitorUI(browser, fixture);
      await openReview();
      await selectTool("fail_release");
      const diagnostic = /Human tool exited unsuccessfully \([^)]*\b1\)\./;
      const before = await toolOutput();
      assert.doesNotMatch(before.text, diagnostic, "this case has not produced the failing tool's exit diagnostic yet");
      assert.doesNotMatch(before.notice, /fail_release failed/);
      await press(browser, ["Enter"], async () => {
        const shown = await toolOutput();
        return shown.noticeError && shown.outputError &&
          shown.notice === "fail_release failed; a tool error is not a verdict." &&
          shown.title === "Output · fail_release · tool error" && diagnostic.test(shown.text);
      }, "the selected failing tool's actual exit diagnostic and error notice");
      const shown = await toolOutput();
      assert.equal(shown.notice, "fail_release failed; a tool error is not a verdict.");
      assert.equal(shown.title, "Output · fail_release · tool error");
      assert.match(shown.text, diagnostic);
      assert.equal(shown.noticeError, true);
      assert.equal(shown.outputError, true);
      const saved = signoff(fixture);
      assert.equal(saved.status, "WAITING_HUMAN", "a failed tool is not a Human verdict");
      assert.equal(saved.result, null);
      assert.equal(saved.claim?.reviewer, "alice", "the same Human review remains claimed");
      assert.ok(await human().isExisting(), "the same Human Detail remains open after the tool error");
    },
  ),

  // UX edges: E-HUMAN-ESC-CANCEL
  scenario<null>(
    "monitor-tool-cancel",
    "cancels a running tool with Esc",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      const { text, human, selectTool, openReview } = await monitorUI(browser, fixture);
      await openReview();
      await selectTool("slow_release");
      assert.equal(slowTool(fixture), null, "selection did not start this case's cancellable tool");
      await press(browser, ["Enter"], async () => /Running slow_release/.test(await text()), "the tool running");
      await browser.waitUntil(() => slowTool(fixture) !== null, { timeout: 15000, timeoutMsg: "the owned tool process started" });
      const alive = slowTool(fixture);
      assert.ok(alive && await alive(), "the cancellation target is an actual running process");
      await browser.keys(["Escape"]);
      await browser.waitUntil(async () => !/Running slow_release/.test(await text()), { timeout: 15000, timeoutMsg: "the tool cancelled" });
      await browser.waitUntil(async () => !(await alive()), { timeout: 15000, timeoutMsg: "Esc terminated the owned tool process" });
      assert.ok(await human().isExisting(), "Esc cancelled the job and left the Detail open");
    },
  ),

  // UX edges: E-HUMAN-RED E-HUMAN-INVALID E-HUMAN-SWITCH
  scenario<null>(
    "monitor-invalid-red",
    "keeps a RED result that fails its schema correctable and shows the failing path",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      const { human, openReview } = await monitorUI(browser, fixture);
      await openReview();
      await human().click();
      // Each verdict switch rebuilds the form: the field is found again every time.
      const reason = () => browser.$("#field-reason");
      await press(browser, ["r"], () => reason().isExisting(), "the RED reason form");
      await fill(browser, "#field-reason", "no");
      await browser.keys([primaryModifier, "s"]);
      const alert = await browser.$('[role="alert"]');
      await alert.waitForExist({ timeout: 15000 });
      assert.match(await alert.getText(), /reason/);
      assert.equal(signoff(fixture).status, "WAITING_HUMAN");
      // The form keeps what was entered and stays editable, to be corrected.
      assert.equal(await reason().getValue(), "no");
      await fill(browser, "#field-reason", "not good enough");
      assert.equal(await reason().getValue(), "not good enough");
      assert.equal(signoff(fixture).result, null, "the invalid RED result was not saved");
      // Continue this rejected submission, without leaving its claimed review. Each verdict
      // must rebuild the owner's corresponding schema, even after validation failed.
      await press(browser, [primaryModifier, "g"], async () =>
        await browser.$("#field-comment").isExisting() && !(await reason().isExisting()), "the pass-schema form after invalid RED");
      assert.equal(await browser.$("#field-approved").getValue(), "true (fixed)", "the pass schema's approved constant");
      await fill(browser, "#field-comment", "checked after rejection");
      assert.equal(await browser.$("#field-comment").getValue(), "checked after rejection");
      await press(browser, [primaryModifier, "r"], async () =>
        await reason().isExisting() && !(await browser.$("#field-comment").isExisting()), "the fail-schema form again");
      await fill(browser, "#field-reason", "a correctable reason");
      assert.equal(await reason().getValue(), "a correctable reason");
      assert.equal(await browser.$("#field-approved").isExisting(), false, "no pass-schema field in RED");
      const saved = signoff(fixture);
      assert.equal(saved.status, "WAITING_HUMAN");
      assert.equal(saved.result, null, "switching did not submit the rejected draft");
      assert.equal(saved.claim?.reviewer, "alice", "the same review remains claimed and editable");
    },
  ),

  // UX edges: E-HUMAN-GREEN E-HUMAN-TEXT E-HUMAN-SUBMIT
  scenario<null>(
    "monitor-green-json-korean",
    "submits GREEN with Korean text edited as JSON and keeps the completed result open",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      const { human, openReview } = await monitorUI(browser, fixture);
      await openReview();
      await press(browser, ["g"], () => browser.$("#field-comment").isExisting(), "the GREEN form");
      const comment = await browser.$("#field-comment");
      await comment.waitForExist();
      // An input method's Escape (composing, keyCode 229) belongs to the composition.
      await browser.execute(() => {
        const field = document.querySelector("#field-comment");
        field?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", isComposing: true, bubbles: true }));
        field?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", keyCode: 229, bubbles: true }));
      });
      assert.ok(await comment.isExisting(), "the form stays open");
      const korean = "릴리스 노트를 확인했습니다";
      await fill(browser, "#field-comment", korean);
      assert.equal(await comment.getValue(), korean);
      // Ctrl+E turns the typed form into focused JSON. Edit that text field through
      // WebDriver's text-input API, then Ctrl+S submits it; no click in between.
      await browser.keys([primaryModifier, "e"]);
      await browser.waitUntil(() => browser.execute(() => document.activeElement?.matches("textarea.json") ?? false), { timeout: 15000, timeoutMsg: "focus in the JSON text" });
      const json = await browser.$("textarea.json");
      assert.deepEqual(JSON.parse(await json.getValue()), { approved: true, comment: korean });
      const edited = JSON.stringify({ approved: true, comment: `${korean} (JSON)` }, null, 2);
      await fill(browser, "textarea.json", edited);
      await browser.keys([primaryModifier, "s"]);
      await browser.waitUntil(async () => signoff(fixture).status === "GREEN", { timeout: 15000, timeoutMsg: "GREEN saved" });
      assert.deepEqual(signoff(fixture).result, { verdict: "GREEN", approved: true, comment: `${korean} (JSON)` });
      const run = signoff(fixture).runId;
      await browser.waitUntil(() => browser.execute((id) => {
        const row = [...document.querySelectorAll('.monitor [aria-label="Runs"] [role="option"]')]
          .find((entry) => entry.querySelector(".id")?.textContent?.split(/\s/)[0] === id);
        return row?.querySelector(".status")?.textContent?.trim() === "GREEN";
      }, run), { timeout: 15000, timeoutMsg: "the app has rendered this Run's terminal GREEN update" });
      await commandsRendered(browser);
      assert.ok(await human().isExisting(), "the completed Detail stays open after the terminal subscription update");
      const completed = await browser.execute(() => document.querySelector<HTMLElement>('[aria-label="Human review"]')?.innerText ?? "");
      assert.match(completed, /completed result/i);
      assert.match(completed, /Submitted GREEN for release\/signoff/);
    },
  ),

  // UX edges: E-MON-RESIZE
  scenario<null>(
    "monitor-resize",
    "resizes a split pane by dragging its divider",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      await monitorUI(browser, fixture);
      const width = () =>
        browser.execute(() =>
          (document.querySelector('[role="tree"]')?.closest("[data-pane]") as HTMLElement).getBoundingClientRect().width,
        );
      const before = await width();
      const dividers = await browser.$$("[data-pane-resizer]");
      // Paneforge installs its move listener in a post-press render effect. Keep the real
      // pointer held across that render boundary, rather than guessing how long it takes.
      await browser.action("pointer", { id: "resize" }).move({ origin: dividers[2] }).down().perform(true);
      try {
        await afterRender(browser);
        const pointer = browser.action("pointer", { id: "resize" });
        for (let step = 0; step < 6; step += 1) pointer.move({ origin: "pointer", x: 20, y: 0 });
        await pointer.perform(true);
        await browser.waitUntil(async () => (await width()) > before + 60, { timeout: 15000, timeoutMsg: "the dragged Run tree" });
        await browser.action("pointer", { id: "resize" }).up().perform();
      } finally { await browser.releaseActions(); }
      await browser.waitUntil(async () => (await width()) > before + 60, { timeout: 15000, timeoutMsg: "a wider Run tree" });
    },
  ),

  // UX edges: E-MON-SCOPE-MOVE E-MON-SCOPE-RUNS E-MON-RUNS-MOVE E-MON-RUNS-TREE
  // E-MON-TREE-MOVE E-MON-TREE-FOLD E-MON-TREE-DETAIL E-MON-DETAIL-BACK
  // E-MON-TREE-TO-RUNS E-MON-RUNS-TO-SCOPE
  scenario<NavigationData>(
    "monitor-keyboard-journey",
    "walks Scope → another Run → tree movement and folds → Detail → Tree → Runs → Scope without a mouse",
    async () => {
      const { fixture, ...data } = await createNavigationFixture();
      return { fixture, data };
    },
    async (browser, { data: example }) => {
      const pane = (name: string) => browser.$(`[role="listbox"][aria-label="${name}"]`);
      const selectedRun = () => browser.execute(() => document.querySelector('[aria-label="Runs"] [aria-selected="true"] .id')?.textContent?.split(/\s/)[0] ?? "");
      const selectedTree = () => browser.$('[role="treeitem"][aria-selected="true"]').getAttribute("data-path");
      const scopeIndex = () => browser.execute(() => [...document.querySelectorAll('[aria-label="Scope"] [role="option"]')]
          .findIndex((row) => row.getAttribute("aria-selected") === "true"));
      await browser.waitUntil(() => pane("Runs").isFocused(), { timeout: 15000, timeoutMsg: "the initial Runs pane focused" });
      const [older, newer] = example.runs;
      await press(browser, ["ArrowLeft"], () => pane("Scope").isFocused(), "Scope focused");
      const labels = await browser.execute(() => [...document.querySelectorAll('[aria-label="Scope"] [role="option"] .label')]
        .map((row) => row.textContent?.trim() ?? ""));
      const repo = labels.indexOf("repo");
      assert.ok(repo > 0, "this case's repository follows ALL");
      while ((await scopeIndex()) < repo) {
        const before = await scopeIndex();
        await press(browser, ["ArrowDown"], async () => (await scopeIndex()) === before + 1, "the next scope selected");
      }
      assert.equal(await scopeIndex(), repo);
      const runs = () => browser.execute(() => [...document.querySelectorAll('[aria-label="Runs"] .id')]
        .map((row) => row.textContent?.split(/\s/)[0] ?? "").sort());
      await browser.waitUntil(async () => JSON.stringify(await runs()) === JSON.stringify([...example.scopes.repo].sort()), { timeout: 15000, timeoutMsg: "the selected repository's exact Runs" });
      await press(browser, ["Enter"], () => pane("Runs").isFocused(), "the repository's Runs focused");
      assert.equal(await selectedRun(), newer.id);
      await press(browser, ["ArrowDown"], async () => (await selectedRun()) === older.id, "another identified Run selected");
      await press(browser, ["Enter"], () => browser.$('[role="tree"]').isFocused(), "that Run's Tree focused");
      const evalPath = `a:subject/e:subject/${older.eval}`;
      await browser.waitUntil(async () => (await selectedTree()) === evalPath, { timeout: 15000, timeoutMsg: "the older Run's own criterion selected" });
      await press(browser, ["k"], async () => (await selectedTree()) === "a:subject", "the Artifact above its eval selected");
      const expanded = () => browser.$('[role="treeitem"][data-path="a:subject"]').getAttribute("aria-expanded");
      await press(browser, ["h"], async () => (await expanded()) === "false", "the Artifact folded");
      await press(browser, ["l"], async () => (await expanded()) === "true", "the Artifact unfolded");
      await press(browser, ["j"], async () => (await selectedTree()) === evalPath, "the identified eval selected again");
      const human = () => browser.$('[aria-label="Human review"]');
      await press(browser, ["Enter"], () => human().isExisting(), "the selected eval's Detail opened");
      await browser.waitUntil(() => browser.execute((name) => document.querySelector('[aria-label="Human review"] h3')?.textContent?.includes(`subject/${name}`) ?? false,
        older.eval), { timeout: 15000, timeoutMsg: "Detail has loaded the eval selected in this journey" });
      await press(browser, ["Escape"], async () => !(await human().isExisting()) && await browser.$('[role="tree"]').isFocused(), "back to Tree");
      assert.equal(await selectedTree(), evalPath, "Tree selection survived the Detail");
      await press(browser, ["Escape"], () => pane("Runs").isFocused(), "back to Runs");
      assert.equal(await selectedRun(), older.id, "the same Run remains selected");
      await press(browser, ["ArrowLeft"], () => pane("Scope").isFocused(), "back to Scope");
      assert.equal(await scopeIndex(), repo, "the same repository scope remains selected");
    },
  ),

  // UX edges: E-QUIT-ASK E-QUIT-STAY E-QUIT-RELEASE
  scenario<null>(
    "monitor-quit-claim",
    "asks before quitting with a claim taken here: Esc stays, u releases and quits",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      const text = () => browser.execute(() => document.body.innerText);
      await browser.waitUntil(async () => (await text()).includes("signoff"), { timeout: 15000, timeoutMsg: "the Run tree" });
      // The Runs pane has focus first: Enter steps into the tree, which starts on the sign-off.
      await browser.keys(["Enter"]);
      await browser.waitUntil(() => browser.$('[role="tree"]').isFocused(), { timeout: 15000, timeoutMsg: "tree focus" });
      await browser.keys(["Enter"]);
      await browser.waitUntil(() => browser.$('[aria-label="Human review"]').isExisting(), { timeout: 15000, timeoutMsg: "Human Detail" });
      await browser.waitUntil(async () => (await text()).includes("Unclaimed. c claims it for alice"), { timeout: 15000, timeoutMsg: "the claim stage" });
      await browser.keys(["c"]);
      await browser.waitUntil(async () => signoff(fixture).claim?.reviewer === "alice", { timeout: 15000, timeoutMsg: "claimed" });
      await browser.waitUntil(() => reviewing(browser), { timeout: 15000, timeoutMsg: "the review stage" });
      // Back out of the Detail to the tree, then q outside any field.
      await browser.keys(["Escape"]);
      await browser.$('[aria-label="Human review"]').waitForExist({ reverse: true, timeout: 10000 });
      const dialog = await browser.$('[role="alertdialog"]');
      await browser.keys(["q"]);
      await dialog.waitForExist({ timeout: 10000 });
      await browser.waitUntil(() => browser.execute(() => !!document.activeElement?.closest('[role="alertdialog"]')), { timeout: 15000, timeoutMsg: "focus in the prompt" });
      await browser.keys(["Escape"]);
      await dialog.waitForExist({ reverse: true, timeout: 10000 });
      assert.equal(signoff(fixture).claim?.reviewer, "alice", "staying keeps the claim");

      await browser.keys(["q"]);
      await dialog.waitForExist({ timeout: 10000 });
      await browser.waitUntil(() => browser.execute(() => !!document.activeElement?.closest('[role="alertdialog"]')), { timeout: 15000, timeoutMsg: "focus in the prompt again" });
      // Confirm the window's actual terminal state, never an arbitrary driver exception.
      await sessionEndedByApp(browser, () => browser.keys(["u"]));
      await browser.waitUntil(() => signoff(fixture).claim === null, { timeout: 15000, timeoutMsg: "released on quit" });
      assert.equal(signoff(fixture).status, "WAITING_HUMAN");
    },
  ),
];
