// The completion oracle is independent of elapsed time and of unchanged screenshots.
import assert from "node:assert/strict";
import { it } from "node:test";
import { completed, type Checkpoint } from "./commands.mts";

const searched = (query: string, sequence: number, done = true, pending = 0): Checkpoint => ({
  query,
  reviewQuery: null,
  activity: { revision: sequence * 2, pending, commands: {}, reviewSearch: null, search: { query, sequence, completed: done } },
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
  assert.equal(completed({ activity: searched("A", 1).activity, query: null, reviewQuery: null }), true);
});

it("a completed failed query cannot reuse rows left by a successful query", () => {
  const before = searched("reviewer:alice", 1);
  const failed = { ...searched("sort:size", 2), queryFailure: "Unknown sort `size`" };
  assert.equal(completed(failed, before), true, "an invalid Verify query was rendered by its matching reply");
  assert.equal(completed({ ...failed, query: "sort:another" }, before), false, "an old error cannot settle a different query");
});

it("does not take old Review rows for the new filter when a background search finishes first", () => {
  const review = (filter: string, searched: string, sequence: number, completed = true): Checkpoint => ({
    query: null,
    reviewQuery: filter,
    activity: { revision: sequence * 2, pending: 0, search: null, commands: {}, reviewSearch: { query: searched, sequence, completed } },
  });
  const before = review("waiting", "waiting", 1);
  assert.equal(completed(review("is:mine", "waiting", 1), before), false, "the new filter has not dispatched");
  assert.equal(completed(review("is:mine", "waiting", 2), before), false, "a reload of the previous query proves nothing");
  assert.equal(completed(review("is:mine", "is:mine", 3, false), before), false, "new dispatch without completion");
  const matching = review("is:mine", "is:mine", 3);
  const failed = { ...matching, activity: { ...matching.activity, reviewSearch: { ...matching.activity.reviewSearch!, failed: true as const } } };
  assert.equal(completed(failed, before), false, "a failed reply alone is not a render");
  assert.equal(completed({ ...failed, reviewFailure: "Invalid Review filter" }, before), true, "a rendered failure settles the matching query");
  assert.equal(completed(matching, before), true);
  assert.equal(completed(before, matching), false, "an old completion cannot satisfy a return to the same filter");
  assert.equal(completed(review("waiting", "waiting", 4), matching), true);
});

it("needs no search for the starting query until one is typed", () => {
  const start: Checkpoint = { query: "sort:recent", reviewQuery: null, activity: { revision: 1, pending: 0, commands: {}, search: null, reviewSearch: null } };
  assert.equal(completed(start), true);
  assert.equal(completed({ ...start, query: "branch:x" }, start), false, "a typed query waits for its own search");
});
