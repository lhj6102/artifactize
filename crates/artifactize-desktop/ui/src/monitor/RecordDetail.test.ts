// An eval's Summary: its status line (REUSED in the status colour, what running it cost or what
// reuse saved in grey), its instruction and its Result, then Execution options, Execution
// metadata, Fingerprint and Debug, folded under their header rows. An Artifact's Detail shows its
// Path, with every other fact, what it waits for included, under Debug. A Run's leads with its
// Outcome and what it waits for.
import { cleanup, fireEvent, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, expect, it } from "vitest";
import type { CardView, DetailView } from "../ipc/client";
import { renderHosted } from "../test-host";
import RecordDetail from "./RecordDetail.svelte";

beforeEach(() => localStorage.clear());
afterEach(cleanup);

const render = (props: { detail: DetailView }) => renderHosted(RecordDetail, { log: false, ...props });
/** The header rows of the sections shown, in order, and whether each is unfolded. */
const rows = () => screen.queryAllByRole("button", { expanded: true }).concat(screen.queryAllByRole("button", { expanded: false }))
  .sort((one, other) => (one.compareDocumentPosition(other) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1))
  .map((row) => `${row.textContent?.trim()}${row.getAttribute("aria-expanded") === "true" ? " ▾" : " ▸"}`);

const card = (change: Partial<CardView> = {}): CardView => ({
  instruction: "Check that the tests follow the test rules.",
  result: null,
  raw: null,
  exitCode: null,
  error: null,
  reused: false,
  took: "3m 12s",
  tokens: 48200,
  model: "gpt-6.1-sol",
  ...change,
});

const detail = (change: Partial<DetailView> = {}): DetailView => ({
  source: { kind: "run", run: "run-a" },
  plan: null,
  node: "e:app/rules",
  target: "eval",
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
  outcome: null,
  ...change,
});

it("shows the status with what running it cost, the instruction and the result's findings", () => {
  render({
    detail: detail({
      card: card({
        result: [
          {
            key: "violations",
            kind: "items",
            items: [{ head: "TEST-SEAMS · ui/src/Panel.test.ts:40", fields: [{ key: "change", value: "Assert the rendered row." }] }],
          },
        ],
      }),
    }),
  });
  const status = screen.getByText(/^\s*RED$/);
  expect(status.classList.contains("tone-red")).toBe(true);
  expect(status.querySelector("svg")).toBeTruthy();
  expect(screen.getByText("3m 12s · 48.2k tokens · gpt-6.1-sol")).toBeTruthy();
  expect(screen.getByText("Check that the tests follow the test rules.")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Result" }).getAttribute("aria-expanded")).toBe("true");
  expect(screen.getByText("violations")).toBeTruthy();
  expect(screen.getByText("TEST-SEAMS · ui/src/Panel.test.ts:40")).toBeTruthy();
  expect(screen.getByText("Assert the rendered row.")).toBeTruthy();
});

it("marks a reused result REUSED in the status colour and shows what reuse saved", () => {
  render({ detail: detail({ mark: { status: "GREEN", glyph: "✓" }, card: card({ reused: true }) }) });
  const status = screen.getByText(/GREEN · REUSED/);
  expect(status.classList.contains("tone-green")).toBe(true);
  expect(screen.getByText("saved 3m 12s · saved 48.2k tokens · gpt-6.1-sol")).toBeTruthy();
});

it("folds execution options, how it ran, the fingerprint and Debug under their names, saying nothing twice", async () => {
  render({ detail: detail({ card: card({ tokens: 1_250_000 }) }) });
  expect(rows()).toEqual(["Execution options ▸", "Execution metadata ▸", "Fingerprint ▸", "Debug ▸"]);
  expect(screen.queryByText("timeoutMs 60000")).toBeNull();
  await fireEvent.click(screen.getByRole("button", { name: "Execution options" }));
  expect(screen.getByText("timeoutMs 60000")).toBeTruthy();
  await fireEvent.click(screen.getByRole("button", { name: "Execution metadata" }));
  expect(screen.getByText("complete · attempts 1")).toBeTruthy();
  await fireEvent.click(screen.getByRole("button", { name: "Fingerprint" }));
  expect(screen.getByText("9f3c1e")).toBeTruthy();
  // Debug holds the request's other facts, and nothing the Summary shows.
  expect(screen.queryByText("run-a-1")).toBeNull();
  await fireEvent.click(screen.getByRole("button", { name: "Debug" }));
  expect(screen.getByText("run-a-1")).toBeTruthy();
  expect(screen.getByText("k-1")).toBeTruthy();
  expect(screen.getAllByText("9f3c1e")).toHaveLength(1);
  expect(screen.getAllByText("complete · attempts 1")).toHaveLength(1);
});

it("keeps a section folded or unfolded by its name for the next Detail", async () => {
  const result = card({ result: [{ kind: "text", key: "summary", text: "All rules hold." }] });
  const first = render({ detail: detail({ card: result }) });
  await fireEvent.click(screen.getByRole("button", { name: "Fingerprint" }));
  await fireEvent.click(screen.getByRole("button", { name: "Result" }));
  first.unmount();
  render({ detail: detail({ node: "e:app/lint", card: result }) });
  expect(screen.getByRole("button", { name: "Fingerprint" }).getAttribute("aria-expanded")).toBe("true");
  expect(screen.getByRole("button", { name: "Result" }).getAttribute("aria-expanded")).toBe("false");
});

it("says a runtime eval's time and exit once, beside its status, without tokens or a model", () => {
  render({ detail: detail({ profile: "runtime", mark: { status: "GREEN", glyph: "✓" }, card: card({ exitCode: 0, tokens: null, model: null, took: "41s" }) }) });
  expect(screen.getByText("41s · exit 0")).toBeTruthy();
  expect(screen.queryByText(/tokens/)).toBeNull();
});

it("shows an Artifact's path, and its other facts only under Debug", async () => {
  render({
    detail: detail({
      node: "a:app",
      target: "artifact",
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
  expect(rows()).toEqual(["Path ▾", "Debug ▸"]);
  expect(screen.getByText("crates/app")).toBeTruthy();
  // Its tally and its evals are the graph's and the Evals rows': not said again here.
  expect(screen.queryByText("RED · 1 RED · 2 GREEN")).toBeNull();
  expect(screen.queryByText("a1b2")).toBeNull();
  await fireEvent.click(screen.getByRole("button", { name: "Debug" }));
  expect(screen.getByText("a1b2")).toBeTruthy();
});

it("leads a Run's Detail with its outcome and what it waits for, the rest folded", () => {
  render({
    detail: detail({
      node: "run:run-a",
      target: "run",
      title: "Run run-a",
      profile: null,
      card: null,
      mark: { status: "RED", glyph: "✗" },
      outcome: {
        failed: [{ mark: { status: "RED", glyph: "✗" }, text: "cli/tests" }],
        tally: [
          { mark: { status: "RED", glyph: "✗" }, count: 1 },
          { mark: { status: "GREEN", glyph: "✓" }, count: 4 },
        ],
        took: "2m 4s",
        finished: true,
        comment: "Fixing the login form",
      },
      sections: [
        { name: "waits", title: "Waits for", fields: [], waits: [{ text: "release", pending: false }, { text: "release/signoff", pending: true }] },
        { name: "provenance", title: "Provenance", fields: [{ key: "Timing", value: "started a · took 2m 4s" }], waits: [] },
        { name: "technical", title: "Technical", fields: [{ key: "Run", value: "run-a" }], waits: [] },
      ],
    }),
  });
  expect(rows()).toEqual(["Outcome ▾", "Waits for ▾", "Provenance ▸", "Technical ▸"]);
  expect(screen.getByText("cli/tests")).toBeTruthy();
  expect(screen.getByText("4 GREEN")).toBeTruthy();
  expect(screen.getByText("2m 4s")).toBeTruthy();
  expect(screen.getByText("Fixing the login form")).toBeTruthy();
  expect(screen.getByText("release/signoff").classList.contains("pending")).toBe(true);
  expect(screen.queryByText("run-a")).toBeNull();
});

it("says on a Current whether an eval would run or be reused and why, with the result it would reuse", () => {
  render({
    detail: detail({
      source: { kind: "current", branch: { kind: "branch", common: "/work/app/.git", branch: "main" } },
      mark: null,
      plan: { glyph: "↻", text: "reused", tone: "reused", reason: "same definition and inputs as run-a" },
      card: card({ reused: true, result: [{ kind: "text", key: "summary", text: "All rules hold." }] }),
    }),
  });
  const line = screen.getByText(/reused$/);
  expect(line.classList.contains("tone-reused")).toBe(true);
  expect(screen.getByText("same definition and inputs as run-a")).toBeTruthy();
  expect(screen.getByText("All rules hold.")).toBeTruthy();
  // The plan takes the status line's place: no Run status is shown.
  expect(screen.queryByText(/RED/)).toBeNull();
});
