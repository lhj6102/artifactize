// The palette registry: what a screen registers leaves again with it, so switching screens
// never lists a command twice.
import { describe, expect, it } from "vitest";
import { Registry, type Command } from "./commands.svelte";

const command = (id: string): Command => ({ id, label: id, group: "Test", run: () => {} });

describe("registry", () => {
  it("removes exactly the list that was added, however often lists come and go", () => {
    const registry = new Registry();
    for (let round = 0; round < 3; round += 1) {
      const monitor = registry.add(() => [command("monitor.refresh")]);
      const review = registry.add(() => [command("review.refresh")]);
      monitor();
      review();
    }
    const kept = registry.add(() => [command("app.palette")]);
    expect(registry.all.map((entry) => entry.id)).toEqual(["app.palette"]);
    kept();
    expect(registry.all).toEqual([]);
  });

  it("lists a command once when two scopes offer the same id", () => {
    const registry = new Registry();
    const first = registry.add(() => [command("human.claim")]);
    const second = registry.add(() => [{ ...command("human.claim"), label: "later" }]);
    const listed = registry.all;
    expect(listed.map((entry) => [entry.id, entry.label])).toEqual([["human.claim", "later"]]);
    first();
    second();
  });
});
