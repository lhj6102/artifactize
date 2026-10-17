// An eval's Summary: its status (REUSED in the status colour, what running it cost or what reuse
// saved in grey), its instruction and result, then execution options, metadata and fingerprint,
// with every other fact folded under Debug. An Artifact's Detail shows its path and what it
// waits for, the rest under Debug.
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, expect, it } from "vitest";
import type { CardView, DetailView } from "../ipc/client";
import RecordDetail from "./RecordDetail.svelte";

afterEach(cleanup);

const card = (change: Partial<CardView> = {}): CardView => ({
  instruction: "Check that the tests follow the test rules.",
  result: null,
  exitCode: null,
  error: null,
  reused: false,
  took: "3m 12s",
  tokens: 48200,
  model: "gpt-6.1-sol",
  ...change,
});

const detail = (change: Partial<DetailView> = {}): DetailView => ({
  run: "run-a",
  node: "e:app/rules",
  title: "app/rules · Rules",
  summary: "RED 3m 12s · agent codex gpt-6.1-sol",
  request: null,
  profile: "agent",
  mark: { status: "RED", glyph: "✗" },
  running: false,
  sections: [
    {
      name: "outcome",
      title: "Outcome",
      fields: [
        { key: "Status", value: "RED — a finding" },
        { key: "Result", value: "verdict RED" },
      ],
      waits: [],
    },
    { name: "what", title: "What", fields: [{ key: "Instruction", value: "Check that the tests follow the test rules." }], waits: [] },
    {
      name: "provenance",
      title: "Provenance",
      fields: [
        { key: "Timing", value: "created a · started b · completed c" },
        { key: "Usage", value: "complete · attempts 1" },
      ],
      waits: [],
    },
    {
      name: "technical",
      title: "Technical",
      fields: [
        { key: "Profile", value: "agent codex gpt-6.1-sol" },
        { key: "Options", value: "timeoutMs 60000" },
        { key: "Fingerprint", value: "9f3c1e" },
        { key: "Key", value: "k-1" },
        { key: "Request", value: "run-a-1" },
      ],
      waits: [],
    },
  ],
  evidence: null,
  card: card(),
  ...change,
});

it("shows the status with what running it cost, the instruction and the result's findings", () => {
  render(RecordDetail, {
    detail: detail({
      card: card({
        result: [
          {
            key: "violations",
            text: null,
            items: [{ head: "TEST-SEAMS · ui/src/Panel.test.ts:40", fields: [{ key: "change", value: "Assert the rendered row." }] }],
          },
        ],
      }),
    }),
  });
  const status = screen.getByText("✗ RED");
  expect(status.classList.contains("tone-red")).toBe(true);
  expect(screen.getByText("3m 12s · 48.2k tokens · gpt-6.1-sol")).toBeTruthy();
  expect(screen.getByText("Check that the tests follow the test rules.")).toBeTruthy();
  expect(screen.getByRole("heading", { name: "Result" })).toBeTruthy();
  expect(screen.getByText("1 violations")).toBeTruthy();
  expect(screen.getByText("TEST-SEAMS · ui/src/Panel.test.ts:40")).toBeTruthy();
  expect(screen.getByText("Assert the rendered row.")).toBeTruthy();
});

it("marks a reused result REUSED in the status colour and shows what reuse saved", () => {
  render(RecordDetail, { detail: detail({ mark: { status: "GREEN", glyph: "✓" }, card: card({ reused: true }) }) });
  const status = screen.getByText("✓ GREEN · REUSED");
  expect(status.classList.contains("tone-green")).toBe(true);
  expect(screen.getByText("saved 3m 12s · saved 48.2k tokens · gpt-6.1-sol")).toBeTruthy();
});

it("shows execution options, what running it took and spent, and the fingerprint; every other fact folds under Debug", async () => {
  render(RecordDetail, { detail: detail({ card: card({ tokens: 1_250_000 }) }) });
  const headings = screen.getAllByRole("heading").map((heading) => heading.textContent);
  expect(headings).toEqual(["Execution options", "Execution metadata", "Fingerprint"]);
  expect(screen.getByText("timeoutMs 60000")).toBeTruthy();
  // Time and tokens in k or M, not the per-turn usage or timestamps.
  expect(screen.getByText("3m 12s · 1.3M tokens")).toBeTruthy();
  expect(screen.queryByText("complete · attempts 1")).toBeNull();
  expect(screen.getByText("9f3c1e")).toBeTruthy();
  // Debug starts folded: the request's other facts are not shown until it is opened.
  const fold = screen.getByRole("button", { name: "▸ Debug" });
  expect(fold.getAttribute("aria-expanded")).toBe("false");
  expect(screen.queryByText("run-a-1")).toBeNull();
  await fireEvent.click(fold);
  expect(screen.getByText("run-a-1")).toBeTruthy();
  expect(screen.getByText("k-1")).toBeTruthy();
  expect(screen.getByText("complete · attempts 1")).toBeTruthy();
  // What the Summary shows is not repeated under Debug.
  expect(screen.getAllByText("9f3c1e")).toHaveLength(1);
});

it("shows a runtime eval's exit code and no tokens or model", () => {
  render(RecordDetail, {
    detail: detail({ profile: "runtime", mark: { status: "GREEN", glyph: "✓" }, card: card({ exitCode: 0, tokens: null, model: null, took: "41s" }) }),
  });
  // Beside the status and in the metadata: the time only.
  expect(screen.getAllByText("41s")).toHaveLength(2);
  expect(screen.queryByText(/tokens/)).toBeNull();
  expect(screen.getByText("exit 0")).toBeTruthy();
});

it("shows an Artifact's path, and its other facts only under Debug", async () => {
  render(RecordDetail, {
    detail: detail({
      node: "a:app",
      profile: null,
      mark: null,
      card: null,
      summary: "RED · 1 RED · 2 GREEN",
      sections: [
        {
          name: "outcome",
          title: "Outcome",
          fields: [
            { key: "Status", value: "RED" },
            { key: "Evals", value: "lint, test" },
          ],
          waits: [],
        },
        {
          name: "technical",
          title: "Technical",
          fields: [
            { key: "Path", value: "crates/app" },
            { key: "Fingerprint", value: "a1b2" },
          ],
          waits: [],
        },
      ],
    }),
  });
  expect(screen.getByText("RED · 1 RED · 2 GREEN")).toBeTruthy();
  expect(screen.getByText("crates/app")).toBeTruthy();
  expect(screen.queryByText("a1b2")).toBeNull();
  await fireEvent.click(screen.getByRole("button", { name: "▸ Debug" }));
  expect(screen.getByText("a1b2")).toBeTruthy();
});
