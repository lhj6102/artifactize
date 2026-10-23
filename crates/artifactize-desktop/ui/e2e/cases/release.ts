// Newer releases through the real app: a loopback stand-in for the repository's site answers
// the app's release lookup and records what the system browser fetches. The app finds its
// install channel from where it runs; a case places the candidate where a channel puts it.
// The app keeps the site's answer in its state for a day; a case moves that answer's time back
// a day to stand for a day passing.
import assert from "node:assert/strict";
import { app, cli, create, createOutside, environment, type Fixture } from "../fixture.ts";
import { checkpoint } from "../commands.ts";
import { dispatchInput } from "../input.ts";
import { afterRender, press } from "../keys.ts";
import { mainTitle } from "../pane.ts";
import { visibleCenter } from "../pointer.ts";
import { scenario, type Scenario } from "../scenario.ts";
import {
  appendText, diskImageAppDirectory, nativePath, placeApp, primaryModifier, readText, replaceText, runProgramOk,
  serveLoopbackHttp, storePackageDirectory, writeText,
  type Environment, type HttpServer, type NativePath,
} from "../platform.ts";

const LATEST = "9.9.9";
const LOOKUP = "/api/releases/latest";
const PAGE = "/releases/latest";
const DISK_IMAGE = "/releases/latest/download/Artifactize.dmg";
/** The answer the app keeps in its state, and a day in its seconds. */
const KEPT = "release-check.json";
const DAY_SECONDS = 24 * 60 * 60;

type Site = { url: string; visits: NativePath; answer: NativePath };

/** The stand-in site: it answers the lookup with the tag in `answer`, which a case may replace
 * while the app runs, serves the release page and file, and records the path of every GET. */
async function site(fixture: Fixture, tag: string): Promise<{ data: Site; server: HttpServer }> {
  const visits = nativePath(fixture.root, "site-get-paths.jsonl");
  const answer = nativePath(fixture.root, "site-latest-tag.txt");
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
  return { data: { url: server.url, visits, answer }, server };
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

/** The app's release lookup has answered at least once and the window shows its answer. */
async function lookedUp(browser: WebdriverIO.Browser): Promise<void> {
  await browser.waitUntil(async () => ((await checkpoint(browser)).activity.commands.releaseNotice?.completed ?? 0) >= 1,
    { timeout: 15000, timeoutMsg: "the app looked up the latest release" });
  await afterRender(browser);
}

const notice = (browser: WebdriverIO.Browser) => browser.$(`button=${LATEST} available`);
const dialog = (browser: WebdriverIO.Browser) => browser.$('[role="dialog"]');
const shown = (browser: WebdriverIO.Browser) => async () =>
  (await dialog(browser).isExisting()) && (await dialog(browser).getText()).includes(`artifactize ${LATEST} is available`);
const closed = (browser: WebdriverIO.Browser) => async () => !(await dialog(browser).isExisting());

async function openDialog(browser: WebdriverIO.Browser): Promise<string> {
  await notice(browser).click();
  await browser.waitUntil(shown(browser), { timeout: 15000, timeoutMsg: "the notice opens its dialog" });
  const text = await dialog(browser).getText();
  assert.ok(text.includes(`This app is ${running()}.`), `the dialog names the running version: ${text}`);
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
  await dialog(browser).$("button=Close").click();
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
  const { data, server } = await site(fixture, tag);
  return {
    fixture,
    env: { ...environment(fixture), ...placed?.env, ARTIFACTIZE_RELEASES_URL: data.url },
    data,
    ...(placed ? { app: placed.app } : {}),
    ...(options.browser ? { browser: `${data.url}${options.browser}` } : {}),
    release: () => server.close(),
  };
}

export const scenarios: Scenario[] = [
  // UX journey: J-UPDATE; UX edges: E-UPDATE-NOTICE E-UPDATE-OPEN E-UPDATE-LINK E-UPDATE-CLOSE
  scenario<Site>(
    "release-notice",
    "a release found later while the app runs shows in the header, as doctor reports it; its dialog links the release page, which opens in the browser, and closes by Esc or Close",
    () => prepare(`v${running()}`, { browser: PAGE, used: true }),
    async (browser, { fixture, env, data }) => {
      const lookups = () => visited(data).filter((path) => path === LOOKUP).length;
      const asked = async () => (await checkpoint(browser)).activity.commands.releaseNotice?.completed ?? 0;
      // At start the app asks the site once. The running version is the latest: nothing shows.
      await lookedUp(browser);
      assert.equal(await notice(browser).isExisting(), false, "no notice while this app is the latest release");
      assert.equal(lookups(), 1, "the app asked the site at start");
      const view = await mainTitle(browser);
      // A release comes out. The app looks again by itself, but within the day it reuses its
      // answer and does not ask the site.
      replaceText(data.answer, `v${LATEST}`);
      const before = await asked();
      await browser.waitUntil(async () => (await asked()) >= before + 2, { timeout: 15000, timeoutMsg: "the app looked again twice" });
      await afterRender(browser);
      assert.equal(await notice(browser).isExisting(), false, "within the day the kept answer stands");
      assert.equal(lookups(), 1, "within the day the app asked the site nothing more");
      // A day later, the next look asks the site again and finds the release.
      const kept = nativePath(fixture.state, KEPT);
      const answer = JSON.parse(readText(kept)) as { checkedAt: number };
      replaceText(kept, JSON.stringify({ ...answer, checkedAt: answer.checkedAt - DAY_SECONDS }));
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

      const text = await openDialog(browser);
      assert.ok(text.includes("See the release page"), text);
      onlyLookups(data);
      await follow(browser, data, PAGE);
      await press(browser, ["Escape"], closed(browser), "the dialog closes by Esc");
      assert.equal(await notice(browser).isExisting(), true, "the notice stays in the header after Esc");

      await openDialog(browser);
      await closeByButton(browser);
    },
  ),
  // UX journey: J-UPDATE; UX edges: E-UPDATE-NOTICE E-UPDATE-OPEN E-UPDATE-CLOSE
  scenario<Site>(
    "release-command",
    "an app installed with Cargo shows Cargo's update command, as text that can be selected, and runs nothing itself",
    () => prepare(`v${LATEST}`, {
      place: (fixture) => {
        const cargo = nativePath(fixture.root, "cargo");
        return { app: placeApp(app(), nativePath(cargo, "bin")), env: { CARGO_HOME: cargo } };
      },
    }),
    async (browser, { data }) => {
      await notice(browser).waitForExist({ timeout: 15000, timeoutMsg: "the lookup at start puts the notice in the header" });
      const text = await openDialog(browser);
      assert.ok(text.includes("To update, run:"), text);
      const command = dialog(browser).$("pre.command");
      assert.equal(await command.getText(), "cargo install artifactize artifactize-tools --locked");
      // Select All is real keyboard input after a click in the command; the selection is
      // only observed, never installed by page script.
      await dispatchInput(browser, { kind: "text-click", ...await visibleCenter(browser, await command.getElement()) });
      await browser.keys([primaryModifier, "a"]);
      const selected = await browser.execute(() => getSelection()?.toString() ?? "");
      assert.ok(selected.includes("cargo install artifactize artifactize-tools --locked"), `the command is selectable text: ${selected}`);
      await closeByButton(browser);
      onlyLookups(data);
    },
  ),
  // UX journey: J-UPDATE; UX edges: E-UPDATE-NOTICE E-UPDATE-OPEN E-UPDATE-LINK E-UPDATE-CLOSE
  scenario<Site>(
    "release-download",
    "an app installed from the disk image links the newest disk image to download and install by hand, and downloads nothing itself",
    () => prepare(`v${LATEST}`, {
      place: (fixture) => ({ app: placeApp(app(), diskImageAppDirectory(fixture.root)) }),
      browser: DISK_IMAGE,
    }),
    async (browser, { data }) => {
      await notice(browser).waitForExist({ timeout: 15000, timeoutMsg: "the lookup at start puts the notice in the header" });
      const text = await openDialog(browser);
      assert.ok(text.includes("download and install it"), text);
      assert.equal(await dialog(browser).$("pre.command").isExisting(), false, "a disk image has no command to run");
      onlyLookups(data);
      await follow(browser, data, DISK_IMAGE);
      await closeByButton(browser);
    },
  ),
  // UX journey: J-UPDATE; UX edges: E-UPDATE-NOTICE
  scenario<Site>(
    "release-store",
    "a Microsoft Store install shows no notice, whatever release is out",
    () => prepare(`v${LATEST}`, { place: (fixture) => ({ app: placeApp(app(), storePackageDirectory(fixture.root)) }) }),
    async (browser) => {
      await lookedUp(browser);
      assert.equal(await notice(browser).isExisting(), false, "the Store updates the app: no notice");
    },
  ),
];
