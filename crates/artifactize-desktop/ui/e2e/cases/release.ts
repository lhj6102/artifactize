// Newer releases through the real app: a loopback stand-in for the repository's site answers
// the app's release lookup and records what the system browser fetches. The app finds its
// install channel from where it runs; a case places the candidate where a channel puts it.
// The app keeps the site's answer for a day. The fixture advances its external clock.
import assert from "node:assert/strict";
import { readCandidate } from "../build.ts";
import { app, cli, create, createOutside, environment, type Fixture } from "../fixture.ts";
import { checkpoint } from "../commands.ts";
import { dispatchInput } from "../input.ts";
import { afterRender, press } from "../keys.ts";
import { mainTitle } from "../pane.ts";
import { scenario, type Scenario } from "../scenario.ts";
import {
  appendText, commandDirectory, copyFile, diskImageAppDirectory, executableName, nativeInput, nativePath, placeApp,
  primaryModifier, readText, replaceText, runProgramOk, serveLoopbackHttp, storePackageDirectory, writeText,
  type Environment, type HttpServer, type NativePath,
} from "../platform.ts";

const LATEST = "9.9.9";
const LOOKUP = "/api/releases/latest";
const PAGE = "/releases/latest";
const DISK_IMAGE = "/releases/latest/download/Artifactize.dmg";
const DAY_SECONDS = 24 * 60 * 60;
const CARGO_CONTROL = ["install", "artifactize", "artifactize-tools", "--locked"];

type Site = { url: string; visits: NativePath; answer: NativePath; clock: NativePath; afterDay: number };
const cargoLog = (fixture: Fixture) => nativePath(fixture.root, "cargo-launches.jsonl");

/** The stand-in site: it answers the lookup with the tag in `answer`, which a case may replace
 * while the app runs, serves the release page and file, and records the path of every GET. */
async function site(fixture: Fixture, tag: string): Promise<{ data: Site; server: HttpServer }> {
  const visits = nativePath(fixture.root, "site-get-paths.jsonl");
  const answer = nativePath(fixture.root, "site-latest-tag.txt");
  const clock = nativePath(fixture.root, "release-clock-seconds");
  // Both the app and doctor read this same fixture clock.
  const afterDay = 1_800_000_000;
  writeText(clock, String(afterDay - DAY_SECONDS));
  writeText(visits, "");
  writeText(answer, tag);
  const server = await serveLoopbackHttp((request, response) => {
    request.resume();
    if (request.method === "GET") appendText(visits, `${JSON.stringify(request.url)}\n`);
    const lookup = request.url === LOOKUP;
    const known = lookup || request.url === PAGE || request.url === DISK_IMAGE;
    response.writeHead(known ? 200 : 404, { "content-type": lookup ? "application/json" : "text/html" });
    response.end(lookup ? JSON.stringify({ tag_name: readText(answer) }) : "");
  });
  return { data: { url: server.url, visits, answer, clock, afterDay }, server };
}

const visited = (data: Site): string[] => readText(data.visits).split("\n").filter(Boolean).map((line) => JSON.parse(line) as string);

/** Nothing but release lookups reached the site: the app downloaded nothing by itself. */
function onlyLookups(data: Site): void {
  const fetched = visited(data).filter((path) => path !== LOOKUP);
  assert.deepEqual(fetched, [], "the app fetched nothing but release lookups by itself");
}

/** The version the candidate build reports. */
function running(): string {
  return runProgramOk(cli(), ["--version"]).trim().split(" ")[1];
}

/** The stable release of the candidate's own version, newer only for a prerelease build. */
function baseline() {
  const [version, prerelease] = running().split("+")[0].split("-", 2);
  return { tag: `v${version}`, notice: prerelease ? `${version} available` : null };
}

/** The app's release lookup has answered at least once, none of its calls failed, and the
 * window shows its answer. */
async function lookedUp(browser: WebdriverIO.Browser): Promise<void> {
  const calls = async () => (await checkpoint(browser)).activity.commands.releaseNotice;
  await browser.waitUntil(async () => ((await calls())?.completed ?? 0) >= 1,
    { timeout: 15000, timeoutMsg: "the app looked up the latest release" });
  assert.equal((await calls())?.failed, 0, "the app's release lookup succeeded");
  await afterRender(browser);
}

const notice = (browser: WebdriverIO.Browser) => browser.$(`button=${LATEST} available`);
const anyNotice = (browser: WebdriverIO.Browser) => browser.$("header.bar").$("button*= available");
const dialog = (browser: WebdriverIO.Browser) => browser.$('[role="dialog"]');
const shown = (browser: WebdriverIO.Browser) => async () =>
  (await dialog(browser).isExisting()) && (await dialog(browser).getText()).includes("Update artifactize");
const closed = (browser: WebdriverIO.Browser) => async () => !(await dialog(browser).isExisting());

async function openDialog(browser: WebdriverIO.Browser): Promise<string> {
  await notice(browser).click();
  await browser.waitUntil(shown(browser), { timeout: 15000, timeoutMsg: "the notice opens its dialog" });
  const text = await dialog(browser).getText();
  // The two versions as numbers: the running one, then the newer one.
  assert.equal(await dialog(browser).$(".versions").getText(), `${running()} → ${LATEST}`, `the dialog names both versions: ${text}`);
  return text;
}

/** A click on the dialog's link to `path` opens it in the system browser; the dialog stays. */
async function follow(browser: WebdriverIO.Browser, data: Site, path: string): Promise<void> {
  await dialog(browser).$(`button=${data.url}${path}`).click();
  await browser.waitUntil(() => visited(data).includes(path), {
    timeout: 15000,
    timeoutMsg: `the system browser reaches ${path}, which the dialog links`,
  });
  assert.equal(await shown(browser)(), true, "the dialog stays open after its link opens");
}

async function closeByButton(browser: WebdriverIO.Browser): Promise<void> {
  await dialog(browser).$("button*=Close").click();
  await browser.waitUntil(closed(browser), { timeout: 15000, timeoutMsg: "the dialog closes by Close" });
  assert.equal(await notice(browser).isExisting(), true, "the notice stays in the header after Close");
}

/** A release case's fixture: the stand-in site, the app placed by `place` when given, and
 * the system browser for `browser`. */
async function prepare(tag: string, options: {
  place?: (fixture: Fixture) => { app: NativePath; env?: Environment };
  browser?: string;
  /** A fixture whose state the CLI already made, as on a machine that ran artifactize before. */
  used?: boolean;
} = {}) {
  const fixture = options.used ? await create() : createOutside();
  const placed = options.place?.(fixture);
  // Everything that can fail comes before the site starts, so its server is always released.
  const env = { ...environment(fixture), ...placed?.env };
  const { data, server } = await site(fixture, tag);
  return {
    fixture,
    env: { ...env, ARTIFACTIZE_RELEASES_URL: data.url, ARTIFACTIZE_E2E_RELEASE_CLOCK: data.clock },
    data,
    ...(placed ? { app: placed.app } : {}),
    ...(options.browser ? { browser: `${data.url}${options.browser}` } : {}),
    release: () => server.close(),
  };
}

export const scenarios: Scenario[] = [
  // UX journey: J-UPDATE
  scenario<Site>(
    "release-notice",
    "a release found later while the app runs shows in the header, as doctor reports it; its dialog links the release page, which opens in the browser, and closes by Esc or Close",
    () => prepare(baseline().tag, { browser: PAGE, used: true }),
    async (browser, { fixture, env, data }) => {
      const lookups = () => visited(data).filter((path) => path === LOOKUP).length;
      const asked = async () => (await checkpoint(browser)).activity.commands.releaseNotice?.completed ?? 0;
      const shownNotice = async () => await anyNotice(browser).isExisting() ? await anyNotice(browser).getText() : null;
      const initialNotice = baseline().notice;
      // A released candidate starts without a notice; a dev candidate sees the matching stable release.
      await lookedUp(browser);
      assert.equal(await shownNotice(), initialNotice, "the header reflects the baseline stable release");
      assert.equal(lookups(), 1, "the app asked the site at start");
      const view = await mainTitle(browser);
      // A release comes out. The app looks again by itself, but within the day it reuses its
      // answer and does not ask the site.
      replaceText(data.answer, `v${LATEST}`);
      const before = await asked();
      await browser.waitUntil(async () => (await asked()) >= before + 2, { timeout: 15000, timeoutMsg: "the app looked again twice" });
      await afterRender(browser);
      assert.equal(await shownNotice(), initialNotice, "within the day the kept answer stands");
      assert.equal(lookups(), 1, "within the day the app asked the site nothing more");
      // A day later, the next look asks the site again and finds the release.
      replaceText(data.clock, String(data.afterDay));
      await notice(browser).waitForExist({ timeout: 15000, timeoutMsg: "a day later the app's lookup puts the notice in the header" });
      assert.equal(lookups(), 2, "a day later the app asked the site once more");
      assert.equal(await mainTitle(browser), view, "the notice leaves the shown view as it was");
      const header = await browser.execute(() =>
        [...document.querySelectorAll("header.bar button")].map((button) => button.textContent?.trim() ?? ""));
      assert.ok(header.indexOf(`${LATEST} available`) < header.indexOf("Settings"), `the notice comes before Settings: ${header.join(" | ")}`);
      // doctor makes the same lookup and finds the same release, with the same update.
      const report = JSON.parse(runProgramOk(cli(), ["--state-dir", fixture.state, "doctor", "--json"], { env })) as {
        checks: { name: string; status: string; details: { latest?: string; update?: { kind: string; url?: string } } }[];
      };
      const release = report.checks.find((check) => check.name === "release");
      assert.equal(release?.status, "WARN", JSON.stringify(release));
      assert.equal(release?.details.latest, LATEST);
      assert.deepEqual(release?.details.update, { kind: "page", url: `${data.url}${PAGE}` });
      assert.equal(lookups(), 2, "doctor reused the app's answer of the day");

      await openDialog(browser);
      // How to update, as the link to the release page alone.
      assert.equal(await dialog(browser).$(`button=${data.url}${PAGE}`).isExisting(), true, "the release page linked");
      assert.equal(await dialog(browser).$("pre.command").isExisting(), false, "nothing to run");
      onlyLookups(data);
      await follow(browser, data, PAGE);
      await press(browser, ["Escape"], closed(browser), "the dialog closes by Esc");
      assert.equal(await notice(browser).isExisting(), true, "the notice stays in the header after Esc");

      await openDialog(browser);
      await closeByButton(browser);
    },
  ),
  // UX journey: J-UPDATE
  scenario<Site>(
    "release-command",
    "an app installed with Cargo shows Cargo's update command, as text that can be selected, and runs nothing itself",
    () => prepare(`v${LATEST}`, {
      place: (fixture) => {
        const cargo = nativePath(fixture.root, "cargo");
        const bin = nativePath(cargo, "bin");
        const placed = placeApp(app(), bin);
        copyFile(readCandidate().observer, nativePath(bin, executableName("cargo")));
        const env = commandDirectory({ ...environment(fixture), CARGO_HOME: cargo, ARTIFACTIZE_E2E_CARGO_LOG: cargoLog(fixture) }, bin);
        // Positive control: the same command on the app's private PATH is recorded.
        runProgramOk("cargo", CARGO_CONTROL, { env });
        assert.equal(readText(cargoLog(fixture)).trim(), JSON.stringify(CARGO_CONTROL));
        return { app: placed, env };
      },
    }),
    async (browser, { data, fixture }) => {
      await notice(browser).waitForExist({ timeout: 15000, timeoutMsg: "the lookup at start puts the notice in the header" });
      await openDialog(browser);
      const command = dialog(browser).$("pre.command");
      const wanted = "cargo install artifactize artifactize-tools --locked";
      assert.equal(await command.getText(), wanted);
      const [start, end] = await browser.execute((element: HTMLElement) => {
        const range = document.createRange();
        range.selectNodeContents(element);
        const rows = range.getClientRects();
        const first = rows[0], last = rows[rows.length - 1];
        return [{ x: first.left + 1, y: first.top + first.height / 2 }, { x: last.right - 1, y: last.top + last.height / 2 }];
      }, await command.getElement() as unknown as HTMLElement);
      await dispatchInput(browser, { kind: "text-click", ...start });
      await dispatchInput(browser, { kind: "text-click", ...end, shift: true });
      assert.equal(await browser.execute(() => getSelection()?.toString() ?? ""), wanted, "only the update command is selected");
      await nativeInput(browser, { type: "copy" });
      await closeByButton(browser);
      const field = browser.$('input[aria-label="Request ID"]');
      await press(browser, [primaryModifier, "o"], () => field.isDisplayed(), "a field to paste the command into");
      await field.click();
      await nativeInput(browser, { type: "paste" });
      await browser.waitUntil(async () => await field.getValue() === wanted,
        { timeout: 15000, timeoutMsg: "the update command pasted exactly" });
      await press(browser, ["Escape"], async () => !(await field.isDisplayed()), "the paste field closed");
      assert.equal(readText(cargoLog(fixture)).trim(), JSON.stringify(CARGO_CONTROL), "only the fixture's positive control launched Cargo");
      onlyLookups(data);
    },
  ),
  // UX journey: J-UPDATE
  scenario<Site>(
    "release-download",
    "an app installed from the disk image links the newest disk image to download and install by hand, and downloads nothing itself",
    () => prepare(`v${LATEST}`, {
      place: (fixture) => ({ app: placeApp(app(), diskImageAppDirectory(fixture.root)) }),
      browser: DISK_IMAGE,
    }),
    async (browser, { data }) => {
      await notice(browser).waitForExist({ timeout: 15000, timeoutMsg: "the lookup at start puts the notice in the header" });
      await openDialog(browser);
      assert.equal(await dialog(browser).$("pre.command").isExisting(), false, "a disk image has no command to run");
      onlyLookups(data);
      await follow(browser, data, DISK_IMAGE);
      await closeByButton(browser);
    },
  ),
  // UX journey: J-UPDATE
  scenario<Site>(
    "release-prerelease",
    "a newer prerelease is not shown after a fresh lookup replaces the previous stable-release notice",
    () => prepare(`v${LATEST}`, { used: true }),
    async (browser, { data }) => {
      await notice(browser).waitForDisplayed();
      const before = visited(data).filter((path) => path === LOOKUP).length;
      replaceText(data.answer, `v${LATEST}-rc.1`);
      replaceText(data.clock, String(data.afterDay));
      await browser.waitUntil(() => visited(data).filter((path) => path === LOOKUP).length > before,
        { timeout: 15000, timeoutMsg: "the site served the fresh prerelease lookup" });
      await anyNotice(browser).waitForDisplayed({ reverse: true, timeout: 15000, timeoutMsg: "the applied response cleared the prior notice without showing a prerelease" });
      onlyLookups(data);
    },
  ),
  // UX journey: J-UPDATE
  scenario<Site>(
    "release-store",
    "a Microsoft Store install shows no notice, whatever release is out",
    () => prepare(`v${LATEST}`, { place: (fixture) => ({ app: placeApp(app(), storePackageDirectory(fixture.root)) }) }),
    async (browser) => {
      await lookedUp(browser);
      assert.equal(await anyNotice(browser).isExisting(), false, "the Store updates the app: no notice");
    },
  ),
];
