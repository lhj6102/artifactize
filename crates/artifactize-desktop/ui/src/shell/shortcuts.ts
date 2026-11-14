// The keys of what has focus, for Help: the Pane, a graph with its Detail, a Human review or
// Settings; a graph's Help adds what its marks mean. Every key a working screen does not show
// is named here or in the palette.
import type { Light } from "../ipc/client";
import { CURRENT_LEGEND, LEGEND } from "../monitor/graph";

export type Section = { title: string; keys: [string, string][] };

/** What has focus, as Help lists its keys. */
export type Focused = "pane" | "graph" | "current" | "review" | "settings";

const WINDOW: Section = {
  title: "Window",
  keys: [
    ["Ctrl+K", "Command palette"],
    ["Ctrl+B", "Show or hide the Pane"],
    ["Ctrl+,", "Settings, and back"],
    ["Ctrl+O", "Open a request by ID"],
    ["/", "Search the branches"],
    ["Ctrl+R / F5", "Refresh"],
    ["?", "These shortcuts"],
    ["q", "Quit"],
  ],
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
    ["Esc", "Close the Detail, then back to the Pane"],
  ],
};

const REVIEW: Section = {
  title: "Review",
  keys: [
    ["c", "Claim to review"],
    ["u", "Release the claim"],
    ["g / r", "Open the GREEN or the RED form"],
    ["Ctrl+G / Ctrl+R", "Choose GREEN or RED while editing"],
    ["Ctrl+S", "Submit"],
    ["t", "Show or hide Debug"],
    ["Esc", "Cancel running work, then leave the form, then the review"],
  ],
};

const KEYS: Record<Focused, Section[]> = {
  pane: [
    {
      title: "Pane",
      keys: [
        ["↑ / ↓ · j / k", "Select a branch, a timeline entry or a waiting review"],
        ["← / →", "Fold or expand the branch, or step out of or into its timeline"],
        ["Enter", "Go to the Main area, or open the waiting review"],
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
  graph: [GRAPH, WINDOW],
  current: [GRAPH, WINDOW],
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
export function help(focused: Focused): { sections: Section[]; marks: Light[] } {
  return { sections: KEYS[focused], marks: focused === "graph" ? LEGEND : focused === "current" ? CURRENT_LEGEND : [] };
}
