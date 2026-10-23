// Newer releases through the real app: a loopback stand-in for the repository's site answers
// the app's release lookup and records what the system browser fetches. The app finds its
// install channel from where it runs; a case places the candidate where a channel puts it.
import assert from "node:assert/strict";
import { app, cli, createOutside, environment, type Fixture } from "../fixture.ts";
import { checkpoint } from "../commands.ts";
import { dispatchInput } from "../input.ts";
import { afterRender, press } from "../keys.ts";
import { mainTitle } from "../pane.ts";
import { visibleCenter } from "../pointer.ts";
import { scenario, type Scenario } from "../scenario.ts";
import {
  appendText, nativePath, placeApp, primaryModifier, readText, replaceText, runProgramOk, serveLoopbackHttp, storePackageDirectory,
  writeText,
  type HttpServer, type NativePath,
} from "../platform.ts";

const LATEST = "9.9.9";

type Site = { url: string; visits: NativePath; answer: NativePath };

/** The stand-in site: it answers the lookup with the tag in `answer`, which a case may replace
 * while the app runs, and records the path of every GET. */
async function site(fixture: Fixture, tag: string): Promise<{ data: Site; server: HttpServer }> {
  const visits = nativePath(fixture.root, "site-get-paths.jsonl");
  const answer = nativePath(fixture.root, "site-latest-tag.txt");
  writeText(visits, "");
  writeText(answer, tag);
  const server = await serveLoopbackHttp((request, response) => {
    request.resume();
    if (request.method === "GET") appendText(visits, `${JSON.stringify(request.url)}\n`);
    const lookup = request.url === "/api/releases/latest";
    response.writeHead(lookup || request.url === "/releases/latest" ? 200 : 404, { "content-type": lookup ? "application/json" : "text/html" });
    response.end(lookup ? JSON.stringify({ tag_name: readText(answer) }) : "");
  });
  return { data: { url: server.url, visits, answer }, server };
}

const visited = (data: Site): string[] => readText(data.visits).split("\n").filter(Boolean).map((line) => JSON.parse(line) as string);

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

export const scenarios: Scenario[] = [
  // UX journey: J-UPDATE; UX edges: E-UPDATE-NOTICE E-UPDATE-OPEN E-UPDATE-LINK E-UPDATE-CLOSE
  scenario<Site>(
    "release-notice",
    "a release found later while the app runs shows in the header; its dialog links the release page, which opens in the browser, and closes by Esc or Close",
    async () => {
      const fixture = createOutside();
      const { data, server } = await site(fixture, `v${running()}`);
      const page = `${data.url}/releases/latest`;
      return {
        fixture,
        env: { ...environment(fixture), ARTIFACTIZE_RELEASES_URL: data.url },
        data,
        browser: page,
        release: () => server.close(),
      };
    },
    async (browser, { data }) => {
      // The running version is the latest: nothing shows.
      await lookedUp(browser);
      assert.equal(await notice(browser).isExisting(), false, "no notice while this app is the latest release");
      const view = await mainTitle(browser);
      // A release comes out while the app runs; a later lookup finds it.
      replaceText(data.answer, `v${LATEST}`);
      await notice(browser).waitForExist({ timeout: 15000, timeoutMsg: "a later lookup puts the notice in the header" });
      assert.equal(await mainTitle(browser), view, "the notice leaves the shown view as it was");
      const header = await browser.execute(() =>
        [...document.querySelectorAll("header.bar button")].map((button) => button.textContent?.trim() ?? ""));
      assert.ok(header.indexOf(`${LATEST} available`) < header.indexOf("Settings"), `the notice comes before Settings: ${header.join(" | ")}`);

      const text = await openDialog(browser);
      assert.ok(text.includes("See the release page"), text);
      const page = `${data.url}/releases/latest`;
      await dialog(browser).$(`button=${page}`).click();
      await browser.waitUntil(() => visited(data).includes("/releases/latest"), {
        timeout: 15000,
        timeoutMsg: "the system browser reaches the release page the dialog links",
      });
      assert.equal(await shown(browser)(), true, "the dialog stays open after its link opens");
      await press(browser, ["Escape"], closed(browser), "the dialog closes by Esc");
      assert.equal(await notice(browser).isExisting(), true, "the notice stays in the header after Esc");

      await openDialog(browser);
      await dialog(browser).$("button=Close").click();
      await browser.waitUntil(closed(browser), { timeout: 15000, timeoutMsg: "the dialog closes by Close" });
      assert.equal(await notice(browser).isExisting(), true, "the notice stays in the header after Close");
    },
  ),
  // UX journey: J-UPDATE; UX edges: E-UPDATE-NOTICE E-UPDATE-OPEN E-UPDATE-CLOSE
  scenario<Site>(
    "release-command",
    "an app installed with Cargo shows Cargo's update command, as text that can be selected",
    async () => {
      const fixture = createOutside();
      const cargo = nativePath(fixture.root, "cargo");
      const placed = placeApp(app(), nativePath(cargo, "bin"));
      const { data, server } = await site(fixture, `v${LATEST}`);
      return {
        fixture,
        env: { ...environment(fixture), ARTIFACTIZE_RELEASES_URL: data.url, CARGO_HOME: cargo },
        data,
        app: placed,
        release: () => server.close(),
      };
    },
    async (browser) => {
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
      await dialog(browser).$("button=Close").click();
      await browser.waitUntil(closed(browser), { timeout: 15000, timeoutMsg: "the dialog closes by Close" });
    },
  ),
  // UX journey: J-UPDATE; UX edges: E-UPDATE-NOTICE
  scenario<Site>(
    "release-store",
    "a Microsoft Store install never looks a release up and shows no notice",
    async () => {
      const fixture = createOutside();
      const placed = placeApp(app(), storePackageDirectory(fixture.root));
      const { data, server } = await site(fixture, `v${LATEST}`);
      return {
        fixture,
        env: { ...environment(fixture), ARTIFACTIZE_RELEASES_URL: data.url },
        data,
        app: placed,
        release: () => server.close(),
      };
    },
    async (browser, { data }) => {
      await lookedUp(browser);
      assert.equal(await notice(browser).isExisting(), false, "the Store updates the app: no notice");
      assert.deepEqual(visited(data), [], "the app asked nothing about releases");
    },
  ),
];
