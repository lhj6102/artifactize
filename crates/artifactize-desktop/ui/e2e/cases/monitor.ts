// A Run in the Main area end to end, in the real app on fixture state made by the real CLI:
// the branch's Run → its graph → an Artifact's Detail → its own tools, or its Human eval's tab
// with the shared Human Detail's claim, release, tools and their live Tool log, RED validation,
// groups of fields, Korean text and GREEN submission, a live update from another process, and
// the dragged split and Detail width.
import assert from "node:assert/strict";
import { artifactize, create, finishTalk, type Fixture, launched, requests, signoff, slowTool, type SignoffDefinition } from "../fixture.ts";
import { claimable, reviewing } from "../human-review.ts";
import { sessionEndedByApp } from "../observation.ts";
import { afterRender, fill, press } from "../keys.ts";
import { checkpoint, commandsRendered } from "../commands.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { nativeInput, nativePath, primaryModifier, readText, writeText } from "../platform.ts";
import { visibleCenter } from "../pointer.ts";
import { clickOption, dispatchInput, tab } from "../input.ts";
import { emptyRepository } from "../catalog-fixtures.ts";
import { textContrast } from "../rendering.ts";
import { detail, graphShown, openArtifact, openTab, tabAt } from "../graph-view.ts";
import { branch, ENTRIES, expand, listed, MAIN, mainTitle, selectBranch } from "../pane.ts";
import { runOf as runIn, toolLog as logIn } from "../tool-log.ts";

/** A fresh release waiting for its sign-off, the app started on its branch: its Run shown. */
async function atRepo(definition?: SignoffDefinition) {
  const fixture = await create(definition);
  return { fixture, args: ["--repo", fixture.repo], data: null };
}

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
    await browser.waitUntil(async () => (await human().$(".meta").getText()).startsWith(`Request ${id} · DEFERRED · unclaimed · waiting `), {
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

/** The nonblank lines of `text`, as typed or pasted on any platform. */
const linesOf = (text: string) => text.split(/\r?\n/).filter((line) => line !== "");

/** Whether `text` holds `lines` unchanged, in order, with no other line between them. */
function holds(text: string, lines: string[]): boolean {
  const got = linesOf(text);
  return got.some((_, at) => lines.every((line, step) => got[at + step] === line));
}

/** Where `text` first leaves `lines`, from where their first line appears in it. */
function difference(lines: string[], text: string): string {
  const got = linesOf(text);
  const start = Math.max(got.indexOf(lines[0]), 0);
  const at = lines.findIndex((line, step) => got[start + step] !== line);
  return `the log has ${lines.length} lines, the pasted text ${got.length}; log line ${at} ${JSON.stringify(lines[at])}, pasted ${JSON.stringify(got[start + at])}`;
}

export const scenarios: Scenario[] = [
  // UX journeys: J-LAUNCH J-MAIN-RUN; UX edges: E-LAUNCH-REPO E-MON-RUNS-GRAPH
  scenario<null>(
    "monitor-run",
    "shows the branch's Run under its headline as its graph, with release's waiting sign-off a click away",
    () => atRepo(),
    async (browser, { fixture }) => {
      await monitorUI(browser, fixture);
      assert.equal(await browser.$(".brand").getText(), "Artifactize");
      await browser.waitUntil(() => browser.execute(() => {
        const mark = document.querySelector<HTMLImageElement>(".brand img");
        return !!mark && mark.complete && mark.naturalWidth > 0;
      }), { timeout: 15000, timeoutMsg: "the approved brand image finished loading successfully" });
      assert.equal(await browser.$(".brand img").execute((image) => {
        const mark = image as HTMLImageElement;
        return mark.complete && mark.naturalWidth > 0;
      }), true, "the approved brand mark actually loaded");
      const run = signoff(fixture).runId;
      assert.equal(await mainTitle(browser), `Run ${run}`);
      assert.deepEqual((await branch(browser, "repo  master"))?.chips.map((chip) => [chip.id, chip.filled]), [[run, true]]);
      const headline = await browser.$(`${MAIN} button.headline`).getText();
      assert.match(headline, /WAITING_HUMAN/);
      assert.match(headline, /validation pending/);
      // What needs attention is the Run's own, in its Main area above its graph: the Waiting
      // stack in the Pane names the same sign-off.
      const attention = () => browser.execute((main) => {
        const area = document.querySelector(main)!;
        const graph = area.querySelector(".svelte-flow")?.getBoundingClientRect();
        return [...area.querySelectorAll<HTMLElement>("*")]
          .filter((element) => !element.closest(".svelte-flow") && [...element.childNodes].some((node) => node.nodeType === Node.TEXT_NODE && node.textContent?.includes("release/signoff")))
          .map((element) => ({ text: element.textContent?.trim() ?? "", above: !!graph && element.getBoundingClientRect().bottom <= graph.top + 1 }));
      }, MAIN);
      await browser.waitUntil(async () => (await attention()).some((line) => line.above),
        { timeout: 15000, timeoutMsg: `what needs attention reads in the Run's Main area above its graph: ${JSON.stringify(await attention())}` });
      // The Run fills the right side as its graph, with every Artifact as a node.
      for (const id of ["a:release", "a:ship", "a:docs"]) await graphShown(browser, id);
      // Enter steps from the Pane into the Main area; a click on release shows its waiting sign-off.
      await browser.waitUntil(() => browser.$(ENTRIES).isFocused(), { timeout: 15000, timeoutMsg: "the Pane's entries focused" });
      await press(browser, ["Enter"], () => browser.$(MAIN).isFocused(), "the Main area focused");
      const opened = await openArtifact(browser, "a:release", "Artifact release");
      assert.deepEqual(opened.evals.map((row) => row.name), ["signoff"]);
      assert.match(opened.evals[0].status, /waiting for a Human|Human/, "the sign-off waits for a Human");
    },
  ),

  // UX edges: E-HUMAN-CLAIMED-ELSEWHERE E-HUMAN-RELEASED-ELSEWHERE E-HUMAN-CLAIM E-HUMAN-RELEASE E-MON-LIVE
  scenario<null>(
    "monitor-external-claim",
    "follows a claim another process makes, then claims and releases explicitly",
    () => atRepo(),
    async (browser, { fixture }) => {
      const { text, human, openReview } = await monitorUI(browser, fixture);
      await openReview(false);
      const id = signoff(fixture).id;
      const meta = () => human().$(".meta").getText();
      assert.equal(artifactize(fixture, "request", "claim", id, "--reviewer", "bob"), 0);
      await browser.waitUntil(async () => (await meta()).startsWith(`Request ${id} · DEFERRED · claimed by bob · read-only · waiting `) &&
        (await human().getText()).includes("Claimed by bob. Read-only until released.") && !(await human().$("button*=Claim").isExisting()),
      { timeout: 12000, timeoutMsg: "the opened Human Detail displays bob's exact claim and is read-only" });
      assert.equal(artifactize(fixture, "request", "unclaim", id, "--reviewer", "bob"), 0);
      await browser.waitUntil(async () => (await meta()).startsWith(`Request ${id} · DEFERRED · unclaimed · waiting `) &&
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
    () => atRepo(),
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
      assert.equal(saved.status, "DEFERRED", "tool output is not a Human verdict");
      assert.equal(saved.claim?.reviewer, "alice");
      assert.ok(await human().isExisting(), "the same Human Detail remains open");
    },
  ),

  // UX edges: E-HUMAN-TOOL-LAUNCH
  scenario<null>(
    "monitor-tool-launch",
    "runs a launch tool, says the application was opened, and logs it until it exits",
    () => atRepo(),
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
    () => atRepo(),
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
    () => atRepo(),
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
      // The Tool log as it shows, line by line. Engines serialize the rest of a whole-page
      // selection differently (WebView2 copies form field values that a selection string
      // leaves out, and blank lines between blocks vary), so the result is checked for the
      // log's own lines, unchanged and in order with nothing between them.
      const shown = await browser.execute((log) => [...document.querySelectorAll(`${log} p`)].map((row) => row.textContent ?? ""), LOG);
      // The pristine provider has no clipboard shortcuts. The evaluated device uses only
      // the WebView's editing API; it copies our selection and pastes into the focused field.
      await nativeInput(browser, { type: "copy" });
      await browser.$("#field-comment").click();
      await nativeInput(browser, { type: "paste" });
      let pasted = "";
      await browser.waitUntil(async () => holds(pasted = await browser.$("#field-comment").getValue(), shown),
        { timeout: 15000, timeoutMsg: "the copied Tool log text pasted unchanged into the result" })
        .catch((error: Error) => { throw new Error(`${error.message}: ${pasted ? difference(shown, pasted) : "nothing pasted"}`); });
      // A submission clears the Tool log; the completed result stays open.
      await browser.keys([primaryModifier, "s"]);
      await browser.waitUntil(() => signoff(fixture).status === "GREEN", { timeout: 15000, timeoutMsg: "GREEN saved" });
      assert.deepEqual(signoff(fixture).result, { verdict: "GREEN", approved: true, comment: pasted });
      await browser.waitUntil(async () => (await toolLog()).rows.length === 0, { timeout: 15000, timeoutMsg: "the Tool log cleared by the submission" });
      await browser.waitUntil(async () => /completed result/i.test(await human().getText()), { timeout: 15000, timeoutMsg: "the completed result open" });
    },
  ),

  // UX edges: E-HUMAN-TOOL-FAIL
  scenario<null>(
    "monitor-tool-failure",
    "says a tool failed and ends its run red in the Tool log",
    () => atRepo(),
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
      assert.equal(saved.status, "DEFERRED", "a failed tool is not a Human verdict");
      assert.equal(saved.result, null);
      assert.equal(saved.claim?.reviewer, "alice", "the same Human review remains claimed");
      assert.ok(await human().isExisting(), "the same Human Detail remains open after the tool error");
    },
  ),

  // UX edges: E-HUMAN-GREEN E-HUMAN-TOOL E-HUMAN-ESC-CANCEL E-HUMAN-ESC-FORM E-HUMAN-ESC-DETAIL E-MON-DETAIL-BACK
  scenario<null>(
    "monitor-tool-cancel",
    "steps back one thing per Esc from the Detail's tabs: cancels a running tool, then leaves the form, then closes the Detail",
    () => atRepo(),
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

  // UX journey: J-HUMAN-RED; UX edges: E-HUMAN-RED E-HUMAN-INVALID E-HUMAN-SWITCH
  scenario<null>(
    "monitor-invalid-red",
    "keeps a RED or GREEN result that fails its schema correctable and shows the failing path",
    () => atRepo(),
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
      assert.equal(signoff(fixture).status, "DEFERRED");
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
      assert.equal(saved.status, "DEFERRED");
      assert.equal(saved.result, null, "switching did not submit the rejected draft");
      assert.equal(saved.claim?.reviewer, "alice", "the same review remains claimed and editable");
      // A GREEN result failing its schema the same way: the required comment left empty.
      await press(browser, [primaryModifier, "g"], () => browser.$("#field-comment").isExisting(), "the GREEN form again");
      await fill(browser, "#field-comment", "");
      const before = await alert.getText().catch(() => "");
      await browser.keys([primaryModifier, "s"]);
      await browser.waitUntil(async () => {
        const shown = await browser.$('[role="alert"]');
        return await shown.isExisting() && /comment/.test(await shown.getText()) && (await shown.getText()) !== before;
      }, { timeout: 15000, timeoutMsg: "the GREEN result's failing path shown" });
      assert.equal(signoff(fixture).status, "DEFERRED", "the invalid GREEN result was not submitted");
      await fill(browser, "#field-comment", "checked again");
      assert.equal(await browser.$("#field-comment").getValue(), "checked again", "the GREEN form stays correctable");
    },
  ),

  // UX edges: E-HUMAN-RED E-HUMAN-LIST-ADD E-HUMAN-LIST-REMOVE E-HUMAN-GROUP-ADD E-HUMAN-GROUP-REMOVE E-HUMAN-SUBMIT
  scenario<null>(
    "monitor-red-groups",
    "enters a list of steps one text at a time and a list of findings as groups of fields, each added and removed, and submits them",
    () => atRepo(),
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
      // Submitted: the form gives way to the completed RED result, which shows what was sent.
      const completed = () => browser.execute(() => document.querySelector<HTMLElement>('[aria-label="Human review"]')?.innerText ?? "");
      await browser.waitUntil(async () => /completed result/i.test(await completed()), { timeout: 15000, timeoutMsg: "the completed RED result is open" });
      assert.equal(await browser.$("#field-reason").isExisting(), false, "the completed result replaced the form");
      const shown = await completed();
      for (const value of ["RED", "two problems found", "check the title", "date"]) assert.ok(shown.includes(value), `the completed result shows ${value}`);
    },
  ),

  // UX journey: J-HUMAN-FORM
  scenario<null>(
    "monitor-form-scalars",
    "edits a number, a boolean and a choice, marks required fields and shows Optional hints before submitting typed values",
    () => atRepo({ title: "Scalar form", instruction: "Read the release notes and approve them.", scalars: true }),
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
    () => atRepo({ title: "Grouped tools", instruction: "Read {docs} and approve." }),
    async (browser, { fixture }) => {
      const { human, openReview, runOf, toolLog } = await monitorUI(browser, fixture, "Read {docs} and approve.");
      await openReview();
      const groups = await browser.execute(() => [...document.querySelectorAll('[aria-label="Tools"] .group')].map((group) => group.getAttribute("aria-label")));
      assert.deepEqual(groups, ["Tools of release", "Tools of docs"], "own tools precede the alphabetically earlier dependency");
      await human().$('[aria-label="Tools of release"]').$('button=▶ notes').click();
      await browser.waitUntil(async () => (await runOf("notes"))?.at(-1)?.kind === "end", { timeout: 15000, timeoutMsg: "release's notes completed" });
      await human().$('[aria-label="Tools of docs"]').$('button=▶ reference').click();
      await browser.waitUntil(async () => (await human().getText()).includes("Documentation checked."), { timeout: 15000, timeoutMsg: "the dependency's own notes were printed" });
      // The printed text is not its end: wait for docs's reference run to end, and end well.
      await browser.waitUntil(async () => (await runIn(browser, '[aria-label="Human review"]', "reference", "docs"))?.at(-1)?.kind === "end",
        { timeout: 15000, timeoutMsg: "docs's reference completed" });
      const reference = (await runIn(browser, '[aria-label="Human review"]', "reference", "docs"))!;
      assert.ok(!reference.some((row) => row.error) && !(await toolLog()).noticeError, `docs's reference succeeded: ${JSON.stringify(reference)}`);
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
    () => atRepo({ title: "Long Tool log", instruction: "Read the release notes and approve them.", report: true }),
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
    () => atRepo(),
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
      await expand(browser, "repo  master");
      await browser.waitUntil(async () => /^GREEN /.test((await branch(browser, "repo  master"))?.chips.find((chip) => chip.id === run)?.text ?? ""),
        { timeout: 15000, timeoutMsg: "the app has rendered this Run's terminal GREEN update on its chip" });
      await commandsRendered(browser);
      assert.ok(await human().isExisting(), "the completed Detail stays open after the terminal subscription update");
      const completed = await browser.execute(() => document.querySelector<HTMLElement>('[aria-label="Human review"]')?.innerText ?? "");
      assert.match(completed, /completed result/i);
      assert.match(completed, /Submitted GREEN for release\/signoff/);
    },
  ),

  // UX journeys: J-PANE J-RESIZE; UX edges: E-MON-RESIZE E-MON-DETAIL-ARTIFACT E-MON-DETAIL-WIDTH E-MON-DETAIL-BACK
  scenario<{ width: number }>(
    "monitor-resize",
    "resizes the Main area against the Pane by dragging their divider, kept whatever the Main area shows, and the Detail by dragging its edge, kept for the next launch",
    async () => {
      const fixture = await create();
      return { fixture, args: ["--repo", emptyRepository(fixture)], data: { width: 400 } };
    },
    async (browser, { fixture, data: { width } }) => {
      // Started on the branch without Runs; the release's branch shows its Run.
      await listed(browser, "repo  master");
      await selectBranch(browser, "repo  master");
      await monitorUI(browser, fixture);
      const main = () => browser.execute(() => Math.round(document.querySelector<HTMLElement>(".monitor .stage")!.getBoundingClientRect().width));
      const divider = () => browser.$(".monitor [data-pane-resizer]");
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
      const layout = () => browser.execute(() => {
        const resizer = document.querySelector<HTMLElement>(".monitor [data-pane-resizer]");
        const box = resizer?.getBoundingClientRect();
        return {
          window: [innerWidth, innerHeight, devicePixelRatio],
          resizer: box && [box.left, box.top, box.width, box.height],
          state: resizer && [...resizer.attributes].map((attribute) => `${attribute.name}=${attribute.value}`),
          panes: [...document.querySelectorAll<HTMLElement>(".monitor [data-pane]")].map((pane) => Math.round(pane.getBoundingClientRect().width)),
          events: (window as unknown as { resizeLog?: string[] }).resizeLog?.slice(0, 20),
        };
      });
      assert.equal((await browser.$$(".monitor [data-pane-resizer]")).length, 1, "one divider, between the Pane and the Main area");
      const before = await main();
      await watchMouse();
      // Paneforge installs its move listener in a post-press render effect. Keep the real
      // pointer held across that render boundary, rather than guessing how long it takes.
      // Press 2px past the line, over the Main area: the grab zone reaches over both sides.
      await browser.action("pointer", { id: "resize" }).move({ origin: await divider(), x: 2, y: 0 }).down().perform(true);
      try {
        await afterRender(browser);
        const pointer = browser.action("pointer", { id: "resize" });
        for (let step = 0; step < 6; step += 1) pointer.move({ origin: "pointer", x: -20, y: 0 });
        await pointer.perform(true);
        try {
          await browser.waitUntil(async () => (await main()) > before + 60, { timeout: 15000, timeoutMsg: "the dragged Main area" });
        } catch (error) {
          throw new Error(`the dragged Main area: ${JSON.stringify(await layout())}`, { cause: error });
        }
        await browser.action("pointer", { id: "resize" }).up().perform();
      } finally { await browser.releaseActions(); }
      const split = await main();
      assert.ok(split > before + 60, "the Main area grew by the drag");

      // The Detail's left edge: a drag widens it within the Main area, and it opens again at
      // that width.
      await openArtifact(browser, "a:docs", "Artifact docs");
      const placed = () => browser.execute(() => {
        const dock = document.querySelector('.stage aside[aria-label="Detail"]')?.getBoundingClientRect();
        const stage = document.querySelector(".stage")!.getBoundingClientRect();
        return dock ? { width: Math.round(dock.width), left: Math.round(dock.left - stage.left), right: Math.round(stage.right - dock.right) } : null;
      });
      const shown = (await placed())!;
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
        return !!widened && widened.width > shown.width + 60;
      }, { timeout: 15000, timeoutMsg: "the dragged Detail widened" })
        .catch((error: unknown) => { throw new Error(`${String(error)}: ${JSON.stringify({ shown, widened })}`); });
      assert.ok(widened!.left > 0 && widened!.right > 0, `the Detail stays within the Main area: ${JSON.stringify(widened)}`);
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
      assert.ok(readable && minimum.width <= shown.width && minimum.left > 0 && minimum.right > 0, "the minimum is a readable, contained column, not collapsed text");
      await dragEdge(Math.round(room.right - 1));
      assert.equal((await placed())?.width, minimum.width, "dragging past the minimum cannot shrink the column further");
      // Dragged as far left as the pointer goes, over the Pane: it stops short of the Pane.
      await dragEdge(Math.round(room.left - 40));
      const maximum = (await placed())!;
      assert.ok(maximum.width > minimum.width && maximum.width < room.width && maximum.left > 0 && maximum.right > 0, "the widest Detail stays in the Main area, never over the Pane");
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
        divider: getComputedStyle(document.querySelector(".monitor [data-pane-resizer]")!).cursor,
      }));
      assert.match(cursor.edge, /(?:ew|col)-resize/);
      assert.match(cursor.divider, /(?:ew|col)-resize/);
      const edgePoint = await visibleCenter(browser, await browser.$('aside[aria-label="Detail"] [aria-label="Detail width"]').getElement());
      await browser.action("pointer").move(edgePoint).perform();
      assert.equal(await browser.$('[role="tooltip"], [role="menu"]').isExisting(), false, "hovering the edge opens nothing");
      await press(browser, ["Escape"], async () => (await placed()) === null, "the Detail closes");
      // The split stays as dragged whatever the Main area shows: another branch's Current,
      // then Settings, then the Run again.
      await selectBranch(browser, "empty-repo  master");
      await browser.waitUntil(async () => (await mainTitle(browser)) === "Current · empty-repo  master", { timeout: 15000, timeoutMsg: "the other branch's Current shown" });
      assert.equal(await main(), split, "the split stays as dragged on a Current");
      await browser.$("button*=Settings").click();
      await browser.waitUntil(async () => (await mainTitle(browser)) === "Settings", { timeout: 15000, timeoutMsg: "Settings shown" });
      assert.equal(await main(), split, "the split stays as dragged on Settings");
      const line = await visibleCenter(browser, await divider().getElement());
      await browser.action("pointer").move(line).perform();
      assert.equal(await browser.$('[role="tooltip"], [role="menu"]').isExisting(), false, "hovering the divider opens nothing");
      await selectBranch(browser, "repo  master");
      await graphShown(browser, "a:docs");
      assert.equal(await main(), split, "the Run shown again keeps the dragged split");
    },
    async (browser, { data: { width } }) => {
      await listed(browser, "repo  master");
      await selectBranch(browser, "repo  master");
      await graphShown(browser, "a:docs");
      await openArtifact(browser, "a:docs", "Artifact docs");
      const actual = await browser.execute(() => Math.round(document.querySelector('aside[aria-label="Detail"]')!.getBoundingClientRect().width));
      assert.ok(Math.abs(actual - width) <= 1, `the fresh service-owned app launch restores ${width}px, got ${actual}`);
    },
  ),

  // UX journey: J-DETAIL-TOOLS; UX edges: E-MON-DETAIL-ARTIFACT E-DETAIL-TOOL E-DETAIL-TOOL-LAUNCH E-DETAIL-TOOL-FAIL
  scenario<null>(
    "monitor-detail-tools",
    "runs an Artifact's own tools from its Detail at once, unclaimed and recorded as no review's evidence: output, a launch and a failure in its Tool log",
    () => atRepo(),
    async (browser, { fixture }) => {
      const DETAIL = 'aside[aria-label="Detail"]';
      await monitorUI(browser, fixture);
      await openArtifact(browser, "a:release", "Artifact release");
      const tools = () => browser.$(DETAIL).$('section[aria-label="Tools of release"]');
      await tools().$("button=▶ notes").waitForEnabled({ timeout: 15000, timeoutMsg: "release's own tools listed" });
      // Below its evals and above its path: the Tools, then their Tool log.
      const order = await browser.execute((root) => {
        const dock = document.querySelector(root)!;
        const parts = [dock.querySelector('[aria-label^="Evals of "]'), dock.querySelector('section[aria-label="Tools of release"]'),
          dock.querySelector('[role="log"][aria-label="Tool log"]'), dock.querySelector('[role="document"] .card > p.line')];
        return parts.every(Boolean) && parts.every((part, at) => at === 0 || (parts[at - 1]!.compareDocumentPosition(part!) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0);
      }, DETAIL);
      assert.ok(order, "its evals, then its Tools and their Tool log, then its path");
      assert.equal(await browser.$(".graph .svelte-flow__node button").isExisting(), false, "the graph's nodes carry no tool buttons");
      const toolRunsBefore = (await checkpoint(browser)).activity.commands.artifactToolRun?.completed ?? 0;
      // An output tool runs at once, unclaimed: its start, each line it printed and its exit.
      await tools().$("button=▶ notes").click();
      await browser.waitUntil(async () => (await runIn(browser, DETAIL, "notes"))?.at(-1)?.kind === "end", { timeout: 15000, timeoutMsg: "the notes run ended" });
      assert.deepEqual(await runIn(browser, DETAIL, "notes"), [
        { text: "▶ HH:MM:SS · notes · release", kind: "start", error: false },
        { text: "# Release notes", kind: "line", error: false },
        { text: "- Ready to ship.", kind: "line", error: false },
        { text: "■ exit 0", kind: "end", error: false },
      ]);
      assert.equal((await logIn(browser, DETAIL)).noticeError, false);
      assert.ok(((await checkpoint(browser)).activity.commands.artifactToolRun?.completed ?? 0) > toolRunsBefore, "the same probe observes this tool's run");
      assert.equal(signoff(fixture).claim, null, "running a tool claimed nothing");
      // A launch tool answers at once and says so; its application runs and its exit ends the run.
      assert.equal(launched(fixture), false);
      await tools().$("button=▶ open").click();
      await browser.waitUntil(async () => (await logIn(browser, DETAIL)).notice === "open_release launched.", { timeout: 15000, timeoutMsg: "the launch said" });
      await browser.waitUntil(() => launched(fixture), { timeout: 15000, timeoutMsg: "the launched application ran" });
      await browser.waitUntil(async () => (await runIn(browser, DETAIL, "open"))?.at(-1)?.text === "■ exit 0", { timeout: 15000, timeoutMsg: "its exit logged" });
      // A launch tool whose application keeps running still returns at once: the launch is said,
      // the Detail's tools are usable again and its run stays open in the Tool log.
      await tools().$("button=▶ talk").click();
      await browser.waitUntil(async () => (await logIn(browser, DETAIL)).notice === "talk_release launched." &&
        (await runIn(browser, DETAIL, "talk"))?.some((row) => row.text === "opened the stand-in") === true &&
        await tools().$("button=▶ notes").isEnabled(), { timeout: 15000, timeoutMsg: "the launch returned while its application runs" });
      assert.ok(!(await runIn(browser, DETAIL, "talk"))!.some((row) => row.kind === "end"), "the launched application has not ended");
      // Let it go on: what it prints next joins its run, and its exit ends it.
      finishTalk(fixture);
      await browser.waitUntil(async () => (await runIn(browser, DETAIL, "talk"))?.at(-1)?.text === "■ exit 0", { timeout: 15000, timeoutMsg: "the stand-in's exit logged" });
      assert.ok((await runIn(browser, DETAIL, "talk"))!.some((row) => row.text === "chose: B"), "its later lines joined its own run");
      // A failing tool says so, its run ended red.
      await tools().$("button=▶ fail").click();
      await browser.waitUntil(async () => (await runIn(browser, DETAIL, "fail"))?.at(-1)?.kind === "end" && (await logIn(browser, DETAIL)).noticeError,
        { timeout: 15000, timeoutMsg: "the failing tool's end and error" });
      assert.deepEqual(await runIn(browser, DETAIL, "fail"), [
        { text: "▶ HH:MM:SS · fail · release", kind: "start", error: false },
        { text: "■ exit 1", kind: "end", error: true },
      ]);
      // None of it is a review's evidence: the sign-off still waits, unclaimed, its own Tool log empty.
      const saved = signoff(fixture);
      assert.deepEqual([saved.status, saved.claim, saved.result], ["DEFERRED", null, null]);
      assert.ok(requests(fixture).every((request) => request.claim === null), "no request was claimed");
      await openTab(browser, "e:release/signoff");
      await browser.$('[aria-label="Human review"]').waitForExist({ timeout: 15000 });
      await browser.waitUntil(async () => (await logIn(browser, '[aria-label="Human review"]')).rows.length === 0 &&
        await browser.$('[aria-label="Human review"] [role="log"][aria-label="Tool log"]').isExisting(), { timeout: 15000, timeoutMsg: "the review's own Tool log is empty" });
    },
  ),

  // UX journey: J-DETAIL-TOOLS; UX edges: E-MON-DETAIL-ARTIFACT E-DETAIL-TOOL
  scenario<{ run: string }>(
    "monitor-detail-tools-now",
    "runs a shown Run's Artifact tools as the branch declares them now, in its worktree as it is now, not as the Run saw it",
    async () => {
      const fixture = await create();
      const run = signoff(fixture).runId;
      // After the Run was saved: release declares one more tool, and its notes change.
      const declaration = nativePath(fixture.repo, "release", "index.artf");
      writeText(declaration, readText(declaration).replace("[views.human_tools]\n",
        '[views.human_tools]\nlatest = { description = "Print the notes as they are now.", builtin = "read", args = ["{artifactPath}/notes.md"] }\n'));
      writeText(nativePath(fixture.repo, "release", "notes.md"), "# Release notes\n- Changed after the Run.\n");
      return { fixture, args: ["--repo", fixture.repo], data: { run } };
    },
    async (browser, { data: { run } }) => {
      const DETAIL = 'aside[aria-label="Detail"]';
      await browser.waitUntil(async () => (await mainTitle(browser)) === `Run ${run}`, { timeout: 15000, timeoutMsg: "the Run saved before the change shown" });
      await graphShown(browser, "a:release");
      await openArtifact(browser, "a:release", "Artifact release");
      const tools = () => browser.$(DETAIL).$('section[aria-label="Tools of release"]');
      // The tool declared only after the Run was saved is listed and runs.
      await tools().$("button=▶ latest").waitForEnabled({ timeout: 15000, timeoutMsg: "the tool declared now listed" });
      await tools().$("button=▶ latest").click();
      await browser.waitUntil(async () => (await runIn(browser, DETAIL, "latest"))?.at(-1)?.kind === "end", { timeout: 15000, timeoutMsg: "the latest run ended" });
      const latest = (await runIn(browser, DETAIL, "latest"))!;
      assert.deepEqual([latest[0], latest.at(-1)], [
        { text: "▶ HH:MM:SS · latest · release", kind: "start", error: false },
        { text: "■ done", kind: "end", error: false },
      ]);
      assert.ok(latest.some((row) => row.kind === "line" && row.text.includes("- Changed after the Run.")), `it read today's notes: ${JSON.stringify(latest)}`);
      // A command tool runs in the worktree as it is now: it prints the changed notes.
      await tools().$("button=▶ notes").click();
      await browser.waitUntil(async () => (await runIn(browser, DETAIL, "notes"))?.at(-1)?.kind === "end", { timeout: 15000, timeoutMsg: "the notes run ended" });
      assert.deepEqual(await runIn(browser, DETAIL, "notes"), [
        { text: "▶ HH:MM:SS · notes · release", kind: "start", error: false },
        { text: "# Release notes", kind: "line", error: false },
        { text: "- Changed after the Run.", kind: "line", error: false },
        { text: "■ exit 0", kind: "end", error: false },
      ]);
    },
  ),

  // UX journey: J-QUIT; UX edges: E-QUIT-ASK E-QUIT-STAY E-QUIT-RELEASE
  scenario<null>(
    "monitor-quit-claim",
    "asks before quitting with a claim taken here: Esc stays, u releases and quits",
    () => atRepo(),
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
      assert.equal(signoff(fixture).status, "DEFERRED");
    },
  ),
];
