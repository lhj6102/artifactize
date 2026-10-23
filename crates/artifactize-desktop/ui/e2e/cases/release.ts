// A newer release through the real app: a loopback stand-in for GitHub's latest-release API
// answers, the header names the release and its dialog says how to update. This build's
// install channel is unknown, so the dialog links the release page.
import assert from "node:assert/strict";
import { createOutside, environment } from "../fixture.ts";
import { press } from "../keys.ts";
import { scenario, type Scenario } from "../scenario.ts";
import { serveLoopbackHttp } from "../platform.ts";

const LATEST = "9.9.9";
const RELEASE_PAGE = "https://github.com/lhj6102/artifactize/releases/latest";

export const scenarios: Scenario[] = [
  // UX journey: J-UPDATE
  scenario<null>(
    "release-notice",
    "a newer release shows in the header; its dialog says how to update and closes by Esc or Close",
    async () => {
      const fixture = createOutside();
      const releases = await serveLoopbackHttp((request, response) => {
        request.resume();
        const latest = request.url === "/repos/latest";
        response.writeHead(latest ? 200 : 404, { "content-type": "application/json" });
        response.end(JSON.stringify(latest ? { tag_name: `v${LATEST}` } : {}));
      });
      return {
        fixture,
        env: { ...environment(fixture), ARTIFACTIZE_RELEASES_URL: `${releases.url}/repos/latest` },
        data: null,
        release: () => releases.close(),
      };
    },
    async (browser) => {
      const notice = () => browser.$(`button=${LATEST} available`);
      await notice().waitForExist({ timeout: 15000, timeoutMsg: "the header names the newer release" });
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
      assert.equal(await dialog().$(`button=${RELEASE_PAGE}`).isExisting(), true, "the release page is a link");
      await press(browser, ["Escape"], closed, "the dialog closes by Esc");

      await notice().click();
      await browser.waitUntil(shown, { timeout: 15000, timeoutMsg: "the notice opens its dialog again" });
      await dialog().$("button=Close").click();
      await browser.waitUntil(closed, { timeout: 15000, timeoutMsg: "the dialog closes by Close" });
      assert.equal(await notice().isExisting(), true, "the notice stays in the header");
    },
  ),
];
