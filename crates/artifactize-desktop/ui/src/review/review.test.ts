// The Review state against a mocked command boundary: verdict forms that answer out of
// order, the library's draft per verdict and its Notes, and the Tool log the backend's lines
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
function backend() {
  const pending: Pending[] = [];
  const library = draftLibrary();
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

  it("opens no form that arrives after editing stopped", async () => {
    const { pending } = backend();
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    const green = review.choose("GREEN");
    await waitFor(() => expect(pending).toHaveLength(1));
    review.stopEditing();
    pending[0].answer();
    await green;
    expect(review.form).toBeNull();
  });

  it("keeps each verdict's typed draft in the library when switching verdicts", async () => {
    const { pending, library } = backend();
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    const green = review.choose("GREEN");
    await waitFor(() => expect(pending).toHaveLength(1));
    pending[0].answer();
    await green;
    await review.form!.set("comment", { kind: "text", value: "x" });
    const red = review.choose("RED");
    await waitFor(() => expect(pending[1]?.verdict).toBe("RED"));
    pending[1].answer();
    await red;
    const again = review.choose("GREEN");
    await waitFor(() => expect(pending[2]?.verdict).toBe("GREEN"));
    pending[2].answer();
    await again;
    // The library kept the GREEN draft while RED was open, and hands it back.
    expect(library.draft.verdict).toBe("GREEN");
    expect(review.form?.values.comment).toEqual({ kind: "text", value: "x" });
  });

  it("writes the Notes in order, keeping a Point the library appended meanwhile", async () => {
    const { library } = backend();
    const review = new Review("run-a-2", new Jobs());
    await review.load();
    await waitFor(() => expect(review.draft).not.toBeNull());
    review.writeNotes("Total");
    review.writeNotes("Total still");
    // A viewer's Point lands while the typing is on its way.
    library.answer("human_draft_append", { text: "#checkout .total" });
    review.writeNotes("Total still wrong");
    await waitFor(() => expect(library.draft.notes).toBe("Total still wrong\n#checkout .total"));
    await waitFor(() => expect(review.notes).toBe("Total still wrong\n#checkout .total"));
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
    expect(review.log).toEqual([]);
    review.logged([{ run: "7", at: 0, operation: "notes", artifact: "release", lines: [{ text: "hello", error: false }], end: null }]);
    expect(review.log.map((entry) => entry.lines)).toEqual([[{ text: "hello", error: false }]]);
  });
});
