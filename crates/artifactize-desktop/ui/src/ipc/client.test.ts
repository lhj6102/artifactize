// Event subscriptions end cleanly even when a view goes away before Tauri finished
// registering the listener.
import { describe, expect, it } from "vitest";
import { subscribe } from "./client";

describe("subscribe", () => {
  it("removes a listener whose registration completes after cleanup", async () => {
    let removed = 0;
    let deliver: ((event: { payload: number }) => void) | undefined;
    let registered: (() => void) | undefined;
    const listen = (callback: (event: { payload: number }) => void) =>
      new Promise<() => void>((resolve) => {
        deliver = callback;
        registered = () => resolve(() => (removed += 1));
      });
    const seen: number[] = [];
    const stop = subscribe(listen, (payload) => seen.push(payload));
    stop();
    registered?.();
    await Promise.resolve();
    await Promise.resolve();
    deliver?.({ payload: 1 });
    expect(removed).toBe(1);
    expect(seen).toEqual([]);
  });

  it("delivers payloads until stopped", async () => {
    let deliver: ((event: { payload: string }) => void) | undefined;
    let removed = 0;
    const listen = async (callback: (event: { payload: string }) => void) => {
      deliver = callback;
      return () => (removed += 1);
    };
    const seen: string[] = [];
    const stop = subscribe(listen, (payload) => seen.push(payload));
    await Promise.resolve();
    deliver?.({ payload: "a" });
    stop();
    deliver?.({ payload: "b" });
    expect(seen).toEqual(["a"]);
    expect(removed).toBe(1);
  });
});
