// What one tool the agent used reads as in the conversation (D-CHAT-4): an icon of what it does,
// a verb (in the present tense while it runs) and what it acted on, and for an edit the lines it
// added and removed. The tool's name and arguments are the library's transcript, as recorded.
import type { IconName } from "../shell/icons";

export type ToolLine = { icon: IconName; verb: string; target: string; lines: string | null };

type Kind = { icon: IconName; done: string; running: string; target: (args: Record<string, unknown>) => string };

const text = (value: unknown): string => (typeof value === "string" ? value : "");
const path = (args: Record<string, unknown>) => text(args.path) || ".";

const KINDS: Record<string, Kind> = {
  read: { icon: "file", done: "Read", running: "Reading", target: path },
  list: { icon: "folder", done: "Listed", running: "Listing", target: path },
  glob: { icon: "folder", done: "Found", running: "Finding", target: (args) => text(args.pattern) || path(args) },
  grep: { icon: "folder", done: "Searched", running: "Searching", target: (args) => [text(args.pattern), text(args.path)].filter(Boolean).join(" · ") },
  write: { icon: "edit", done: "Wrote", running: "Writing", target: path },
  edit: { icon: "edit", done: "Edited", running: "Editing", target: path },
  run_command: {
    icon: "command",
    done: "Ran",
    running: "Running",
    target: (args) => [text(args.program), ...(Array.isArray(args.args) ? args.args.map(text) : [])].join(" "),
  },
};

/** The lines a text holds: an empty text holds none. */
const count = (value: unknown) => (text(value) === "" ? 0 : text(value).split("\n").length);

/** How the tool `name`, called with the JSON `argumentsJson`, reads; `running` while it has no
 * outcome yet. A tool of Verify reads as its name with what it acted on. */
export function toolLine(name: string, argumentsJson: string, running: boolean): ToolLine {
  let args: Record<string, unknown> = {};
  try {
    const parsed: unknown = JSON.parse(argumentsJson);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) args = parsed as Record<string, unknown>;
  } catch {
    // Arguments that are not an object show nothing as the tool's target.
  }
  const kind = KINDS[name];
  if (!kind) {
    const target = text(args.run) || text(args.artifact) || text(args.eval) || text(args.id) || "";
    return { icon: "verify", verb: name.replace(/_/g, " "), target, lines: null };
  }
  const lines = name === "edit" ? `+${count(args.new_text)} −${count(args.old_text)}` : name === "write" ? `+${count(args.text)}` : null;
  return { icon: kind.icon, verb: running ? kind.running : kind.done, target: kind.target(args), lines };
}
