// Korean through the desktop's real input method: WebDriver's keystrokes go through GTK's
// input method, so with fcitx5 in Hangul mode `dkssud` composes 안녕 in the form field, and
// keys that are part of the composition are never shortcuts. Opt in with E2E_IME=hangul on
// a desktop whose input method is in Hangul mode; the other tests type literal text.
import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { create, type Fixture, signoff } from "./fixture.ts";
import { type Session, startWith } from "./driver.ts";

const enabled = process.env.E2E_IME === "hangul";

describe("Korean input method", { skip: !enabled && "set E2E_IME=hangul with fcitx5 Hangul active" }, () => {
  let fixture: Fixture;
  let session: Session;
  let browser: WebdriverIO.Browser;

  before(async () => {
    fixture = create();
    session = await startWith("desktop", fixture);
    browser = session.browser;
  });

  after(async () => {
    await session?.stop();
  });

  it("composes Hangul in a form field and submits it", async () => {
    await browser.$('//li[@role="treeitem"][.//span[text()="signoff"]]').click();
    await browser.keys(["Enter"]);
    await browser.$('[aria-label="Human review"]').waitForExist();
    await browser.keys(["c"]);
    await browser.waitUntil(async () => signoff(fixture).claim?.reviewer === "alice");
    await browser.keys(["g"]);
    const comment = await browser.$("#field-comment");
    await comment.waitForExist();
    await comment.click();
    // One key at a time: the input method composes asynchronously. Space commits the word;
    // `c`, `g`, `r` and `u` inside the composition must stay text.
    for (const key of [..."dkssudgktpdy", " ", ..."cgru", " "]) {
      await browser.keys([key]);
      await browser.pause(120);
    }
    const value = await browser.execute(() => (document.querySelector("#field-comment") as HTMLTextAreaElement).value);
    assert.equal(value, "안녕하세요 ㅊ효 ");
    assert.equal(signoff(fixture).status, "WAITING_HUMAN");
    await browser.keys(["Control", "s"]);
    await browser.waitUntil(async () => signoff(fixture).status === "GREEN", { timeout: 15000 });
    assert.equal(signoff(fixture).result?.comment, "안녕하세요 ㅊ효 ");
  });
});
