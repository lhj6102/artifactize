// How a tool the agent used reads in the conversation, from its recorded name and arguments.
import { describe, expect, it } from "vitest";
import { toolLine } from "./tool-line";

describe("toolLine", () => {
  it("reads a file tool as a verb and its path, in the present tense while it runs", () => {
    expect(toolLine("read", '{"path":"checkout/total.ts"}', false)).toEqual({ icon: "file", verb: "Read", target: "checkout/total.ts", lines: null });
    expect(toolLine("read", '{"path":"checkout/total.ts"}', true).verb).toBe("Reading");
  });

  it("counts the lines an edit added and removed", () => {
    const edit = toolLine("edit", JSON.stringify({ path: "a.ts", old_text: "x", new_text: "y\nz\nw" }), false);
    expect(edit).toEqual({ icon: "edit", verb: "Edited", target: "a.ts", lines: "+3 −1" });
  });

  it("reads a command as its program and arguments", () => {
    expect(toolLine("run_command", '{"program":"pnpm","args":["test","checkout"]}', false).target).toBe("pnpm test checkout");
  });

  it("reads a Verify tool by its name, and arguments that are not an object as no target", () => {
    expect(toolLine("verify_status", '{"run":"run-9PLM3X"}', false)).toEqual({ icon: "verify", verb: "verify status", target: "run-9PLM3X", lines: null });
    expect(toolLine("grep", "not json", false)).toEqual({ icon: "folder", verb: "Searched", target: "", lines: null });
  });
});
