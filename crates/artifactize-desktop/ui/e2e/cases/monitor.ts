// The monitor end to end, in the real app on fixture state made by the real CLI: catalog →
// Run graph → an Artifact's Detail → its Human eval's tab, the shared Human Detail's claim,
// release, tools and their live Tool log, RED validation, groups of fields, Korean text and
// GREEN submission, and a live update from another process.
import assert from "node:assert/strict";
import { artifactize, create, createNavigationFixture, finishTalk, type Fixture, launched, signoff, slowTool } from "../fixture.ts";
import { claimable, reviewing } from "../human-review.ts";
import { sessionEndedByApp } from "../observation.ts";
import { afterRender, fill, press } from "../keys.ts";
import { checkpoint, commandsRendered } from "../commands.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { nativeInput, primaryModifier } from "../platform.ts";
import { visibleCenter } from "../pointer.ts";
import { clickOption, dispatchInput, tab } from "../input.ts";
import { emptyRepository } from "../catalog-fixtures.ts";
import { textContrast } from "../rendering.ts";
import { detail, graphShown, openArtifact, openTab, tabAt } from "../graph-view.ts";

/** A Tool log row as read: a run's start (its time as HH:MM:SS), a printed line or a run's end,
 * and whether it is red. */
type LogRow = { text: string; kind: "start" | "line" | "end"; error: boolean };

async function monitorUI(browser: WebdriverIO.Browser, fixture: Fixture, instruction = "Read the release notes and approve them.") {
  const text = () => browser.execute(() => document.body.innerText);
  const human = () => browser.$('[aria-label="Human review"][tabindex="0"]');
  /** The Tool log's rows and the review's notice. */
  const toolLog = () => browser.execute(() => {
    const detail = document.querySelector('[aria-label="Human review"]');
    const notice = detail?.querySelector(".notice");
    const log = detail?.querySelector('[role="log"][aria-label="Tool log"]');
    return {
      notice: notice?.textContent?.trim() ?? "",
      noticeError: notice?.classList.contains("error") ?? false,
      rows: [...(log?.querySelectorAll("p:not(.muted)") ?? [])].map((row) => ({
        text: (row.textContent ?? "").replace(/^▶ \d\d:\d\d:\d\d /, "▶ HH:MM:SS "),
        kind: row.classList.contains("start") ? "start" : row.classList.contains("end") ? "end" : "line",
        error: row.classList.contains("error"),
      })),
    };
  }) as Promise<{ notice: string; noticeError: boolean; rows: LogRow[] }>;
  /** The rows of the last run of release's tool `operation`, from its start line on. */
  const runOf = async (operation: string): Promise<LogRow[] | null> => {
    const { rows } = await toolLog();
    const at = rows.findLastIndex((row) => row.kind === "start" && row.text === `▶ HH:MM:SS · ${operation} · release`);
    if (at < 0) return null;
    const next = rows.findIndex((row, index) => index > at && row.kind === "start");
    return rows.slice(at, next < 0 ? rows.length : next);
  };
  const toolRuns = async () => {
    const probe = (await checkpoint(browser)).activity.commands.humanRun;
    assert.ok(probe, "the wrapped humanRun observation is available before any tool action");
    return probe.started;
  };
  /** Release's tool `operation`, its `▶` button usable after the claim. The returned check
   * proves this same probe observes the run the button starts. */
  async function tool(operation: string) {
    const runsBefore = await toolRuns();
    const button = await human().$(`button=▶ ${operation}`);
    await button.waitForEnabled({ timeout: 15000, timeoutMsg: `release's ${operation} tool usable` });
    assert.equal(await toolRuns(), runsBefore, "finding a tool starts no tool command");
    return { button, observed: async () => assert.ok(await toolRuns() > runsBefore, "the same probe observes the explicitly run tool, not a missing-as-zero counter") };
  }

  async function openReview(claim = true) {
    await openArtifact(browser, "a:release", "Artifact release");
    await openTab(browser, "e:release/signoff");
    await human().waitForExist({ timeout: 15000, timeoutMsg: "this case's Human Detail" });
    const id = signoff(fixture).id;
    await browser.waitUntil(async () => (await human().$(".meta").getText()).startsWith(`Request ${id} · WAITING_HUMAN · unclaimed · waiting `), {
      timeout: 15000, timeoutMsg: "the default Human Detail identifies this case's exact waiting request without Technical",
    });
    assert.equal(await human().$(".instruction .text").getText(), instruction, "the selected Human eval's default instruction");
    assert.equal(await human().$("button*=Technical").getAttribute("aria-expanded"), "false", "request and instruction are visible before Technical");
    // It reads top to bottom as the instruction, the Tools, their Tool log and the Result.
    const order = await browser.execute(() => [...document.querySelectorAll('[aria-label="Human review"] .body > *')]
      .map((part) => part.getAttribute("aria-label") ?? "").filter((label) => label !== ""));
    assert.deepEqual(order, ["Instruction", "Tools", "Tool log", "Result"]);
    if (claim) {
      await press(browser, ["c"], async () => signoff(fixture).claim?.reviewer === "alice" && await reviewing(browser), "this case's claim");
      assert.equal(await toolRuns(), 0, "this fresh session has dispatched no tool");
      const initial = await toolLog();
      assert.deepEqual(initial.rows, [], "this case's Tool log starts empty");
      assert.equal(initial.noticeError, false, "this case's claim did not inherit a tool error");
    }
  }
  await graphShown(browser, "a:release");
  return { text, human, toolLog, runOf, tool, openReview };
}

type NavigationData = Omit<Awaited<ReturnType<typeof createNavigationFixture>>, "fixture">;

export const scenarios: Scenario[] = [
  // UX edges: E-LAUNCH-MONITOR E-MON-RUNS-GRAPH
  scenario<null>(
    "monitor-catalog",
    "catalogs the repository and its Run, and shows the Run's graph with release's waiting sign-off a click away",
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
      // The Run fills the right side as its graph, with every Artifact as a node.
      for (const id of ["a:release", "a:ship", "a:docs"]) await graphShown(browser, id);
      // Enter steps from the Runs into the graph; a click on release shows its waiting sign-off.
      await browser.waitUntil(() => browser.$('[role="listbox"][aria-label="Runs"]').isFocused(), { timeout: 15000, timeoutMsg: "the Runs pane focused" });
      await press(browser, ["Enter"], () => browser.$('section.panel[aria-label^="Run run-"]').isFocused(), "the graph's pane focused");
      const opened = await openArtifact(browser, "a:release", "Artifact release");
      assert.deepEqual(opened.evals.map((row) => row.name), ["signoff"]);
      assert.match(opened.evals[0].status, /waiting for a Human|Human/, "the sign-off waits for a Human");
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
        (await human().getText()).includes("Claimed by bob. Read-only until released.") && !(await human().$("button*=Claim").isExisting()),
      { timeout: 12000, timeoutMsg: "the opened Human Detail displays bob's exact claim and is read-only" });
      assert.equal(artifactize(fixture, "request", "unclaim", id, "--reviewer", "bob"), 0);
      await browser.waitUntil(async () => (await meta()).startsWith(`Request ${id} · WAITING_HUMAN · unclaimed · waiting `) &&
        await human().$("button*=Claim").isEnabled(), { timeout: 12000, timeoutMsg: "bob's release is rendered in the same open Human Detail and Claim is offered again" });

      await human().click();
      const claimBefore = (await checkpoint(browser)).activity.commands.humanClaim;
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
    "runs an output tool with a click and logs its run: its start, each line it printed and its exit",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      const { human, toolLog, runOf, tool, openReview } = await monitorUI(browser, fixture);
      await openReview();
      const notes = await tool("notes");
      await notes.button.click();
      await browser.waitUntil(async () => (await runOf("notes"))?.at(-1)?.kind === "end" && (await toolLog()).notice === "notes_release finished.",
        { timeout: 15000, timeoutMsg: "the notes tool's run ended in the Tool log" });
      await notes.observed();
      assert.deepEqual(await runOf("notes"), [
        { text: "▶ HH:MM:SS · notes · release", kind: "start", error: false },
        { text: "# Release notes", kind: "line", error: false },
        { text: "- Ready to ship.", kind: "line", error: false },
        { text: "■ exit 0", kind: "end", error: false },
      ]);
      assert.equal((await toolLog()).noticeError, false);
      const saved = signoff(fixture);
      assert.equal(saved.status, "WAITING_HUMAN", "tool output is not a Human verdict");
      assert.equal(saved.claim?.reviewer, "alice");
      assert.ok(await human().isExisting(), "the same Human Detail remains open");
    },
  ),

  // UX edges: E-HUMAN-TOOL-LAUNCH
  scenario<null>(
    "monitor-tool-launch",
    "runs a launch tool, says the application was opened, and logs it until it exits",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      const { toolLog, runOf, tool, openReview } = await monitorUI(browser, fixture);
      await openReview();
      const open = await tool("open");
      assert.equal(launched(fixture), false, "finding the tool did not launch this case's application");
      await open.button.click();
      await browser.waitUntil(async () => (await toolLog()).notice === "open_release launched.", { timeout: 15000, timeoutMsg: "the launch notice" });
      // The application stand-in really ran, and its exit ends its run in the log.
      await browser.waitUntil(() => launched(fixture), { timeout: 15000, timeoutMsg: "the launched application" });
      await browser.waitUntil(async () => (await runOf("open"))?.at(-1)?.text === "■ exit 0", { timeout: 15000, timeoutMsg: "the launched application's exit logged" });
      await open.observed();
    },
  ),

  // UX edges: E-HUMAN-TOOL-LAUNCH E-HUMAN-LOG-LIVE E-HUMAN-GREEN E-HUMAN-TEXT
  scenario<null>(
    "monitor-tool-log-live",
    "keeps logging a launched application under its own run after its button returned, its stderr in red, while the reviewer writes the result",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      const { human, toolLog, runOf, tool, openReview } = await monitorUI(browser, fixture);
      await openReview();
      const talk = await tool("talk");
      await talk.button.click();
      // The button returns at once; the application is still running and has said one line.
      await browser.waitUntil(async () => (await toolLog()).notice === "talk_release launched." &&
        (await runOf("talk"))?.some((row) => row.text === "opened the stand-in") === true && await reviewing(browser),
      { timeout: 15000, timeoutMsg: "the launched stand-in's first line, with the review idle" });
      assert.ok(!(await runOf("talk"))!.some((row) => row.kind === "end"), "the launched application has not ended");
      await talk.observed();
      const notes = await tool("notes");
      await notes.button.click();
      await browser.waitUntil(async () => (await runOf("notes"))?.at(-1)?.kind === "end",
        { timeout: 15000, timeoutMsg: "a second tool finishes while talk remains live" });
      const otherRun = await runOf("notes");
      assert.ok(!(await runOf("talk"))!.some((row) => row.kind === "end"));
      // The result can be written meanwhile. The button let go of the keyboard while it ran.
      await human().$('[aria-label="Instruction"]').click();
      await press(browser, ["g"], () => browser.$("#field-comment").isExisting(), "the GREEN form");
      await fill(browser, "#field-comment", "chose B");
      // What the application prints next joins the same run, then its exit ends it.
      finishTalk(fixture);
      await browser.waitUntil(async () => (await runOf("talk"))?.at(-1)?.kind === "end", { timeout: 15000, timeoutMsg: "the stand-in's exit logged" });
      // stdout and stderr are separate pipes: each keeps its own order, but nothing orders
      // one against the other, so the lines are compared per stream.
      const rows = (await runOf("talk"))!;
      assert.deepEqual(rows.at(0), { text: "▶ HH:MM:SS · talk · release", kind: "start", error: false });
      assert.deepEqual(rows.at(-1), { text: "■ exit 0", kind: "end", error: false });
      const lines = rows.slice(1, -1);
      assert.ok(lines.every((row) => row.kind === "line"), "only output lines between the run's start and end");
      assert.deepEqual(lines.filter((row) => !row.error).map((row) => row.text), ["opened the stand-in", "chose: B"]);
      assert.deepEqual(lines.filter((row) => row.error).map((row) => row.text), ["warning: B is slower"]);
      assert.deepEqual(await runOf("notes"), otherRun, "late talk lines and its end do not replace or join the other tool run");
      assert.equal(await browser.$("#field-comment").getValue(), "chose B", "the form kept what was typed while the log grew");
      // The log follows its newest line.
      const followed = await browser.execute(() => {
        const log = document.querySelector('[role="log"][aria-label="Tool log"]')!;
        return log.scrollHeight - log.scrollTop - log.clientHeight < 24;
      });
      assert.ok(followed, "the Tool log shows its newest line");
    },
  ),

  // UX edges: E-HUMAN-TOOL E-HUMAN-LOG-UP E-HUMAN-LOG-FOLLOW E-HUMAN-GREEN E-HUMAN-LOG-COPY E-HUMAN-SUBMIT
  scenario<null>(
    "monitor-tool-log-scroll",
    "scrolls the Tool log in its own box, holds it while scrolled up, follows again from its bottom, gives its text to the result and clears on submission",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      const { human, toolLog, tool, openReview } = await monitorUI(browser, fixture);
      await openReview();
      // A normal smaller window makes the result form scroll the log out of view,
      // as on shorter desktops. Every copy action must bring its own target back.
      await browser.setWindowSize(1100, 720);
      await afterRender(browser);
      const notes = await tool("notes");
      const ends = async () => (await toolLog()).rows.filter((row) => row.kind === "end").length;
      /** One more notes run, until its end line is logged and its button is usable again. */
      const runNotes = async () => {
        const before = await ends();
        await notes.button.waitForEnabled({ timeout: 15000, timeoutMsg: "the notes tool usable" });
        // Preparing a user's click may scroll the review to the button. Observe after that
        // input preparation, so the assertion isolates scrolling caused by arriving output.
        await notes.button.scrollIntoView({ block: "nearest" });
        await afterRender(browser);
        const position = await box();
        const point = await visibleCenter(browser, await notes.button.getElement());
        await browser.action("pointer").move(point).down().up().perform();
        await browser.waitUntil(async () => (await ends()) === before + 1 && await notes.button.isEnabled(),
          { timeout: 15000, timeoutMsg: "another notes run logged" });
        return position;
      };
      const LOG = '[role="log"][aria-label="Tool log"]';
      /** Where the log is scrolled, whether it is longer than its box, and where the rest of
       * the review is scrolled. */
      const box = () => browser.execute((selector) => {
        const log = document.querySelector<HTMLElement>(selector)!;
        return {
          top: log.scrollTop,
          below: log.scrollHeight - log.scrollTop - log.clientHeight,
          overflows: log.scrollHeight > log.clientHeight + 24,
          review: log.closest<HTMLElement>(".body")!.scrollTop,
        };
      }, LOG);
      // Runs until the log is longer than its box: it shows its newest line.
      for (let run = 0; run < 8 && !(await box()).overflows; run += 1) await runNotes();
      assert.ok((await box()).overflows, "the Tool log is longer than its box");
      assert.ok((await box()).below < 24, "the Tool log follows its newest line");
      // Scrolled up to its first line, it stays there while the next run's lines arrive below,
      // and the rest of the review stays where it was.
      await browser.$(`${LOG} p.start`).scrollIntoView({ block: "nearest" });
      // Its first line sits at the top of the box, below the box's own padding.
      await browser.waitUntil(async () => { const at = await box(); return at.top < 24 && at.below >= 24; },
        { timeout: 15000, timeoutMsg: "the Tool log scrolled up" });
      const up = await runNotes();
      const held = await box();
      assert.equal(held.top, up.top, "new lines do not move a log scrolled up");
      assert.ok(held.below > up.below, "the new lines arrived below");
      assert.equal(held.review, up.review, "the rest of the review did not scroll");
      // Back at its bottom, it follows each newest line again.
      await browser.$(`${LOG} p.end:last-child`).scrollIntoView({ block: "nearest" });
      await browser.waitUntil(async () => (await box()).below < 24, { timeout: 15000, timeoutMsg: "the Tool log at its bottom" });
      await runNotes();
      assert.ok((await box()).below < 24, "the Tool log follows its newest line again");
      await notes.observed();
      // Its text is ordinary selectable page text. Select it through input, then copy
      // and paste through the engine's own editing commands.
      // The tool buttons let go of the keyboard while they ran: the instruction takes it.
      await human().$('[aria-label="Instruction"]').click();
      await press(browser, ["g"], () => browser.$("#field-comment").isExisting(), "the GREEN form");
      // Select All is real WebDriver keyboard input. It includes the log along with the
      // surrounding review; the selection is only observed, never installed by page script.
      await browser.$(LOG).scrollIntoView({ block: "nearest" });
      await afterRender(browser);
      const logPoint = await visibleCenter(browser, await browser.$(LOG).getElement());
      await dispatchInput(browser, { kind: "text-click", ...logPoint });
      await browser.keys([primaryModifier, "a"]);
      const copied = await browser.execute(() => getSelection()?.toString() ?? "");
      assert.ok(copied.includes("- Ready to ship."), "the selected text includes the selectable Tool log");
      // The pristine provider has no clipboard shortcuts. The evaluated device uses only
      // the WebView's editing API; it copies our selection and pastes into the focused field.
      await nativeInput(browser, { type: "copy" });
      await browser.$("#field-comment").click();
      await nativeInput(browser, { type: "paste" });
      await browser.waitUntil(async () => (await browser.$("#field-comment").getValue()) === copied,
        { timeout: 15000, timeoutMsg: "the copied Tool log text pasted unchanged into the result" });
      // A submission clears the Tool log; the completed result stays open.
      await browser.keys([primaryModifier, "s"]);
      await browser.waitUntil(() => signoff(fixture).status === "GREEN", { timeout: 15000, timeoutMsg: "GREEN saved" });
      assert.deepEqual(signoff(fixture).result, { verdict: "GREEN", approved: true, comment: copied });
      await browser.waitUntil(async () => (await toolLog()).rows.length === 0, { timeout: 15000, timeoutMsg: "the Tool log cleared by the submission" });
      await browser.waitUntil(async () => /completed result/i.test(await human().getText()), { timeout: 15000, timeoutMsg: "the completed result open" });
    },
  ),

  // UX edges: E-HUMAN-TOOL-FAIL
  scenario<null>(
    "monitor-tool-failure",
    "says a tool failed and ends its run red in the Tool log",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      const { human, toolLog, runOf, tool, openReview } = await monitorUI(browser, fixture);
      await openReview();
      const fail = await tool("fail");
      await fail.button.click();
      await browser.waitUntil(async () => (await runOf("fail"))?.at(-1)?.kind === "end" && (await toolLog()).noticeError,
        { timeout: 15000, timeoutMsg: "the failing tool's end and error notice" });
      await fail.observed();
      assert.deepEqual(await runOf("fail"), [
        { text: "▶ HH:MM:SS · fail · release", kind: "start", error: false },
        { text: "■ exit 1", kind: "end", error: true },
      ]);
      assert.equal((await toolLog()).notice, "fail_release failed; a tool error is not a verdict.");
      const saved = signoff(fixture);
      assert.equal(saved.status, "WAITING_HUMAN", "a failed tool is not a Human verdict");
      assert.equal(saved.result, null);
      assert.equal(saved.claim?.reviewer, "alice", "the same Human review remains claimed");
      assert.ok(await human().isExisting(), "the same Human Detail remains open after the tool error");
    },
  ),

  // UX edges: E-HUMAN-GREEN E-HUMAN-TOOL E-HUMAN-ESC-CANCEL E-HUMAN-ESC-FORM E-HUMAN-ESC-DETAIL E-MON-DETAIL-BACK
  scenario<null>(
    "monitor-tool-cancel",
    "steps back one thing per Esc from the Detail's tabs: cancels a running tool, then leaves the form, then closes the Detail",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      const { text, human, runOf, tool, openReview } = await monitorUI(browser, fixture);
      await openReview();
      await press(browser, ["g"], () => browser.$("#field-comment").isExisting(), "the GREEN form");
      await fill(browser, "#field-comment", "kept while stepping back");
      const slow = await tool("slow");
      assert.equal(slowTool(fixture), null, "finding the tool did not start this case's cancellable tool");
      await slow.button.click();
      await browser.waitUntil(async () => /Running slow/.test(await text()), { timeout: 15000, timeoutMsg: "the tool running" });
      await browser.waitUntil(() => slowTool(fixture) !== null, { timeout: 15000, timeoutMsg: "the owned tool process started" });
      const alive = slowTool(fixture);
      assert.ok(alive && await alive(), "the cancellation target is an actual running process");
      // The keyboard goes to the eval's own chip, outside the review: a click on the shown tab
      // keeps it shown.
      const chip = browser.$(tabAt("e:release/signoff"));
      await chip.click();
      await browser.waitUntil(() => chip.isFocused(), { timeout: 15000, timeoutMsg: "the eval's chip focused" });
      await press(browser, ["Escape"], async () => !/Running slow/.test(await text()), "the tool cancelled by Esc on the tabs");
      await browser.waitUntil(async () => !(await alive()), { timeout: 15000, timeoutMsg: "Esc terminated the owned tool process" });
      await browser.waitUntil(async () => (await runOf("slow"))?.at(-1)?.text === "■ cancelled", { timeout: 15000, timeoutMsg: "the run ended as cancelled" });
      await slow.observed();
      assert.ok(await human().isExisting(), "Esc cancelled the job and left the Detail open");
      assert.equal(await browser.$("#field-comment").getValue(), "kept while stepping back", "the form stays open while the job is cancelled");
      await press(browser, ["Escape"], async () => !(await browser.$("#field-comment").isExisting()), "the form left by the next Esc");
      assert.ok(await human().isExisting(), "leaving the form left the Detail open");
      assert.equal(signoff(fixture).claim?.reviewer, "alice", "the review stays claimed");
      await press(browser, ["Escape"], async () => (await detail(browser)) === null, "the Detail closed by the third Esc");
      // The draft was kept: the same verdict reopens it.
      await openArtifact(browser, "a:release", "Artifact release");
      await openTab(browser, "e:release/signoff");
      await human().waitForExist({ timeout: 15000, timeoutMsg: "the Human Detail again" });
      await human().click();
      await press(browser, ["g"], async () => (await browser.$("#field-comment").isExisting()) && (await browser.$("#field-comment").getValue()) === "kept while stepping back", "the kept GREEN draft");
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
      assert.equal(await browser.$("#field-approved").isExisting(), false, "the pass schema's approved constant is filled in, not shown");
      await fill(browser, "#field-comment", "checked after rejection");
      assert.equal(await browser.$("#field-comment").getValue(), "checked after rejection");
      await press(browser, [primaryModifier, "r"], async () =>
        await reason().isExisting() && !(await browser.$("#field-comment").isExisting()), "the fail-schema form again");
      await fill(browser, "#field-reason", "a correctable reason");
      assert.equal(await reason().getValue(), "a correctable reason");
      const saved = signoff(fixture);
      assert.equal(saved.status, "WAITING_HUMAN");
      assert.equal(saved.result, null, "switching did not submit the rejected draft");
      assert.equal(saved.claim?.reviewer, "alice", "the same review remains claimed and editable");
    },
  ),

  // UX edges: E-HUMAN-RED E-HUMAN-LIST-ADD E-HUMAN-LIST-REMOVE E-HUMAN-GROUP-ADD E-HUMAN-GROUP-REMOVE E-HUMAN-SUBMIT
  scenario<null>(
    "monitor-red-groups",
    "enters a list of steps one text at a time and a list of findings as groups of fields, each added and removed, and submits them",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      const { human, openReview } = await monitorUI(browser, fixture);
      await openReview();
      await press(browser, ["r"], () => browser.$("#field-reason").isExisting(), "the RED form");
      await fill(browser, "#field-reason", "two problems found");
      // A list of text: one empty item to type in, another added, the first removed.
      const step = (at: number) => `#field-steps textarea[aria-label="steps ${at}"]`;
      assert.ok(await browser.$(step(1)).isExisting(), "the first step to type in");
      await fill(browser, step(1), "check the date");
      await human().$("#field-steps + button").click();
      await browser.$(step(2)).waitForExist({ timeout: 15000, timeoutMsg: "a second step added" });
      assert.equal(await browser.$(step(2)).getValue(), "", "the added step is empty");
      await fill(browser, step(2), "check the title");
      await human().$('button[aria-label="Remove steps 1"]').click();
      await browser.$(step(2)).waitForExist({ reverse: true, timeout: 15000, timeoutMsg: "the first step removed" });
      assert.equal(await browser.$(step(1)).getValue(), "check the title", "the second step moved up");
      await human().$('button[aria-label="Remove steps 1"]').click();
      await browser.waitUntil(async () => (await browser.$(step(1)).getValue()) === "" && !(await browser.$(step(2)).isExisting()),
        { timeout: 15000, timeoutMsg: "removing the final step leaves exactly one empty editable item" });
      await fill(browser, step(1), "check the title");
      const group = (at: number) => human().$(`[role="group"][aria-label="findings ${at}"]`);
      // One empty group is there to type in.
      assert.ok(await group(1).isExisting(), "the first findings group");
      await fill(browser, "#field-findings-0-problem", "typo");
      await human().$("#field-findings + button").click();
      await group(2).waitForExist({ timeout: 15000, timeoutMsg: "a second findings group added" });
      assert.equal(await browser.$("#field-findings-1-problem").getValue(), "", "the added group's text starts empty");
      assert.equal(await browser.$("#field-findings-1-severity").getValue(), "", "the added group's choice starts empty");
      assert.equal(await browser.$("#field-findings-0-problem").getValue(), "typo", "adding a group preserves the original group's text");
      assert.equal(await browser.$("#field-findings-0-severity").getValue(), "", "adding a group preserves the original group's unset choice");
      await fill(browser, "#field-findings-1-problem", "date");
      // An optional choice left empty stays out of the result.
      assert.equal(await browser.$("#field-findings-1-severity").getValue(), "", "the severity left empty");
      await human().$('button[aria-label="Remove findings 1"]').click();
      await group(2).waitForExist({ reverse: true, timeout: 15000, timeoutMsg: "the first group removed" });
      assert.equal(await browser.$("#field-findings-0-problem").getValue(), "date", "the second group moved up");
      await human().$('button[aria-label="Remove findings 1"]').click();
      await browser.waitUntil(async () => (await browser.$("#field-findings-0-problem").getValue()) === "" && !(await group(2).isExisting()),
        { timeout: 15000, timeoutMsg: "removing the final group leaves one empty group with its schema fields" });
      assert.ok(await browser.$("#field-findings-0-severity").isExisting());
      await fill(browser, "#field-findings-0-problem", "date");
      await browser.keys([primaryModifier, "s"]);
      await browser.waitUntil(() => signoff(fixture).status === "RED", { timeout: 15000, timeoutMsg: "RED saved" });
      assert.deepEqual(signoff(fixture).result, { verdict: "RED", reason: "two problems found", findings: [{ problem: "date" }], steps: ["check the title"] });
    },
  ),

  // UX journey: J-HUMAN-FORM
  scenario<null>(
    "monitor-form-scalars",
    "edits a number, a boolean and a choice, marks required fields and shows Optional hints before submitting typed values",
    async () => ({ fixture: await create({ title: "Scalar form", instruction: "Read the release notes and approve them.", scalars: true }), data: null }),
    async (browser, { fixture }) => {
      const { human, openReview } = await monitorUI(browser, fixture);
      await openReview();
      await press(browser, ["g"], () => browser.$("#field-comment").isExisting(), "the scalar GREEN form");
      assert.ok(await human().$('label[for="field-comment"] [aria-label="required"]').isExisting(), "the required comment has its star");
      assert.equal(await browser.$("#field-note").getAttribute("placeholder"), "Optional");
      await browser.$("#field-note").scrollIntoView({ block: "nearest" });
      const placeholder = await textContrast(browser, "#field-note");
      await fill(browser, "#field-note", "Optional");
      await tab(browser);
      const entered = await textContrast(browser, "#field-note");
      assert.ok(placeholder > entered * 0.1 && placeholder < entered * 0.9,
        `the painted Optional hint is visible and greyer than the same glyphs entered normally: ${placeholder}/${entered}`);
      await fill(browser, "#field-note", "");
      await fill(browser, "#field-comment", "checked typed fields");
      await fill(browser, "#field-score", "7.5");
      await human().$('input[name="field-confirmed"]').click();
      await clickOption(browser, await browser.$('#field-channel option[value="1"]').getElement());
      await browser.waitUntil(async () => (await browser.$("#field-channel").getValue()) === "1", { timeout: 15000, timeoutMsg: "the preview choice selected" });
      await browser.keys([primaryModifier, "s"]);
      await browser.waitUntil(() => signoff(fixture).status === "GREEN", { timeout: 15000, timeoutMsg: "the scalar result saved" });
      assert.deepEqual(signoff(fixture).result, { verdict: "GREEN", approved: true, comment: "checked typed fields", score: 7.5, confirmed: true, channel: "preview" });
      await browser.waitUntil(async () => {
        const text = await human().getText();
        return /completed result/i.test(text) && text.includes("7.5") && text.includes("preview");
      }, { timeout: 15000, timeoutMsg: "the completed scalar result rendered with its submitted number and choice" });
    },
  ),

  // UX journey: J-HUMAN-TOOL
  scenario<null>(
    "monitor-tool-groups",
    "groups tools by their declaring Artifact, own first, and runs the tool of each group",
    async () => ({ fixture: await create({ title: "Grouped tools", instruction: "Read {docs} and approve." }), data: null }),
    async (browser, { fixture }) => {
      const { human, openReview, runOf } = await monitorUI(browser, fixture, "Read {docs} and approve.");
      await openReview();
      const groups = await browser.execute(() => [...document.querySelectorAll('[aria-label="Tools"] .group')].map((group) => group.getAttribute("aria-label")));
      assert.deepEqual(groups, ["Tools of release", "Tools of docs"], "own tools precede the alphabetically earlier dependency");
      await human().$('[aria-label="Tools of release"]').$('button=▶ notes').click();
      await browser.waitUntil(async () => (await runOf("notes"))?.at(-1)?.kind === "end", { timeout: 15000, timeoutMsg: "release's notes completed" });
      await human().$('[aria-label="Tools of docs"]').$('button=▶ reference').click();
      await browser.waitUntil(async () => (await human().getText()).includes("Documentation checked."), { timeout: 15000, timeoutMsg: "the dependency's own notes were printed" });
      const text = await human().getText();
      assert.match(text, /reference · docs/);
      assert.match(text, /notes · release/);
      assert.match(text, /Ready to ship/);
    },
  ),

  // UX journey: J-HUMAN-LOG
  scenario<null>(
    "monitor-tool-log-long-lines",
    "wraps long stdout and stderr in a monospace Tool log and paints stderr and a failing end red",
    async () => ({ fixture: await create({ title: "Long Tool log", instruction: "Read the release notes and approve them.", report: true }), data: null }),
    async (browser, { fixture }) => {
      const { human, openReview, tool, runOf } = await monitorUI(browser, fixture);
      await openReview();
      const report = await tool("report");
      await report.button.click();
      await browser.waitUntil(async () => (await runOf("report"))?.at(-1)?.kind === "end", { timeout: 15000, timeoutMsg: "the long report finished" });
      const appearance = await browser.execute(() => {
        const log = document.querySelector<HTMLElement>('[role="log"][aria-label="Tool log"]')!;
        const rows = [...log.querySelectorAll<HTMLElement>("p")];
        const out = rows.find((row) => row.textContent?.startsWith("fixture stdout 000"))!;
        const err = rows.find((row) => row.textContent?.startsWith("fixture stderr 000"))!;
        const colour = getComputedStyle(err).color.match(/[\d.]+/g)!.map(Number);
        return { font: getComputedStyle(log).fontFamily, wraps: out.getBoundingClientRect().height > 18 && err.getBoundingClientRect().height > 18,
          fits: log.scrollWidth <= log.clientWidth + 1, red: colour[0] > colour[1] && colour[0] > colour[2],
          distinct: getComputedStyle(out).color !== getComputedStyle(err).color, colour: getComputedStyle(err).color };
      });
      assert.match(appearance.font, /mono/i);
      assert.ok(appearance.wraps && appearance.fits && appearance.red && appearance.distinct, JSON.stringify(appearance));
      const fail = await tool("fail");
      await fail.button.click();
      await browser.waitUntil(async () => (await runOf("fail"))?.at(-1)?.kind === "end", { timeout: 15000, timeoutMsg: "the failing tool's end logged" });
      const ending = await human().$('[role="log"] p.end.error:last-child').getCSSProperty("color");
      assert.equal(String(ending.value).replace(/\s/g, ""), appearance.colour.replace(/\s/g, ""), "the failed end is painted the same red as stderr");
    },
  ),

  // UX edges: E-HUMAN-GREEN E-HUMAN-TEXT E-HUMAN-SUBMIT
  scenario<null>(
    "monitor-green-korean",
    "submits GREEN with Korean text typed into its form and keeps the completed result open",
    async () => ({ fixture: await create(), data: null }),
    async (browser, { fixture }) => {
      const { human, openReview } = await monitorUI(browser, fixture);
      await openReview();
      await press(browser, ["g"], () => browser.$("#field-comment").isExisting(), "the GREEN form");
      const comment = await browser.$("#field-comment");
      await comment.waitForExist();
      const korean = "릴리스 노트를 확인했습니다";
      await fill(browser, "#field-comment", korean);
      assert.equal(await comment.getValue(), korean);
      // The form is the only way in: there is no JSON editor to switch to.
      assert.equal(await human().$("textarea.json").isExisting(), false);
      assert.equal(await human().$("button*=JSON").isExisting(), false);
      await browser.keys([primaryModifier, "s"]);
      await browser.waitUntil(async () => signoff(fixture).status === "GREEN", { timeout: 15000, timeoutMsg: "GREEN saved" });
      assert.deepEqual(signoff(fixture).result, { verdict: "GREEN", approved: true, comment: korean });
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

  // UX edges: E-MON-RESIZE E-MON-DETAIL-ARTIFACT E-MON-DETAIL-WIDTH E-MON-DETAIL-BACK
  scenario<{ width: number }>(
    "monitor-resize",
    "resizes the graph against the left side, Scope against Runs, and the Detail, by dragging their dividers and its edge",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", emptyRepository(fixture)], data: { width: 400 } };
    },
    async (browser, { fixture, data: { width } }) => {
      await browser.$('[aria-label="Scope"] [data-index="0"]').waitForExist({ timeout: 15000 });
      await browser.$('[aria-label="Scope"] [data-index="0"]').click();
      await monitorUI(browser, fixture);
      const size = (selector: string, axis: "width" | "height") =>
        browser.execute((within, dimension) =>
          (document.querySelector(within)?.closest("[data-pane]") as HTMLElement).getBoundingClientRect()[dimension as "width" | "height"],
        selector, axis);
      // Paneforge installs its move listener in a post-press render effect. Keep the real
      // pointer held across that render boundary, rather than guessing how long it takes.
      const dividers = () => browser.$$(".monitor [data-pane-resizer]");
      // What the page received, read only when a drag did not resize: the mouse events that
      // reached it, where they landed, and the divider's and panes' geometry.
      const watchMouse = () => browser.execute(() => {
        const log: string[] = [];
        (window as unknown as { resizeLog: string[] }).resizeLog = log;
        for (const type of ["pointerdown", "mousedown", "mousemove", "mouseup", "mouseleave"]) {
          document.addEventListener(type, (event) => {
            const mouse = event as MouseEvent;
            const target = event.target as Element | null;
            log.push(`${type}@${mouse.clientX},${mouse.clientY}:${target?.tagName ?? "?"}.${target?.getAttribute("class") ?? ""}${target?.hasAttribute("data-pane-resizer") ? "[resizer]" : ""}`);
          }, { capture: true });
        }
      });
      const layout = (at: number) => browser.execute((index) => {
        const resizer = document.querySelectorAll(".monitor [data-pane-resizer]")[index] as HTMLElement | undefined;
        const box = resizer?.getBoundingClientRect();
        return {
          window: [innerWidth, innerHeight, devicePixelRatio],
          resizer: box && [box.left, box.top, box.width, box.height],
          state: resizer && [...resizer.attributes].map((attribute) => `${attribute.name}=${attribute.value}`),
          panes: [...document.querySelectorAll<HTMLElement>(".monitor [data-pane]")].map((pane) => Math.round(pane.getBoundingClientRect().width)),
          events: (window as unknown as { resizeLog?: string[] }).resizeLog?.slice(0, 20),
        };
      }, at);
      const drag = async (at: number, x: number, y: number, grown: () => Promise<boolean>, what: string) => {
        await watchMouse();
        // Press 2px past the line, over the pane after it: the grab zone reaches over both
        // neighbours, and that pane paints its own layers (the graph's canvas) there.
        await browser.action("pointer", { id: "resize" })
          .move({ origin: (await dividers())[at], x: x === 0 ? 0 : 2, y: y === 0 ? 0 : 2 }).down().perform(true);
        try {
          await afterRender(browser);
          const pointer = browser.action("pointer", { id: "resize" });
          for (let step = 0; step < 6; step += 1) pointer.move({ origin: "pointer", x, y });
          await pointer.perform(true);
          try {
            await browser.waitUntil(grown, { timeout: 15000, timeoutMsg: `the dragged ${what}` });
          } catch (error) {
            throw new Error(`the dragged ${what}: ${JSON.stringify(await layout(at))}`, { cause: error });
          }
          await browser.action("pointer", { id: "resize" }).up().perform();
        } finally { await browser.releaseActions(); }
        await browser.waitUntil(grown, { timeout: 15000, timeoutMsg: `a larger ${what}` });
      };
      const graph = () => size(".monitor .stage", "width");
      const scope = () => size('[role="listbox"][aria-label="Scope"]', "height");
      const graphBefore = await graph();
      const scopeBefore = await scope();
      assert.equal((await dividers()).length, 2, "one divider between Scope and Runs, one between the left side and the graph");
      // In document order: Scope | Runs inside the left side, then the left side | the graph.
      await drag(1, -20, 0, async () => (await graph()) > graphBefore + 60, "Run graph");
      await drag(0, 0, 20, async () => (await scope()) > scopeBefore + 60, "Scope pane");

      // The Detail's left edge: a drag widens it within the graph's area, and it opens again
      // at that width.
      await openArtifact(browser, "a:docs", "Artifact docs");
      const placed = () => browser.execute(() => {
        const dock = document.querySelector('.stage aside[aria-label="Detail"]')?.getBoundingClientRect();
        const stage = document.querySelector(".stage")!.getBoundingClientRect();
        return dock ? { width: Math.round(dock.width), left: Math.round(dock.left - stage.left), right: Math.round(stage.right - dock.right) } : null;
      });
      const before = (await placed())!;
      const edge = await browser.$('aside[aria-label="Detail"] [role="separator"][aria-label="Detail width"]');
      await browser.action("pointer", { id: "edge" }).move({ origin: edge, x: 0, y: 0 }).down().perform(true);
      try {
        const pointer = browser.action("pointer", { id: "edge" });
        for (let step = 0; step < 6; step += 1) pointer.move({ origin: "pointer", x: -20, y: 0 });
        await pointer.up().perform();
      } finally { await browser.releaseActions(); }
      let widened: Awaited<ReturnType<typeof placed>> = null;
      await browser.waitUntil(async () => {
        widened = await placed();
        return !!widened && widened.width > before.width + 60;
      }, { timeout: 15000, timeoutMsg: "the dragged Detail widened" })
        .catch((error: unknown) => { throw new Error(`${String(error)}: ${JSON.stringify({ before, widened })}`); });
      assert.ok(widened!.left > 0 && widened!.right > 0, `the Detail stays within the graph's area: ${JSON.stringify(widened)}`);
      await press(browser, ["Escape"], async () => (await placed()) === null, "the Detail closed");
      await openArtifact(browser, "a:docs", "Artifact docs");
      assert.equal((await placed())?.width, widened!.width, "the Detail opens again at its dragged width");
      const dragEdge = async (x: number) => {
        const handle = await browser.$('aside[aria-label="Detail"] [aria-label="Detail width"]');
        await browser.action("pointer", { id: "bounds" }).move({ origin: handle }).down().perform(true);
        try {
          await browser.action("pointer", { id: "bounds" }).move({ x, y: 200 }).up().perform();
        } finally { await browser.releaseActions(); }
      };
      const room = await browser.execute(() => {
        const box = document.querySelector(".stage")!.getBoundingClientRect();
        return { left: box.left, right: box.right, width: box.width };
      });
      await dragEdge(Math.round(room.right - 1));
      const minimum = (await placed())!;
      const readable = await browser.execute(() => {
        const dock = document.querySelector<HTMLElement>('aside[aria-label="Detail"]')!;
        const heading = dock.querySelector<HTMLElement>("h2")!;
        const tab = dock.querySelector<HTMLElement>('[role="tab"] .label')!;
        return dock.scrollWidth <= dock.clientWidth && heading.scrollWidth <= heading.clientWidth && tab.scrollWidth <= tab.clientWidth;
      });
      assert.ok(readable && minimum.width <= before.width && minimum.left > 0 && minimum.right > 0, "the minimum is a readable, contained column, not collapsed text");
      await dragEdge(Math.round(room.right - 1));
      assert.equal((await placed())?.width, minimum.width, "dragging past the minimum cannot shrink the column further");
      await dragEdge(Math.round(room.left + 1));
      const maximum = (await placed())!;
      assert.ok(maximum.width > minimum.width && maximum.width < room.width && maximum.left > 0 && maximum.right > 0, "the widest Detail stays in its graph area");
      const graphVisible = await browser.execute(() => {
        const stage = document.querySelector(".stage")!.getBoundingClientRect();
        const dock = document.querySelector('aside[aria-label="Detail"]')!.getBoundingClientRect();
        return !!document.elementFromPoint((stage.left + dock.left) / 2, stage.top + stage.height / 2)?.closest(".graph");
      });
      assert.ok(graphVisible, "some actual graph remains visible beside the maximum-width Detail");
      await dragEdge(Math.round(room.right - maximum.right - width));
      await browser.waitUntil(async () => Math.abs((await placed())!.width - width) <= 1, { timeout: 15000, timeoutMsg: "the independently chosen persisted width" });
      const cursor = await browser.execute(() => ({
        edge: getComputedStyle(document.querySelector('aside[aria-label="Detail"] [aria-label="Detail width"]')!).cursor,
        divider: getComputedStyle(document.querySelectorAll(".monitor [data-pane-resizer]")[1]).cursor,
      }));
      assert.match(cursor.edge, /(?:ew|col)-resize/);
      assert.match(cursor.divider, /(?:ew|col)-resize/);
      const edgePoint = await visibleCenter(browser, await browser.$('aside[aria-label="Detail"] [aria-label="Detail width"]').getElement());
      await browser.action("pointer").move(edgePoint).perform();
      assert.equal(await browser.$('[role="tooltip"], [role="menu"]').isExisting(), false, "hovering the edge opens nothing");
      await press(browser, ["Escape"], async () => (await placed()) === null, "the Detail closes before removing the graph");
      const split = await graph();
      // No graph in another repository: the same dragged split remains, then restores its Run.
      const scopes = await browser.$$('[aria-label="Scope"] [role="option"]').getElements();
      const empty = [];
      for (const row of scopes) if ((await row.getText()).includes("empty-repo")) empty.push(row);
      assert.equal(empty.length, 1, "the launch repository has its own empty scope");
      await empty[0].click();
      await browser.waitUntil(async () => (await browser.$(".stage").getText()).includes("Select a Run."), { timeout: 15000, timeoutMsg: "the empty scope removes the graph, not the pane" });
      assert.equal(await graph(), split, "the split stays as dragged without a graph");
      const divider = await visibleCenter(browser, await browser.$$('.monitor [data-pane-resizer]')[1].getElement());
      await browser.action("pointer").move(divider).perform();
      assert.equal(await browser.$('[role="tooltip"], [role="menu"]').isExisting(), false, "hovering the divider opens nothing");
      await browser.$('[aria-label="Scope"] [data-index="0"]').click();
      await graphShown(browser, "a:docs");
      assert.equal(await graph(), split, "the restored graph keeps the dragged split");
    },
    async (browser, { data: { width } }) => {
      await browser.$('[aria-label="Scope"] [data-index="0"]').waitForExist({ timeout: 15000 });
      await browser.$('[aria-label="Scope"] [data-index="0"]').click();
      await graphShown(browser, "a:docs");
      await openArtifact(browser, "a:docs", "Artifact docs");
      const actual = await browser.execute(() => Math.round(document.querySelector('aside[aria-label="Detail"]')!.getBoundingClientRect().width));
      assert.ok(Math.abs(actual - width) <= 1, `the fresh service-owned app launch restores ${width}px, got ${actual}`);
    },
  ),

  // UX edges: E-MON-SCOPE-MOVE E-MON-SCOPE-RUNS E-MON-RUNS-MOVE E-MON-RUNS-GRAPH
  // E-MON-DETAIL-ARTIFACT E-MON-DETAIL-TAB E-HUMAN-ESC-DETAIL E-MON-GRAPH-TO-RUNS E-MON-RUNS-TO-SCOPE
  scenario<NavigationData>(
    "monitor-navigate-journey",
    "walks Scope → another Run → its graph by keys, opens an Artifact's Detail and eval tab by clicks, and steps back to Scope by keys",
    async () => {
      const { fixture, ...data } = await createNavigationFixture();
      return { fixture, data };
    },
    async (browser, { data: example }) => {
      const pane = (name: string) => browser.$(`[role="listbox"][aria-label="${name}"]`);
      const selectedRun = () => browser.execute(() => document.querySelector('[aria-label="Runs"] [aria-selected="true"] .id')?.textContent?.split(/\s/)[0] ?? "");
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
      const graph = () => browser.$('section.panel[aria-label^="Run run-"]');
      await press(browser, ["Enter"], () => graph().isFocused(), "that Run's graph focused");
      const opened = await openArtifact(browser, "a:subject", "Artifact subject");
      assert.deepEqual(opened.evals.map((row) => row.name), [older.eval], "the older Run's own criterion");
      await openTab(browser, `e:subject/${older.eval}`);
      const human = () => browser.$('aside[aria-label="Detail"] [aria-label="Human review"][tabindex="0"]');
      await browser.waitUntil(() => browser.execute((name) => document.querySelector('[aria-label="Human review"] h3')?.textContent?.includes(`subject/${name}`) ?? false,
        older.eval), { timeout: 15000, timeoutMsg: "the eval's tab holds this journey's Human review" });
      await human().click();
      // Esc leaves the review's Detail, back on the graph; Esc again steps back to Runs.
      await press(browser, ["Escape"], async () => (await detail(browser)) === null && await graph().isFocused(), "the Detail closed, the graph focused");
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
      await graphShown(browser, "a:release");
      // A click on release, then on its sign-off's tab, opens the Human Detail.
      await openArtifact(browser, "a:release", "Artifact release");
      await openTab(browser, "e:release/signoff");
      const review = () => browser.$('[aria-label="Human review"][tabindex="0"]');
      await review().waitForExist({ timeout: 15000, timeoutMsg: "Human Detail" });
      await review().$("button*=Claim").waitForEnabled({ timeout: 15000, timeoutMsg: "the review ready to claim" });
      await review().click();
      await browser.waitUntil(() => claimable(browser), { timeout: 15000, timeoutMsg: "the claim stage" });
      await browser.keys(["c"]);
      await browser.waitUntil(async () => signoff(fixture).claim?.reviewer === "alice", { timeout: 15000, timeoutMsg: "claimed" });
      await browser.waitUntil(() => reviewing(browser), { timeout: 15000, timeoutMsg: "the review stage" });
      // Back out of the Detail to the graph, then q outside any field.
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
