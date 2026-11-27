// The Review state against a mocked command boundary: verdict changes that answer out of
// order, the library's draft with its issues and Points, and the Tool log the backend's lines
// build.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it } from "vitest";
import { Jobs } from "../ipc/jobs.svelte";
import { Review } from "./review.svelte";
import { draftLibrary, humanView } from "./fixtures";
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
  it("shows a saved draft's verdict and issues as the library holds them, sending nothing", async () => {
    const { pending, library } = backend();
    library.draft.verdict = "GREEN";
    library.draft.issues = ["Read it all."];
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    await waitFor(() => expect(review.verdict).toBe("GREEN"));
    expect(review.issues).toEqual(["Read it all."]);
    expect(review.blocked("")).toBe("GREEN takes no issues: remove them, or submit RED with them.");
    library.draft.verdict = "RED";
    await review.readDraft();
    expect(review.blocked("")).toBeNull();
    expect(review.submittable("")).toBe(true);
    expect(review.blocked("half typed")).toBe("Add the typed issue first");
    expect(pending).toEqual([]);
  });

  it("asks for a verdict while the draft has none", async () => {
    backend();
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    await review.readDraft();
    expect(review.draft?.verdict).toBeNull();
    expect(review.verdict).toBeNull();
    expect(review.blocked("")).toBe("Choose GREEN or RED");
  });

  it("shows the verdict chosen last when its answer arrives before the earlier one", async () => {
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
    expect(review.verdict).toBe("RED");
  });

  it("writes issues one after another, each edit naming the issue it saw; a refused edit says why and shows the draft as it stands", async () => {
    const { library } = backend();
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    await waitFor(() => expect(review.draft).not.toBeNull());
    const [first, second] = await Promise.all([review.addIssue("one"), review.addIssue("two")]);
    expect([first, second]).toEqual([true, true]);
    expect(review.issues).toEqual(["one", "two"]);
    expect(await review.addIssue("   ")).toBe(false);
    // Changed elsewhere meanwhile: the edit names what it saw, the library refuses it.
    library.draft.issues[1] = "two, as another window wrote it";
    expect(await review.editIssue(1, "two", "2")).toBe(false);
    expect(review.notice?.error).toBe(true);
    expect(review.issues).toEqual(["one", "two, as another window wrote it"]);
    expect(await review.removeIssue(0, "one")).toBe(true);
    expect(review.issues).toEqual(["two, as another window wrote it"]);
  });

  it("keeps the issues when switching verdicts; RED needs one and GREEN takes none", async () => {
    const { pending, library } = backend();
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    await waitFor(() => expect(review.draft).not.toBeNull());
    const red = review.choose("RED");
    await waitFor(() => expect(pending).toHaveLength(1));
    pending[0].answer();
    await red;
    expect(review.blocked("")).toBe("RED needs at least one issue.");
    await review.addIssue("off by one");
    expect(review.blocked("")).toBeNull();
    const green = review.choose("GREEN");
    await waitFor(() => expect(pending[1]?.verdict).toBe("GREEN"));
    pending[1].answer();
    await green;
    expect(review.blocked("")).toBe("GREEN takes no issues: remove them, or submit RED with them.");
    const again = review.choose("RED");
    await waitFor(() => expect(pending[2]?.verdict).toBe("RED"));
    pending[2].answer();
    await again;
    expect(library.draft.verdict).toBe("RED");
    expect(review.issues).toEqual(["off by one"]);
  });

  it("reloads a Point independently of the issues", async () => {
    const { library } = backend();
    library.draft.verdict = "RED";
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    await waitFor(() => expect(review.verdict).toBe("RED"));
    await review.addIssue("Total still wrong");
    const point = { text: "#checkout .total", source: "Checkout", at: "2026-11-22T22:00:00Z" };
    library.draft.points.push(point);
    await review.readDraft();
    expect(review.draft?.points).toEqual([point]);
    expect(review.issues).toEqual(["Total still wrong"]);
  });

  it("keeps the claim and the chosen verdict when a read from before the claim answers after it", async () => {
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
    expect(review.verdict).toBe("GREEN");
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
