// A burst of change events costs at most two reads, and none while what they feed is hidden.
import { describe, expect, it } from "vitest";
import { Coalesced } from "./coalesced";

/** A read that ends when the test says so, counting how often it started. */
function gated() {
  let started = 0;
  const ends: (() => void)[] = [];
  const read = () => {
    started += 1;
    return new Promise<void>((done) => ends.push(done));
  };
  const finish = async () => {
    ends.shift()?.();
    await new Promise((settle) => setTimeout(settle, 0));
  };
  return { read, finish, started: () => started };
}

describe("Coalesced", () => {
  it("reads at most twice for a burst: the one in flight and one after it", async () => {
    const reads = gated();
    const refresh = new Coalesced(reads.read);
    for (let event = 0; event < 50; event += 1) refresh.request();
    expect(reads.started()).toBe(1);
    await reads.finish();
    expect(reads.started()).toBe(2);
    await reads.finish();
    expect(reads.started()).toBe(2);
  });

  it("reads nothing while hidden, then once when shown again", async () => {
    const reads = gated();
    let shown = false;
    const refresh = new Coalesced(reads.read, () => shown);
    for (let event = 0; event < 50; event += 1) refresh.request();
    expect(reads.started()).toBe(0);
    shown = true;
    refresh.showing();
    refresh.showing();
    expect(reads.started()).toBe(1);
    await reads.finish();
    expect(reads.started()).toBe(1);
  });

  it("keeps a change that came in flight for when it is shown, if it was hidden meanwhile", async () => {
    const reads = gated();
    let shown = true;
    const refresh = new Coalesced(reads.read, () => shown);
    refresh.request();
    refresh.request();
    shown = false;
    await reads.finish();
    expect(reads.started()).toBe(1);
    shown = true;
    refresh.showing();
    expect(reads.started()).toBe(2);
    await reads.finish();
  });
});
