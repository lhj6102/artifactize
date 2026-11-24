// The Review state against a mocked command boundary: verdict forms that answer out of
// order, the library's draft per verdict and its Points, and the Tool log the backend's lines
// build.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it } from "vitest";
import { Jobs } from "../ipc/jobs.svelte";
import { Review } from "./review.svelte";
import { draftLibrary, greenForm, humanView, scoredRed } from "./fixtures";
import type { DraftChange } from "../ipc/client";

/** A verdict change the test answers when it chooses. */
type Pending = { verdict: string; answer: () => void };

/** The backend of one claimed review whose draft the library keeps; verdict changes wait for
 * the test to answer them. */
function backend(library = draftLibrary()) {
  const pending: Pending[] = [];
  mockIPC(async (cmd, args) => {
    if (cmd === "human_open") return humanView("review");
    const change = (args as { change?: DraftChange }).change;
    if (cmd === "human_draft_edit" && change?.kind === "verdict") {
      await new Promise<void>((answer) => pending.push({ verdict: change.verdict, answer }));
    }
    return library.answer(cmd, args) ?? null;
  });
  return { pending, library };
}

afterEach(() => clearMocks());

describe("Review", () => {
  it("shows a saved draft's verdict and reasons as the library holds them, sending nothing", async () => {
    const { pending, library } = backend();
    library.draft.verdict = "GREEN";
    library.draft.reasons = ["Read it all."];
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    await waitFor(() => expect(review.verdict).toBe("GREEN"));
    expect(review.reasons).toEqual(["Read it all."]);
    expect(review.form?.verdict).toBe("GREEN");
    expect(review.blocked("")).toBeNull();
    expect(review.submittable("")).toBe(true);
    expect(review.blocked("half typed")).toBe("Add the typed reason first");
    expect(pending).toEqual([]);
  });

  it("opens no form for a draft whose verdict is not chosen yet", async () => {
    backend();
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    await review.readDraft();
    expect(review.draft?.verdict).toBeNull();
    expect(review.form).toBeNull();
  });

  it("shows the verdict chosen last when its form arrives before the earlier one", async () => {
    const { pending } = backend();
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    const green = review.choose("GREEN");
    const red = review.choose("RED");
    await waitFor(() => expect(pending.map((request) => request.verdict)).toEqual(["GREEN", "RED"]));
    pending.find((request) => request.verdict === "RED")!.answer();
    await red;
    pending.find((request) => request.verdict === "GREEN")!.answer();
    await green;
    expect(review.form?.verdict).toBe("RED");
  });

  it("writes reasons one after another, each edit naming the reason it saw; a refused edit says why and shows the draft as it stands", async () => {
    const { library } = backend();
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    await waitFor(() => expect(review.draft).not.toBeNull());
    const [first, second] = await Promise.all([review.addReason("one"), review.addReason("two")]);
    expect([first, second]).toEqual([true, true]);
    expect(review.reasons).toEqual(["one", "two"]);
    expect(await review.addReason("   ")).toBe(false);
    // Changed elsewhere meanwhile: the edit names what it saw, the library refuses it.
    library.draft.reasons[1] = "two, as another window wrote it";
    expect(await review.editReason(1, "two", "2")).toBe(false);
    expect(review.notice?.error).toBe(true);
    expect(review.reasons).toEqual(["one", "two, as another window wrote it"]);
    expect(await review.removeReason(0, "one")).toBe(true);
    expect(review.reasons).toEqual(["two, as another window wrote it"]);
  });

  it("keeps each verdict's own fields in the library when switching verdicts, the reasons kept", async () => {
    const { pending, library } = backend(draftLibrary(greenForm, scoredRed));
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    await waitFor(() => expect(review.draft).not.toBeNull());
    await review.addReason("off by one");
    const red = review.choose("RED");
    await waitFor(() => expect(pending).toHaveLength(1));
    pending[0].answer();
    await red;
    expect(review.blocked("")).toBe("score is required");
    await review.form!.set("score", { kind: "integer", value: "2" });
    expect(review.blocked("")).toBeNull();
    const green = review.choose("GREEN");
    await waitFor(() => expect(pending[1]?.verdict).toBe("GREEN"));
    pending[1].answer();
    await green;
    const again = review.choose("RED");
    await waitFor(() => expect(pending[2]?.verdict).toBe("RED"));
    pending[2].answer();
    await again;
    expect(library.draft.verdict).toBe("RED");
    expect(review.form?.values.score).toEqual({ kind: "integer", value: "2" });
    expect(review.reasons).toEqual(["off by one"]);
  });

  it("reloads a Point independently of the verdict's fields", async () => {
    const { library } = backend(draftLibrary(greenForm, scoredRed));
    library.draft.verdict = "RED";
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    await waitFor(() => expect(review.form?.verdict).toBe("RED"));
    await review.form?.set("note", { kind: "text", value: "Total still wrong" });
    await waitFor(() => expect(library.draft.red.draft.fields[1][1]).toEqual({ kind: "text", value: "Total still wrong" }));
    const point = { text: "#checkout .total", source: "Checkout", at: "2026-11-22T22:00:00Z" };
    library.draft.points.push(point);
    await review.readDraft();
    expect(review.draft?.points).toEqual([point]);
    expect(review.form?.values.note).toEqual({ kind: "text", value: "Total still wrong" });
  });

  it("keeps the claim and the open form when a read from before the claim answers after it", async () => {
    // The first read is held until the test answers it; the claim reads after it.
    let answer!: () => void;
    const held = new Promise<void>((done) => (answer = done));
    let opened = 0;
    const library = draftLibrary();
    mockIPC(async (cmd, args) => {
      if (cmd === "human_open") {
        opened += 1;
        if (opened === 2) await held;
        return humanView("claim", { read: opened });
      }
      if (cmd === "human_claim") return { notice: { text: "Claimed.", error: false }, view: humanView("review", { read: 3 }) };
      return library.answer(cmd, args) ?? null;
    });
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    const late = review.load();
    await waitFor(() => expect(opened).toBe(2));
    await review.claim();
    await review.choose("GREEN");
    expect(review.owned).toBe(true);
    answer();
    await late;
    expect(review.view?.stage).toBe("review");
    expect(review.form?.verdict).toBe("GREEN");
  });

  it("runs a tool for its notice; its lines arrive in the Tool log on their own", async () => {
    mockIPC(async (cmd) => {
      if (cmd === "human_open") return humanView("review");
      if (cmd === "human_run") return { text: "notes_release launched.", error: false };
      return null;
    });
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    await review.run(0);
    expect(review.notice?.text).toBe("notes_release launched.");
    expect(review.submittable("")).toBe(false);
    expect(review.log).toEqual([]);
    review.logged([{ run: "7", at: 0, operation: "notes", artifact: "release", lines: [{ text: "hello", error: false }], end: null }]);
    expect(review.log.map((entry) => entry.lines)).toEqual([[{ text: "hello", error: false }]]);
  });
});
