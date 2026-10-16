// Event subscriptions end cleanly even when a view goes away before Tauri finished
// registering the listener.
import { describe, expect, it } from "vitest";
import { subscribe } from "./client";

describe("subscribe", () => {
  it("removes a listener whose registration completes after cleanup", async () => {
    let removed = 0;
    let deliver!: (event: { payload: number }) => void;
    let registered!: () => void;
    const registration = new Promise<() => void>((resolve) => {
      registered = () => resolve(() => (removed += 1));
    });
    const listen = (callback: (event: { payload: number }) => void) => {
      deliver = callback;
      return registration;
    };
    const seen: number[] = [];
    const stop = subscribe(listen, (payload) => seen.push(payload));
    stop();
    registered();
    // subscribe() registered its continuation first, so removal completes before this await.
    await registration;
    deliver({ payload: 1 });
    expect(removed).toBe(1);
    expect(seen).toEqual([]);
  });

  it("delivers payloads until stopped", async () => {
    let deliver!: (event: { payload: string }) => void;
    let removed = 0;
    const registration = Promise.resolve(() => (removed += 1));
    const listen = (callback: (event: { payload: string }) => void) => {
      deliver = callback;
      return registration;
    };
    const seen: string[] = [];
    const stop = subscribe(listen, (payload) => seen.push(payload));
    await registration;
    deliver({ payload: "a" });
    stop();
    deliver({ payload: "b" });
    expect(seen).toEqual(["a"]);
    expect(removed).toBe(1);
  });
});
