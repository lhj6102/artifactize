// The palette registry: what a screen registers leaves again with it, so switching screens
// never lists a command twice.
import { describe, expect, it } from "vitest";
import { Registry, type Command } from "./commands.svelte";

const registry = new Registry();

const command = (id: string): Command => ({ id, label: id, group: "Test", run: () => {} });

describe("registry", () => {
  it("removes exactly the list that was added, however often lists come and go", () => {
    const before = registry.all.length;
    for (let round = 0; round < 3; round += 1) {
      const monitor = registry.add(() => [command("monitor.refresh")]);
      const review = registry.add(() => [command("review.refresh")]);
      monitor();
      review();
    }
    const kept = registry.add(() => [command("app.palette")]);
    expect(registry.all.map((entry) => entry.id).slice(before)).toEqual(["app.palette"]);
    kept();
    expect(registry.all.length).toBe(before);
  });

  it("lists a command once when two scopes offer the same id", () => {
    const before = registry.all.length;
    const first = registry.add(() => [command("human.claim")]);
    const second = registry.add(() => [{ ...command("human.claim"), label: "later" }]);
    const listed = registry.all.slice(before);
    expect(listed.map((entry) => [entry.id, entry.label])).toEqual([["human.claim", "later"]]);
    first();
    second();
  });
});
