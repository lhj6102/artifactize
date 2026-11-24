// D-CHAT-4: presentation of the library's activity labels and adjacent successful tools.
import type { ChatBlock } from "../ipc/client";
import type { IconName } from "../shell/icons";

export type Tool = Extract<ChatBlock, { kind: "tool" }>;
type Part = { kind: "block"; at: number; block: ChatBlock } | { kind: "group"; at: number; tools: Tool[] };
type Wording = { icon: IconName; past: string; present: string; noun: string; plural: string };
const words = (icon: IconName, past: string, present: string, noun: string, plural = `${noun}s`): Wording => ({ icon, past, present, noun, plural });
const wording = new Map([
  ["read", words("file", "Read", "Reading", "file")],
  ["list", words("folder", "Listed", "Listing", "folder")],
  ["glob", words("folder", "Found", "Finding", "pattern")],
  ["grep", words("folder", "Searched", "Searching", "folder")],
  ["write", words("edit", "Wrote", "Writing", "file")],
  ["edit", words("edit", "Edited", "Editing", "file")],
  ["verify_status", words("verify", "Checked", "Checking", "Run")],
  ["verify_wait", words("verify", "Waited for", "Waiting for", "Run")],
  ["verify_watch", words("verify", "Watched", "Watching", "Run")],
  ["verify_result", words("verify", "Read", "Reading", "Run result")],
  ["verify_cancel", words("verify", "Cancelled", "Cancelling", "Run")],
  ["run_command", words("command", "Ran", "Running", "command")],
]);

export function toolLine(tool: Tool, busy: boolean) {
  const known = wording.get(tool.name);
  const summary = tool.summary || tool.name;
  if (!known) return { icon: "verify" as const, verb: summary, target: "" };
  if (tool.toolKind === "verify") {
    let target = "";
    try {
      const args = JSON.parse(tool.arguments) as { run?: unknown } | null;
      if (typeof args?.run === "string") target = args.run;
    } catch { /* The library may preserve a truncated argument preview. */ }
    return { icon: known.icon, verb: busy ? known.present : known.past, target };
  }
  const space = summary.indexOf(" ");
  return { icon: known.icon, verb: busy ? known.present : known.past, target: space < 0 ? "" : summary.slice(space + 1) };
}

export function activity(blocks: ChatBlock[]): Part[] {
  const parts: Part[] = [];
  for (let at = 0; at < blocks.length;) {
    const start = at;
    while (at < blocks.length && routine(blocks[at])) at++;
    if (at - start >= 2) parts.push({ kind: "group", at: start, tools: blocks.slice(start, at).filter(routine) });
    else {
      parts.push({ kind: "block", at: start, block: blocks[start] });
      at = start + 1;
    }
  }
  return parts;
}

function routine(block: ChatBlock): block is Tool {
  return block.kind === "tool" && block.failed === false;
}

export function activityLabel(tools: Tool[]): string {
  const counts = new Map<string, number>();
  for (const tool of tools) counts.set(tool.name, (counts.get(tool.name) ?? 0) + 1);
  return [...counts].map(([name, count], index) => {
    const { past, noun, plural } = wording.get(name) ?? words("command", "Used", "Using", "tool");
    return `${index ? past.toLowerCase() : past} ${count} ${count === 1 ? noun : plural}`;
  }).join(" · ");
}
