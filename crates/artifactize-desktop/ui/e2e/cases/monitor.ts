// A Run in the Main area end to end, in the real app on fixture state made by the real CLI:
// the branch's Run → its graph → an Artifact's Detail → its own tools, or its Human eval's tab
// with the shared Human Detail's claim, release, tools and their live Tool log, RED validation,
// groups of fields, Korean text and GREEN submission, a live update from another process, and
// the dragged split and Detail width.
import assert from "node:assert/strict";
import { artifactize, create, finishTalk, type Fixture, launched, requests, signoff, slowTool, type SignoffDefinition } from "../fixture.ts";
import { claimable, reviewing } from "../human-review.ts";
import { sessionEndedByApp } from "../observation.ts";
import { joinGatedTools } from "../process-fixtures.ts";
import { afterRender, fill, press, typeText } from "../keys.ts";
import { checkpoint } from "../commands.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { nativeInput, nativePath, primaryModifier, readText, removeTree, serveLoopbackHttp, writeText, type NativePath } from "../platform.ts";
import { visibleCenter } from "../pointer.ts";
import { clickOption, dispatchInput, tab } from "../input.ts";
import { emptyRepository } from "../catalog-fixtures.ts";
import { textContrast } from "../rendering.ts";
import { detail, graphShown, openArtifact, openTab, tabAt } from "../graph-view.ts";
import { branch, ENTRIES, expand, listed, MAIN, mainTitle, selectBranch } from "../pane.ts";
import { runOf as runIn, toolLog as logIn } from "../tool-log.ts";
import { copyCargoUpdate, prepareCargoUpdate } from "./release.ts";

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
  /** The Tool log's rows and what the review said: only a failure is said, in red, in the
   * Result's row of actions. */
  const toolLog = () => browser.execute(() => {
    const detail = document.querySelector('[aria-label="Human review"]');
    const notice = detail?.querySelector('section[aria-label="Result"] .actions .said [role="status"]:not(.busy)');
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
  /** Release's tool button, usable after the claim. Callers observe its own output or effect. */
  async function tool(operation: string) {
    const button = await human().$(`button=${operation}`);
    await button.waitForEnabled({ timeout: 15000, timeoutMsg: `release's ${operation} tool usable` });
    return { button };
  }

  /** The request's state, as its Debug section says it once unfolded. */
  const meta = () => browser.execute(() => {
    const term = [...document.querySelectorAll('[aria-label="Human review"] dt')].find((dt) => dt.textContent?.trim() === "State");
    return term?.nextElementSibling?.textContent?.trim() ?? "";
  });
  async function openReview(claim = true) {
    await openArtifact(browser, "a:release", "Artifact release");
    await openTab(browser, "e:release/signoff");
    await human().waitForExist({ timeout: 15000, timeoutMsg: "this case's Human Detail" });
    const id = signoff(fixture).id;
    // It opens with its stage alone, the request's facts folded under Debug.
    await browser.waitUntil(async () => (await human().$(".stage").getText()) === "CLAIM", { timeout: 15000, timeoutMsg: "the review opens at its claim stage" });
    assert.equal(await human().$(".instruction .text").getText(), instruction, "the selected Human eval's default instruction");
    assert.equal(await human().$("button.head=Debug").getAttribute("aria-expanded"), "false", "the request's facts start folded under Debug");
    // It reads top to bottom as the instruction, the Tools, their Tool log, the Result and Debug.
    const order = await browser.execute(() => [...document.querySelectorAll('[aria-label="Human review"] .one > *')]
      .map((part) => part.getAttribute("aria-label") ?? "").filter((label) => label !== ""));
    assert.deepEqual(order, ["Instruction", "Tools", "Tool log", "Result", "Debug"]);
    await human().$("button.head=Debug").click();
    await browser.waitUntil(async () => (await meta()).startsWith(`Request ${id} · DEFERRED · unclaimed · waiting `), {
      timeout: 15000, timeoutMsg: "Debug identifies this case's exact waiting request",
    });
    if (claim) {
      await press(browser, ["c"], async () => signoff(fixture).claim?.reviewer === "alice" && await reviewing(browser), "this case's claim");
      const initial = await toolLog();
      assert.deepEqual(initial.rows, [], "this case's Tool log starts empty");
      assert.equal(initial.noticeError, false, "this case's claim did not inherit a tool error");
    }
  }
  await graphShown(browser, "a:release");
  /** The review's running work: its running icon, named by what runs. */
  const running = (label: string) => human().$(`.busy [aria-label="${label}"]`).isExisting();
  return { text, human, meta, toolLog, runOf, tool, openReview, running };
}

/** The nonblank lines of `text`, as typed or pasted on any platform. */
export const scenarios: Scenario[] = [
  // UX journeys: J-LAUNCH J-MAIN-RUN
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
      assert.equal(await mainTitle(browser), run);
      assert.deepEqual((await branch(browser, "repo  master"))?.chips.map((chip) => [chip.id, chip.filled]), [[run, true]]);
      // One header row: the Run's ID, then its verdict and what needs attention.
      const headline = (await browser.$(`${MAIN} button.headline`).getText()).trim();
      assert.match(headline, /^WAITING_HUMAN\s+release\/signoff$/);
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

  scenario<null>(
    "monitor-external-claim",
    "follows a claim another process makes, then claims and releases explicitly",
    () => atRepo(),
    async (browser, { fixture }) => {
      const { text, human, meta, openReview } = await monitorUI(browser, fixture);
      await openReview(false);
      const id = signoff(fixture).id;
      assert.equal(artifactize(fixture, "request", "claim", id, "--reviewer", "bob"), 0);
      await browser.waitUntil(async () => (await meta()).startsWith(`Request ${id} · DEFERRED · claimed by bob · read-only · waiting `) &&
        (await human().$(".claimant").isExisting()) && (await human().$(".claimant").getText()) === "bob" &&
        (await human().getText()).includes("Read-only until released.") && !(await human().$("button*=Claim").isExisting()),
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
      await press(browser, ["u"], async () => signoff(fixture).claim === null && await claimable(browser), "released");
      await press(browser, ["c"], () => signoff(fixture).claim?.reviewer === "alice", "claimed again");
      await browser.waitUntil(() => reviewing(browser), { timeout: 15000, timeoutMsg: "the review stage" });
    },
  ),

  scenario<null>(
    "monitor-release-editing",
    "releases the claim while its form is filled in, by Release claim, leaving the form, and keeps the draft for the next claim; u typed in a field is text",
    () => atRepo(),
    async (browser, { fixture }) => {
      const { human, openReview } = await monitorUI(browser, fixture);
      await openReview();
      await press(browser, ["g"], () => browser.$("#field-comment").isExisting(), "the GREEN form");
      await fill(browser, "#field-comment", "half-checked");
      // u in the field is a letter of the comment, not a release.
      await browser.$("#field-comment").click();
      await typeText(browser, "u");
      await browser.waitUntil(async () => (await browser.$("#field-comment").getValue()) === "half-checkedu", { timeout: 15000, timeoutMsg: "u typed into the comment" });
      assert.equal(signoff(fixture).claim?.reviewer, "alice", "u in a field releases nothing");
      // Release claim, from the Result's row of actions, while editing.
      await human().$("button=Release claim").click();
      await browser.waitUntil(async () => signoff(fixture).claim === null && !(await browser.$("#field-comment").isExisting()) && await claimable(browser),
        { timeout: 15000, timeoutMsg: "released, the form left" });
      // Claimed again, the same verdict opens the draft as it was left.
      await human().click();
      await press(browser, ["c"], async () => signoff(fixture).claim?.reviewer === "alice" && await reviewing(browser), "claimed again");
      await press(browser, ["g"], async () => (await browser.$("#field-comment").isExisting()) && (await browser.$("#field-comment").getValue()) === "half-checkedu",
        "the kept draft");
      assert.equal(signoff(fixture).status, "DEFERRED", "nothing was submitted");
    },
  ),

  // UX journey: J-HUMAN-TOOL
  scenario<null>(
    "monitor-tool-output",
    "keeps a tool disabled until the review is claimed, then runs it with a click and logs its output",
    () => atRepo(),
    async (browser, { fixture }) => {
      const { human, toolLog, runOf, tool, openReview } = await monitorUI(browser, fixture);
      await openReview(false);
      const beforeClaim = human().$("button=notes");
      await beforeClaim.waitForDisplayed({ timeout: 15000, timeoutMsg: "the notes tool is shown before a claim" });
      assert.equal(await beforeClaim.isEnabled(), false, "the tool cannot be used until the review is claimed");
      await beforeClaim.click();
      await afterRender(browser);
      assert.equal(await runOf("notes"), null, "clicking the unclaimed tool produces no Tool log run");
      assert.equal(signoff(fixture).claim, null, "clicking a tool does not claim the review");
      await human().$("button=Claim to review").click();
      await browser.waitUntil(async () => signoff(fixture).claim?.reviewer === "alice" && await reviewing(browser),
        { timeout: 15000, timeoutMsg: "the review is now claimed" });
      assert.deepEqual((await toolLog()).rows, [], "the claim alone does not run a tool");
      const notes = await tool("notes");
      await notes.button.click();
      await browser.waitUntil(async () => (await runOf("notes"))?.at(-1)?.kind === "end" && await reviewing(browser),
        { timeout: 15000, timeoutMsg: "the notes tool's run ended in the Tool log" });
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
      // It returns at once; what ended well is said by the log, never a sentence.
      await browser.waitUntil(async () => (await runOf("open")) !== null && await reviewing(browser), { timeout: 15000, timeoutMsg: "the launch returned" });
      assert.equal((await toolLog()).notice, "", "nothing more is said");
      // The application stand-in really ran, and its exit ends its run in the log.
      await browser.waitUntil(() => launched(fixture), { timeout: 15000, timeoutMsg: "the launched application" });
      await browser.waitUntil(async () => (await runOf("open"))?.at(-1)?.text === "■ exit 0", { timeout: 15000, timeoutMsg: "the launched application's exit logged" });
    },
  ),

  // UX journey: J-HUMAN-LOG
  scenario<{ visited: NativePath }>(
    "monitor-tool-file-handoff",
    "hands a declared file to its desktop application and ends the tool log with launched",
    async () => {
      let visited: NativePath | undefined;
      const endpoint = await serveLoopbackHttp((request, response) => {
        request.resume();
        const known = request.method === "GET" && request.url === "/file-opened";
        if (known && visited) writeText(visited, "opened");
        response.writeHead(known ? 200 : 404).end();
      });
      try {
        const input = await atRepo({ title: "A person approves the release", instruction: "Read the release notes and approve them.", fileHandoff: endpoint.url });
        const file = nativePath(input.fixture.repo, "release", "handoff.html");
        visited = nativePath(input.fixture.root, "handoff-visited");
        writeText(visited, "");
        return { ...input, data: { visited }, browser: { file }, release: () => endpoint.close() };
      } catch (error) {
        await endpoint.close();
        throw error;
      }
    },
    async (browser, { fixture, data: { visited } }) => {
      const { openReview, tool, runOf } = await monitorUI(browser, fixture);
      await openReview();
      assert.equal(readText(visited), "", "the file was not opened by setup");
      const handoff = await tool("handoff");
      await handoff.button.click();
      await browser.waitUntil(async () => (await runOf("handoff"))?.at(-1)?.text === "■ launched" && await reviewing(browser),
        { timeout: 15000, timeoutMsg: "the desktop handoff completed as launched, not as a subprocess exit" });
      await browser.waitUntil(() => readText(visited) === "opened", { timeout: 15000, timeoutMsg: "the desktop application opened the declared file" });
    },
  ),

  scenario<null>(
    "monitor-tool-log-live",
    "keeps logging a launched application under its own run after its button returned, its stderr in red, while the reviewer writes the result",
    () => atRepo(),
    async (browser, { fixture }) => {
      const { human, runOf, tool, openReview } = await monitorUI(browser, fixture);
      await openReview();
      const talk = await tool("talk");
      await talk.button.click();
      // The button returns at once; the application is still running and has said one line.
      await browser.waitUntil(async () => (await runOf("talk"))?.some((row) => row.text === "opened the stand-in") === true && await reviewing(browser),
      { timeout: 15000, timeoutMsg: "the launched stand-in's first line, with the review idle" });
      assert.ok(!(await runOf("talk"))!.some((row) => row.kind === "end"), "the launched application has not ended");
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

  scenario<Awaited<ReturnType<typeof prepareCargoUpdate>>["data"]>(
    "monitor-tool-log-scroll",
    "owns the clipboard: copies the Cargo update command, then scrolls and copies the Tool log and clears it on submission",
    async () => {
      const { fixture, args } = await atRepo();
      return { ...await prepareCargoUpdate(fixture), args };
    },
    async (browser, input) => {
      await copyCargoUpdate(browser, input);
      const { fixture } = input;
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
          review: log.closest<HTMLElement>(".one")!.scrollTop,
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
      await browser.waitUntil(async () => (await box()).below < 24,
        { timeout: 15000, timeoutMsg: "the Tool log follows its newest line again" });
      // Its text is ordinary selectable page text. Select it through input, then copy
      // and paste through the engine's own editing commands.
      // The tool buttons let go of the keyboard while they ran: the instruction takes it.
      await human().$('[aria-label="Instruction"]').click();
      await press(browser, ["g"], () => browser.$("#field-comment").isExisting(), "the GREEN form");
      // Click the start of a release-note line, then Shift+click its end. The selection
      // belongs wholly to the Tool log, not the surrounding form.
      const line = browser.$(`${LOG} p:nth-last-child(2)`);
      await line.scrollIntoView({ block: "nearest" });
      await afterRender(browser);
      const endpoint = async (end: boolean) => browser.execute((row: HTMLElement, end) => {
        const range = document.createRange();
        range.selectNodeContents(row);
        const boxes = range.getClientRects();
        const box = boxes[end ? boxes.length - 1 : 0];
        const point = { x: end ? box.right - 1 : box.left + 1, y: box.top + box.height / 2 };
        if (!row.contains(document.elementFromPoint(point.x, point.y))) throw new Error("the Tool log line is not visible at the selection endpoint");
        return point;
      }, await line.getElement() as unknown as HTMLElement, end);
      const wanted = await line.getText();
      assert.equal(wanted, "- Ready to ship.");
      await dispatchInput(browser, { kind: "text-click", ...await endpoint(false) });
      // Measure after the first click focused the scrollable log, not in its former geometry.
      await afterRender(browser);
      await dispatchInput(browser, { kind: "text-click", ...await endpoint(true), shift: true });
      assert.equal(await browser.execute(() => getSelection()?.toString() ?? ""), wanted, "only the log line is selected");
      await nativeInput(browser, { type: "copy" });
      await browser.$("#field-comment").click();
      await nativeInput(browser, { type: "paste" });
      await browser.waitUntil(async () => await browser.$("#field-comment").getValue() === wanted,
        { timeout: 15000, timeoutMsg: "the selected Tool log line pasted exactly into Result" });
      // A submission clears the Tool log; the completed result stays open.
      await browser.keys([primaryModifier, "s"]);
      await browser.waitUntil(() => signoff(fixture).status === "GREEN", { timeout: 15000, timeoutMsg: "GREEN saved" });
      assert.deepEqual(signoff(fixture).result, { verdict: "GREEN", approved: true, comment: wanted });
      await browser.waitUntil(async () => (await toolLog()).rows.length === 0, { timeout: 15000, timeoutMsg: "the Tool log cleared by the submission" });
      await browser.waitUntil(async () => /completed result/i.test(await human().getText()), { timeout: 15000, timeoutMsg: "the completed result open" });
    },
  ),

  // UX journey: J-HUMAN-LOG
  scenario<null>(
    "monitor-tool-missing",
    "ends the run of a tool that could not run with its error, in red",
    () => atRepo({ title: "A person approves the release", instruction: "Read the release notes and approve them.", missing: true }),
    async (browser, { fixture }) => {
      const { toolLog, runOf, tool, openReview } = await monitorUI(browser, fixture);
      await openReview();
      const gone = await tool("gone");
      await gone.button.click();
      await browser.waitUntil(async () => (await runOf("gone"))?.at(-1)?.kind === "end" && (await toolLog()).noticeError,
        { timeout: 15000, timeoutMsg: "the tool's error ends its run" });
      const rows = (await runOf("gone"))!;
      assert.equal(rows[0].text, "▶ HH:MM:SS · gone · release");
      const end = rows.at(-1)!;
      assert.match(end.text, /^■ error: \S/, `its end line says the error: ${end.text}`);
      assert.ok(end.error, "in red");
      assert.equal(signoff(fixture).status, "DEFERRED", "a tool error is not a verdict");
    },
  ),
  scenario<null>(
    "monitor-tool-failure",
    "says a tool failed and ends its run red in the Tool log",
    () => atRepo(),
    async (browser, { fixture }) => {
      const { human, toolLog, runOf, tool, openReview } = await monitorUI(browser, fixture);
      await openReview();
      const fail = await tool("fail");
      await fail.button.click();
      await browser.waitUntil(async () => (await runOf("fail"))?.at(-1)?.kind === "end" && (await toolLog()).notice.includes("fail_release failed"),
        { timeout: 15000, timeoutMsg: "the failing tool's end and notice text" });
      assert.equal((await toolLog()).noticeError, true, "the notice is presented as an error");
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

  scenario<null>(
    "monitor-tool-cancel",
    "steps back one thing per Esc from the Detail's tabs: cancels a running tool, then leaves the form, then closes the Detail",
    () => atRepo(),
    async (browser, { fixture }) => {
      const { human, runOf, tool, openReview, running } = await monitorUI(browser, fixture);
      await openReview();
      await press(browser, ["g"], () => browser.$("#field-comment").isExisting(), "the GREEN form");
      await fill(browser, "#field-comment", "kept while stepping back");
      const slow = await tool("slow");
      assert.equal(slowTool(fixture), null, "finding the tool did not start this case's cancellable tool");
      await slow.button.click();
      await browser.waitUntil(() => running("Running slow"), { timeout: 15000, timeoutMsg: "the tool running" });
      await browser.waitUntil(() => slowTool(fixture) !== null, { timeout: 15000, timeoutMsg: "the owned tool process started" });
      const alive = slowTool(fixture);
      assert.ok(alive && await alive(), "the cancellation target is an actual running process");
      // The keyboard goes to the eval's own chip, outside the review: a click on the shown tab
      // keeps it shown.
      const chip = browser.$(tabAt("e:release/signoff"));
      await chip.click();
      await browser.waitUntil(() => chip.isFocused(), { timeout: 15000, timeoutMsg: "the eval's chip focused" });
      await press(browser, ["Escape"], async () => !(await running("Running slow")), "the tool cancelled by Esc on the tabs");
      await browser.waitUntil(async () => !(await alive()), { timeout: 15000, timeoutMsg: "Esc terminated the owned tool process" });
      await browser.waitUntil(async () => (await runOf("slow"))?.at(-1)?.text === "■ cancelled", { timeout: 15000, timeoutMsg: "the run ended as cancelled" });
      assert.ok(await human().isExisting(), "Esc cancelled the job and left the Detail open");
      assert.equal(await browser.$("#field-comment").getValue(), "kept while stepping back", "the form stays open while the job is cancelled");
      for (const action of ["Close", "graph background"]) {
        const previous = (await runOf("slow"))?.[0];
        await slow.button.click();
        await browser.waitUntil(async () => await running("Running slow") && (await runOf("slow"))?.at(-1)?.kind !== "end" && await slowTool(fixture)?.() === true,
          { timeout: 15000, timeoutMsg: `a freshly owned tool is running before ${action}` });
        const process = slowTool(fixture);
        assert.ok(process && previous);
        if (action === "Close") await browser.$('aside[aria-label="Detail"]').$("button*=Close").click();
        else {
          const point = await browser.execute(() => {
            const canvas = document.querySelector('.graph [data-graph-area]')!.getBoundingClientRect();
            for (let y = canvas.bottom - 24; y > canvas.top + 24; y -= 28) for (let x = canvas.left + 24; x < canvas.right; x += 28) {
              if (document.elementFromPoint(x, y)?.classList.contains("svelte-flow__pane")) return { x: Math.round(x), y: Math.round(y) };
            }
            throw new Error("no exposed graph background");
          });
          await browser.action("pointer").move(point).down().up().perform();
        }
        await browser.waitUntil(async () => !(await process()) && (await runOf("slow"))?.at(-1)?.text === "■ cancelled" && !(await running("Running slow")),
          { timeout: 15000, timeoutMsg: `${action} cancels this tool and it really exits` });
        assert.ok(await human().isDisplayed(), `${action} does not dismiss the review`);
        assert.equal(await browser.$("#field-comment").getValue(), "kept while stepping back");
      }
      await chip.click();
      await browser.waitUntil(() => chip.isFocused(), { timeout: 15000, timeoutMsg: "the eval tab has keyboard focus after the pointer actions" });
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

  // UX journey: J-HUMAN-ESC
  scenario<null>(
    "monitor-submit-cancel",
    "cancels an in-flight submission before it saves, staying on the same review with the same draft",
    () => atRepo({ title: "A person approves the release", instruction: "Read the release notes and approve them.", gatedFingerprint: true }),
    async (browser, { fixture }) => {
      const { human, openReview, meta, running } = await monitorUI(browser, fixture);
      await openReview();
      await press(browser, ["g"], () => browser.$("#field-comment").isExisting(), "the GREEN form");
      const draft = "kept when submission is cancelled";
      await fill(browser, "#field-comment", draft);
      const gate = nativePath(fixture.root, "submit.gate");
      const record = nativePath(fixture.root, "submit.runs");
      const rounds = () => readText(record).split("\n").filter(Boolean).length;
      const before = rounds();
      const id = signoff(fixture).id;
      removeTree(gate);
      await human().$("button=Submit GREEN").click();
      await browser.waitUntil(async () => await running("Submitting GREEN") && rounds() > before,
        { timeout: 15000, timeoutMsg: "the submission reached its gated final fingerprint check" });
      const chip = browser.$(tabAt("e:release/signoff"));
      await chip.click();
      await browser.waitUntil(() => chip.isFocused(), { timeout: 15000, timeoutMsg: "the same review's Detail tab has focus" });
      await press(browser, ["Escape"], async () => !(await running("Submitting GREEN")), "Esc on the tab cancels the in-flight submission");
      // The gate is still closed: only cancellation can end its actual fingerprint process.
      await joinGatedTools(fixture);
      assert.ok((await meta()).startsWith(`Request ${id} · DEFERRED · claimed by you (alice)`), "the same claimed review stays open");
      assert.equal(await browser.$("#field-comment").getValue(), draft, "its draft stays open");
      const saved = signoff(fixture);
      assert.equal(saved.status, "DEFERRED");
      assert.equal(saved.result, null, "cancellation did not save a verdict");
      // The same draft can still be submitted normally once the external check is allowed through.
      writeText(gate, "go");
      await human().$("button=Submit GREEN").click();
      await browser.waitUntil(async () => /completed result/i.test(await human().getText()), { timeout: 15000, timeoutMsg: "the retried submission is complete" });
      assert.deepEqual(signoff(fixture).result, { verdict: "GREEN", approved: true, comment: draft });
    },
  ),

  // UX journey: J-HUMAN-RED
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
      await human().$("button=RED").click();
      await reason().waitForDisplayed({ timeout: 15000, timeoutMsg: "the RED button opened the reason form" });
      await fill(browser, "#field-reason", "no");
      await human().$("button=Submit RED").click();
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

  // UX journey: J-HUMAN-FORM
  scenario<null>(
    "monitor-red-groups",
    "enters a list of steps one text at a time and a list of findings as groups of fields, each added and removed, and submits them",
    () => atRepo(),
    async (browser, { fixture }) => {
      const { human, openReview } = await monitorUI(browser, fixture);
      await openReview();
      await press(browser, ["r"], () => browser.$("#field-reason").isExisting(), "the RED form");
      await fill(browser, "#field-reason", "two problems found");
      // A list of text: add and remove entries. The list need not keep an empty item.
      const step = (at: number) => `#field-steps textarea[aria-label="steps ${at}"]`;
      const addStep = () => human().$("#field-steps + button");
      if (!(await browser.$(step(1)).isExisting())) await addStep().click();
      await browser.$(step(1)).waitForDisplayed({ timeout: 15000, timeoutMsg: "the first editable step" });
      await fill(browser, step(1), "check the date");
      await addStep().click();
      await browser.$(step(2)).waitForExist({ timeout: 15000, timeoutMsg: "a second step added" });
      assert.equal(await browser.$(step(2)).getValue(), "", "the added step is empty");
      await fill(browser, step(2), "check the title");
      await human().$('button[aria-label="Remove steps 1"]').click();
      await browser.$(step(2)).waitForExist({ reverse: true, timeout: 15000, timeoutMsg: "the first step removed" });
      assert.equal(await browser.$(step(1)).getValue(), "check the title", "the second step moved up");
      await human().$('button[aria-label="Remove steps 1"]').click();
      await browser.waitUntil(async () => !(await browser.$(step(1)).isExisting()) || await browser.$(step(1)).getValue() === "",
        { timeout: 15000, timeoutMsg: "the final step's text was removed" });
      if (!(await browser.$(step(1)).isExisting())) await addStep().click();
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
      await human().$('[aria-label="Tools of release"]').$('button=notes').click();
      await browser.waitUntil(async () => (await runOf("notes"))?.at(-1)?.kind === "end", { timeout: 15000, timeoutMsg: "release's notes completed" });
      await human().$('[aria-label="Tools of docs"]').$('button=reference').click();
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
      await browser.waitUntil(async () => {
        const result = human().$('section[aria-label="Result"] pre');
        return await result.isDisplayed() && (await result.getText()).includes(korean)
          && /completed result/i.test(await human().getText());
      }, { timeout: 15000, timeoutMsg: "the completed Detail stays open and shows the submitted Korean text" });
    },
  ),

  // UX journeys: J-PANE J-RESIZE
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
      assert.ok(widened!.left > 0 && widened!.right === 0, `the Detail stays within the Main area, at its right edge: ${JSON.stringify(widened)}`);
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
        return dock.scrollWidth <= dock.clientWidth && heading.scrollWidth <= heading.clientWidth;
      });
      assert.ok(readable && minimum.width <= shown.width && minimum.left > 0 && minimum.right === 0, "the minimum is a readable, contained column at the right edge, not collapsed text");
      await dragEdge(Math.round(room.right - 1));
      assert.equal((await placed())?.width, minimum.width, "dragging past the minimum cannot shrink the column further");
      // Dragged as far left as the pointer goes, over the Pane: it stops short of the Pane.
      await dragEdge(Math.round(room.left - 40));
      const maximum = (await placed())!;
      assert.ok(maximum.width > minimum.width && maximum.width < room.width && maximum.left > 0 && maximum.right === 0, "the widest Detail stays in the Main area, never over the Pane");
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
      // with Settings open over it, then the Run again.
      await selectBranch(browser, "empty-repo  master");
      await browser.waitUntil(async () => (await mainTitle(browser)) === "Current · empty-repo  master", { timeout: 15000, timeoutMsg: "the other branch's Current shown" });
      assert.equal(await main(), split, "the split stays as dragged on a Current");
      await browser.$("button*=Settings").click();
      await browser.waitUntil(() => browser.$("#settings-title").isExisting(), { timeout: 15000, timeoutMsg: "Settings open" });
      assert.equal(await mainTitle(browser), "Current · empty-repo  master", "Settings opens over the Current");
      assert.equal(await main(), split, "the split stays as dragged under Settings");
      await press(browser, ["Escape"], async () => !(await browser.$("#settings-title").isExisting()), "Settings closed");
      assert.equal(await main(), split, "the split stays as dragged after Settings");
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

  // UX journey: J-DETAIL-TOOLS
  scenario<null>(
    "monitor-detail-tools",
    "runs an Artifact's own tools from its Detail at once, unclaimed and recorded as no review's evidence: output, a launch and a failure in its Tool log",
    () => atRepo(),
    async (browser, { fixture }) => {
      const DETAIL = 'aside[aria-label="Detail"]';
      await monitorUI(browser, fixture);
      await openArtifact(browser, "a:release", "Artifact release");
      const tools = () => browser.$(DETAIL).$('section[aria-label="Tools of release"]');
      await tools().$("button=notes").waitForEnabled({ timeout: 15000, timeoutMsg: "release's own tools listed" });
      assert.equal(await browser.$(DETAIL).$('[role="log"][aria-label="Tool log"]').isExisting(), false, "no Tool log before a first run");
      assert.equal(await browser.$(".graph .svelte-flow__node button").isExisting(), false, "the graph's nodes carry no tool buttons");
      const toolRunsBefore = (await checkpoint(browser)).activity.commands.artifactToolRun?.completed ?? 0;
      // An output tool runs at once, unclaimed: its start, each line it printed and its exit.
      await tools().$("button=notes").click();
      await browser.waitUntil(async () => (await runIn(browser, DETAIL, "notes"))?.at(-1)?.kind === "end", { timeout: 15000, timeoutMsg: "the notes run ended" });
      // Below its evals: the Tools, then their Tool log from its first run, then its path.
      const order = await browser.execute((root) => {
        const dock = document.querySelector(root)!;
        const parts = [dock.querySelector('[aria-label^="Evals of "]'), dock.querySelector('section[aria-label="Tools of release"]'),
          dock.querySelector('[role="log"][aria-label="Tool log"]'), dock.querySelector('section[aria-label="Path"] p.line')];
        return parts.every(Boolean) && parts.every((part, at) => at === 0 || (parts[at - 1]!.compareDocumentPosition(part!) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0);
      }, DETAIL);
      assert.ok(order, "its evals, then its Tools and their Tool log, then its path");
      assert.deepEqual(await runIn(browser, DETAIL, "notes"), [
        { text: "▶ HH:MM:SS · notes · release", kind: "start", error: false },
        { text: "# Release notes", kind: "line", error: false },
        { text: "- Ready to ship.", kind: "line", error: false },
        { text: "■ exit 0", kind: "end", error: false },
      ]);
      assert.equal((await logIn(browser, DETAIL)).noticeError, false);
      assert.ok(((await checkpoint(browser)).activity.commands.artifactToolRun?.completed ?? 0) > toolRunsBefore, "the same probe observes this tool's run");
      assert.equal(signoff(fixture).claim, null, "running a tool claimed nothing");
      // The Tool log folded after a run unfolds again for the next one.
      await browser.$(DETAIL).$("button.head=Tool log").click();
      await browser.waitUntil(async () => !(await browser.$(DETAIL).$('[role="log"][aria-label="Tool log"]').isExisting()), { timeout: 15000, timeoutMsg: "the Tool log folded" });
      // A launch tool answers at once and says so; its application runs and its exit ends the run.
      assert.equal(launched(fixture), false);
      await tools().$("button=open").click();
      await browser.waitUntil(async () => (await runIn(browser, DETAIL, "open")) !== null && await tools().$("button=notes").isEnabled(), { timeout: 15000, timeoutMsg: "the launch returned" });
      assert.equal(await browser.$(DETAIL).$("button.head=Tool log").getAttribute("aria-expanded"), "true", "the run unfolded the Tool log again");
      assert.equal((await logIn(browser, DETAIL)).notice, "", "what ended well is said by the log alone");
      await browser.waitUntil(() => launched(fixture), { timeout: 15000, timeoutMsg: "the launched application ran" });
      await browser.waitUntil(async () => (await runIn(browser, DETAIL, "open"))?.at(-1)?.text === "■ exit 0", { timeout: 15000, timeoutMsg: "its exit logged" });
      // A launch tool whose application keeps running still returns at once: the launch is said,
      // the Detail's tools are usable again and its run stays open in the Tool log.
      await tools().$("button=talk").click();
      await browser.waitUntil(async () => (await runIn(browser, DETAIL, "talk"))?.some((row) => row.text === "opened the stand-in") === true &&
        await tools().$("button=notes").isEnabled(), { timeout: 15000, timeoutMsg: "the launch returned while its application runs" });
      assert.ok(!(await runIn(browser, DETAIL, "talk"))!.some((row) => row.kind === "end"), "the launched application has not ended");
      // Let it go on: what it prints next joins its run, and its exit ends it.
      finishTalk(fixture);
      await browser.waitUntil(async () => (await runIn(browser, DETAIL, "talk"))?.at(-1)?.text === "■ exit 0", { timeout: 15000, timeoutMsg: "the stand-in's exit logged" });
      assert.ok((await runIn(browser, DETAIL, "talk"))!.some((row) => row.text === "chose: B"), "its later lines joined its own run");
      // A failing tool says so, its run ended red.
      await tools().$("button=fail").click();
      await browser.waitUntil(async () => (await runIn(browser, DETAIL, "fail"))?.at(-1)?.kind === "end" && (await logIn(browser, DETAIL)).noticeError,
        { timeout: 15000, timeoutMsg: "the failing tool's end and error" });
      assert.deepEqual(await runIn(browser, DETAIL, "fail"), [
        { text: "▶ HH:MM:SS · fail · release", kind: "start", error: false },
        { text: "■ exit 1", kind: "end", error: true },
      ]);
      // Every earlier run stays in the Tool log after the later ones, and after the Detail is
      // closed and opened again while the app runs.
      const ends = async () => Promise.all(["notes", "open", "talk", "fail"].map(async (tool) => (await runIn(browser, DETAIL, tool))?.at(-1)?.text ?? null));
      const kept = ["■ exit 0", "■ exit 0", "■ exit 0", "■ exit 1"];
      assert.deepEqual(await ends(), kept, "every run kept after the later ones");
      await browser.$(DETAIL).$("button*=Close").click();
      await browser.waitUntil(async () => (await detail(browser)) === null, { timeout: 15000, timeoutMsg: "release's Detail closed" });
      await openArtifact(browser, "a:release", "Artifact release");
      await browser.waitUntil(async () => JSON.stringify(await ends()) === JSON.stringify(kept),
        { timeout: 15000, timeoutMsg: "every run kept after the Detail was opened again" });
      assert.deepEqual((await runIn(browser, DETAIL, "notes"))?.map((row) => row.text), ["▶ HH:MM:SS · notes · release", "# Release notes", "- Ready to ship.", "■ exit 0"],
        "the first run whole");
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

  // UX journey: J-DETAIL-TOOLS
  scenario<{ run: string }>(
    "monitor-detail-tools-now",
    "runs a shown Run's Artifact tools as the branch declares them now, in its worktree as it is now, not as the Run saw it",
    async () => {
      const fixture = await create();
      const run = signoff(fixture).runId;
      // After the Run was saved: release declares one more tool, and its notes change.
      const declaration = nativePath(fixture.repo, ".artifactize", "release.artf");
      writeText(declaration, readText(declaration).replace("[views.human_tools]\n",
        '[views.human_tools]\nlatest = { description = "Print the notes as they are now.", builtin = "read", args = ["{artifactPath}/notes.md"] }\n'));
      writeText(nativePath(fixture.repo, "release", "notes.md"), "# Release notes\n- Changed after the Run.\n");
      return { fixture, args: ["--repo", fixture.repo], data: { run } };
    },
    async (browser, { data: { run } }) => {
      const DETAIL = 'aside[aria-label="Detail"]';
      await browser.waitUntil(async () => (await mainTitle(browser)) === run, { timeout: 15000, timeoutMsg: "the Run saved before the change shown" });
      await graphShown(browser, "a:release");
      await openArtifact(browser, "a:release", "Artifact release");
      const tools = () => browser.$(DETAIL).$('section[aria-label="Tools of release"]');
      // The tool declared only after the Run was saved is listed and runs.
      await tools().$("button=latest").waitForEnabled({ timeout: 15000, timeoutMsg: "the tool declared now listed" });
      await tools().$("button=latest").click();
      await browser.waitUntil(async () => (await runIn(browser, DETAIL, "latest"))?.at(-1)?.kind === "end", { timeout: 15000, timeoutMsg: "the latest run ended" });
      const latest = (await runIn(browser, DETAIL, "latest"))!;
      assert.deepEqual([latest[0], latest.at(-1)], [
        { text: "▶ HH:MM:SS · latest · release", kind: "start", error: false },
        { text: "■ done", kind: "end", error: false },
      ]);
      assert.ok(latest.some((row) => row.kind === "line" && row.text.includes("- Changed after the Run.")), `it read today's notes: ${JSON.stringify(latest)}`);
      // A command tool runs in the worktree as it is now: it prints the changed notes.
      await tools().$("button=notes").click();
      await browser.waitUntil(async () => (await runIn(browser, DETAIL, "notes"))?.at(-1)?.kind === "end", { timeout: 15000, timeoutMsg: "the notes run ended" });
      assert.deepEqual(await runIn(browser, DETAIL, "notes"), [
        { text: "▶ HH:MM:SS · notes · release", kind: "start", error: false },
        { text: "# Release notes", kind: "line", error: false },
        { text: "- Changed after the Run.", kind: "line", error: false },
        { text: "■ exit 0", kind: "end", error: false },
      ]);
    },
  ),

  // UX journey: J-QUIT
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
