// The completion oracle is independent of elapsed time and of unchanged screenshots.
import assert from "node:assert/strict";
import { it } from "node:test";
import { completed, type Checkpoint } from "./commands.ts";

const searched = (query: string, sequence: number, done = true, pending = 0): Checkpoint => ({
  query,
  activity: { revision: sequence * 2, pending, commands: {}, search: { query, sequence, completed: done } },
});

it("waits for a new matching search after a query changes, including A → B → A", () => {
  const first = searched("A", 1);
  const typingB = { ...first, query: "B" };
  assert.equal(completed(typingB, first), false, "no command is pending, but the B debounce has not dispatched");
  const b = searched("B", 2);
  assert.equal(completed(b, first), true);
  const oldA = searched("A", 1);
  assert.equal(completed(oldA, b), false, "an old completed A cannot satisfy a later A");
  assert.equal(completed(searched("A", 3, false, 1), b), false, "dispatch is not completion");
  assert.equal(completed(searched("A", 3), b), true);
});

it("requires pending commands to finish even when the query already matches", () => {
  assert.equal(completed(searched("A", 1, true, 1)), false);
  assert.equal(completed(searched("A", 1)), true);
  assert.equal(completed({ activity: searched("A", 1).activity, query: null }), true);
});

it("a completed failed query cannot reuse rows left by a successful query", () => {
  const before = searched("reviewer:alice", 1);
  const failed = { ...searched("is:mine", 2), queryFailure: "Search read failed" };
  assert.equal(completed(failed, before), false);
  assert.equal(completed({ ...failed, queryFailure: null }, before), true);
});

it("needs no search for the starting query until one is typed", () => {
  const start: Checkpoint = { query: "sort:recent", activity: { revision: 1, pending: 0, commands: {}, search: null } };
  assert.equal(completed(start), true);
  assert.equal(completed({ ...start, query: "branch:x" }, start), false, "a typed query waits for its own search");
});
