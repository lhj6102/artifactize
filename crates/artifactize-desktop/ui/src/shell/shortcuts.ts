// The keys of what has focus, for Help: the Pane, a graph with its Detail, a Human review or
// Settings; a graph's Help adds what its marks mean. Every key a working screen does not show
// is named here or in the palette.
import type { Light } from "../ipc/client";
import { CURRENT_LEGEND, LEGEND } from "../monitor/graph";

export type Section = { title: string; keys: [string, string][] };

/** What has focus, as Help lists its keys. */
export type Focused = "pane" | "review-pane" | "chat" | "graph" | "current" | "review" | "settings";

const WINDOW: Section = {
  title: "Window",
  keys: [
    ["Ctrl+K / F1", "Command palette"],
    ["Ctrl+1 / 2 / 3", "Show Chat, Verify or Review"],
    ["Ctrl+B", "Show or hide the Pane"],
    ["Ctrl+,", "Settings, and back"],
    ["Ctrl+O", "Open a request by ID"],
    ["/", "Search the branches"],
    ["Ctrl+F", "Search the branches outside a graph"],
    ["Ctrl+R / F5", "Refresh"],
    ["?", "These shortcuts"],
    ["q", "Quit"],
  ],
};

const COMPOSER: Section = {
  title: "Conversation",
  keys: [["Enter", "Send"], ["Shift+Enter", "Break a line"], ["Esc", "Stop the running turn"]],
};

const GRAPH: Section = {
  title: "Graph",
  keys: [
    ["Tab", "Next Artifact"],
    ["Enter", "Open its Detail"],
    ["Ctrl+F", "Find an Artifact or an eval"],
    ["m", "Minimize or expand a whole"],
    ["t", "Show or hide Debug"],
    ["Home / End", "Top or bottom of an Agent log"],
    ["j / k", "Scroll the Agent log down or up"],
    ["Esc", "Close the Detail, then back to the Pane"],
  ],
};

/** A Current's definitions, edited on its graph and in its Detail. */
const EDITING: Section = {
  title: "Editing",
  keys: [
    ["Enter", "Write a Path, a part or a comment"],
    ["Shift+Enter", "Break a comment's line"],
    ["Ctrl+S", "Save the Source"],
    ["Esc", "Put back what is being dragged, or a Path, a part or a comment being typed"],
  ],
};

const REVIEW: Section = {
  title: "Review",
  keys: [
    ["c", "Claim to review"],
    ["u", "Release the claim"],
    ["g / r", "Open the GREEN or the RED form"],
    ["Ctrl+S", "Submit"],
    ["Ctrl+Page Up / Ctrl+Page Down", "Scroll the review instruction"],
    ["t", "Show or hide Debug"],
    ["Esc", "Cancel running work, then leave the form, then the review"],
  ],
};

const KEYS: Record<Focused, Section[]> = {
  pane: [
    {
      title: "Pane",
      keys: [
        ["↑ / ↓ · j / k", "Select a branch or a timeline entry"],
        ["← / →", "Fold or expand the branch, or step out of or into its timeline"],
        ["Enter", "Go to the Main area"],
      ],
    },
    {
      title: "Search",
      keys: [
        ["↑ / ↓", "Choose a suggestion"],
        ["Tab / Enter", "Take it"],
        ["Esc", "Close suggestions, then put sort:recent back, then back to the entries"],
      ],
    },
    WINDOW,
  ],
  "review-pane": [
    { title: "Reviews", keys: [["↑ / ↓ · j / k", "Move between reviews"], ["Enter", "Open the selected review"]] },
    { title: "Filter", keys: [["↑ / ↓", "Choose a suggestion"], ["Tab / Enter", "Take it"], ["Esc", "Close suggestions, then restore type:human waiting, then back to the reviews"]] },
    WINDOW,
  ],
  chat: [COMPOSER, WINDOW],
  graph: [GRAPH, WINDOW],
  current: [GRAPH, EDITING, COMPOSER, WINDOW],
  review: [REVIEW, WINDOW],
  settings: [
    {
      title: "Settings",
      keys: [
        ["← / →", "Previous or next tab"],
        ["Esc", "Close Settings"],
      ],
    },
    WINDOW,
  ],
};

/** The keys Help lists for what has focus, and on a graph the marks it explains. */
export function help(focused: Focused): { sections: Section[]; marks: (Light | "human")[] } {
  return { sections: KEYS[focused], marks: focused === "graph" ? [...LEGEND, "human"] : focused === "current" ? CURRENT_LEGEND : [] };
}
