// press() against a page double: a key the page received but the app ignored fails at once
// and is never sent again, also when a later key would have made the check pass; a dropped
// key (or a chord's dropped action key) is sent again; several keys are several presses.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { type Page, press } from "./keys.ts";

/** A page whose driver drops the keys `drop` says, and an app that reacts as `react` says. */
function double(drop: (key: string, sent: number) => boolean, react: (key: string) => void) {
  const log: string[] = [];
  const sent: string[][] = [];
  const page: Page = {
    keys: async (keys) => {
      sent.push(keys);
      for (const key of keys) {
        if (drop(key, sent.length)) continue;
        log.push(key);
        react(key);
      }
    },
    received: async () => [...log],
  };
  return { page, sent };
}

const quick = { wait: 30, ms: 120 };

describe("press", () => {
  it("refuses several action keys in one press, which could hide one the app lost", async () => {
    const { page } = double(() => false, () => {});
    await assert.rejects(press(page, ["ArrowDown", "Enter"], () => true, "two keys", quick), /one action at a time/);
  });

  it("fails without a resend when the page received a key the app ignored", async () => {
    // ArrowDown arrives and the app loses it; Enter is never part of this press.
    const { page, sent } = double(() => false, () => {});
    await assert.rejects(press(page, ["ArrowDown"], () => false, "a suggestion chosen", quick), /the page received ArrowDown/);
    assert.equal(sent.length, 1, "a delivered key is never sent again");
  });

  it("sends a dropped key again, and a chord whose action key was dropped", async () => {
    let downs = 0;
    const first = double((_, sent) => sent === 1, () => (downs += 1));
    await press(first.page, ["ArrowDown"], () => downs === 1, "moved", quick);
    assert.equal(first.sent.length, 2);
    // Control arrives but k is dropped: the chord is sent again; then k arrives.
    let open = false;
    const chord = double((key, sent) => key === "k" && sent === 1, (key) => key === "k" && (open = true));
    await press(chord.page, ["Control", "k"], () => open, "palette open", quick);
    assert.deepEqual(chord.sent, [["Control", "k"], ["Control", "k"]]);
  });

  it("does not accept an already-true state when the action key was dropped", async () => {
    const { page, sent } = double((key) => key === "g", () => {});
    await assert.rejects(press(page, ["Control", "g"], () => true, "GREEN stays", { ...quick, tries: 2 }), /never reached the page in 2 tries/);
    assert.equal(sent.length, 2, "modifier-only delivery cannot satisfy an idempotent action");
  });

  it("confirms the consumed chord before accepting an idempotent action", async () => {
    const handled: string[] = [];
    const { page, sent } = double((key, attempt) => key === "g" && attempt === 1, (key) => {
      if (key === "g") handled.push("Control+g");
    });
    page.handled = async () => [...handled];
    await press(page, ["Control", "g"], () => true, "GREEN stays", { ...quick, handled: true });
    assert.deepEqual(sent, [["Control", "g"], ["Control", "g"]]);
  });

  it("rejects an unconsumed or wrong-modifier chord even when its state already holds", async () => {
    for (const chord of [null, "r"]) {
      const handled: string[] = [];
      const { page, sent } = double(() => false, (key) => {
        if (key === "r" && chord) handled.push(chord);
      });
      page.handled = async () => [...handled];
      await assert.rejects(press(page, ["Control", "r"], () => true, "RED stays", { ...quick, handled: true }), /handled effect was not confirmed/);
      assert.equal(sent.length, 1, "an arrived action is not resent to hide missing handling");
    }
  });

  it("gives up after its tries when the key never arrives", async () => {
    const { page, sent } = double(() => true, () => {});
    await assert.rejects(press(page, ["Enter"], () => false, "never", { ...quick, tries: 2 }), /never reached the page in 2 tries/);
    assert.equal(sent.length, 2);
  });
});
