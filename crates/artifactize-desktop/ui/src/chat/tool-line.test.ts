import { expect, it } from "vitest";
import { toolLine, type Tool } from "./tool-line";

const activity = (name: string, summary: string): Tool => ({
  kind: "tool", callId: null, title: null, toolKind: "other", status: "completed", name, summary, arguments: "{}",
  failed: null, output: null, exitCode: null, omittedLines: 0, outputTruncated: false, locations: [], diff: null,
});

it("presents the library summary rather than deriving activity from tool arguments", () => {
  expect(toolLine(activity("read", "read checkout/total.ts"), true)).toEqual({ icon: "file", verb: "Reading", target: "checkout/total.ts" });
  expect(toolLine(activity("edit", "edit a.ts +3 −1"), false)).toEqual({ icon: "edit", verb: "Edited", target: "a.ts +3 −1" });
  expect(toolLine(activity("run_command", "ran pnpm test checkout"), false).target).toBe("pnpm test checkout");
  expect(toolLine({ ...activity("verify_status", "verify status"), toolKind: "verify", arguments: '{"run":"run-fixture"}' }, false))
    .toEqual({ icon: "verify", verb: "Checked", target: "run-fixture" });
});
