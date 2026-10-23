// A newer release through the real app: a loopback stand-in for the repository's site answers
// the release lookup, the header names the release and its dialog says how to update. This
// build's install channel is unknown, so the dialog links the release page, which the system
// browser must GET; only the site records that evidence.
import assert from "node:assert/strict";
import { createOutside, environment } from "../fixture.ts";
import { press } from "../keys.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { appendText, nativePath, readText, serveLoopbackHttp, writeText, type NativePath } from "../platform.ts";

const LATEST = "9.9.9";

export const scenarios: Scenario[] = [
  // UX journey: J-UPDATE; UX edges: E-UPDATE-NOTICE E-UPDATE-OPEN E-UPDATE-LINK E-UPDATE-CLOSE
  scenario<{ page: string; visits: NativePath }>(
    "release-notice",
    "a newer release shows in the header; its dialog says how to update, opens the release page and closes by Esc or Close",
    async () => {
      const fixture = createOutside();
      const visits = nativePath(fixture.root, "site-get-paths.jsonl");
      writeText(visits, "");
      const site = await serveLoopbackHttp((request, response) => {
        request.resume();
        if (request.method === "GET") appendText(visits, `${JSON.stringify(request.url)}\n`);
        const lookup = request.url === "/api/releases/latest";
        response.writeHead(lookup || request.url === "/releases/latest" ? 200 : 404, { "content-type": lookup ? "application/json" : "text/html" });
        response.end(lookup ? JSON.stringify({ tag_name: `v${LATEST}` }) : "");
      });
      const page = `${site.url}/releases/latest`;
      return {
        fixture,
        env: { ...environment(fixture), ARTIFACTIZE_RELEASES_URL: site.url },
        data: { page, visits },
        browser: page,
        release: () => site.close(),
      };
    },
    async (browser, { data: { page, visits } }) => {
      const visited = (): string[] => readText(visits).split("\n").filter(Boolean).map((line) => JSON.parse(line) as string);
      const notice = () => browser.$(`button=${LATEST} available`);
      await notice().waitForExist({ timeout: 15000, timeoutMsg: "the release lookup puts the notice in the header" });
      const header = await browser.execute(() =>
        [...document.querySelectorAll("header.bar button")].map((button) => button.textContent?.trim() ?? ""));
      assert.ok(header.indexOf(`${LATEST} available`) < header.indexOf("Settings"), `the notice comes before Settings: ${header.join(" | ")}`);

      const dialog = () => browser.$('[role="dialog"]');
      const shown = async () => (await dialog().isExisting()) && (await dialog().getText()).includes(`artifactize ${LATEST} is available`);
      const closed = async () => !(await dialog().isExisting());

      await notice().click();
      await browser.waitUntil(shown, { timeout: 15000, timeoutMsg: "the notice opens its dialog" });
      const text = await dialog().getText();
      const running = /This app is (\S+)\./.exec(text)?.[1];
      assert.ok(running && running !== LATEST, `the dialog names the running version: ${text}`);
      assert.ok(text.includes("See the release page"), text);
      const link = () => dialog().$(`button=${page}`);
      assert.equal(await link().isExisting(), true, "the release page is a link");
      await link().click();
      await browser.waitUntil(() => visited().includes("/releases/latest"), {
        timeout: 15000,
        timeoutMsg: "the system browser reaches the release page the dialog links",
      });
      assert.equal(await shown(), true, "the dialog stays open after its link opens");
      await press(browser, ["Escape"], closed, "the dialog closes by Esc");
      assert.equal(await notice().isExisting(), true, "the notice stays in the header after Esc");

      await notice().click();
      await browser.waitUntil(shown, { timeout: 15000, timeoutMsg: "the notice opens its dialog again" });
      await dialog().$("button=Close").click();
      await browser.waitUntil(closed, { timeout: 15000, timeoutMsg: "the dialog closes by Close" });
      assert.equal(await notice().isExisting(), true, "the notice stays in the header after Close");
    },
  ),
];
