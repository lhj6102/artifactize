// The keys each screen answers to, for the help dialog.
type Section = { title: string; keys: [string, string][] };

const COMMON: Section = {
  title: "Window",
  keys: [
    ["Ctrl+K", "Command palette"],
    ["Ctrl+1 / Ctrl+2", "Monitor / Review"],
    ["Ctrl+R / F5", "Refresh"],
    ["?", "These shortcuts"],
    ["q", "Quit (outside fields)"],
    ["Tab / Shift+Tab", "Next / previous pane or control"],
  ],
};

const HUMAN: Section = {
  title: "Human Detail",
  keys: [
    ["c", "Claim"],
    ["g / r", "GREEN / RED form, after claim"],
    ["u", "Release the claim"],
    ["Ctrl+G / Ctrl+R", "Switch verdict while editing"],
    ["Ctrl+S", "Submit the form"],
    ["Ctrl+U", "Release while editing"],
    ["Enter", "Run the selected tool (Tools focused)"],
    ["↑ / ↓ · j / k", "Select a tool"],
    ["i / t", "Expand instruction / Technical"],
    ["Esc", "Cancel work, stop editing, then leave"],
  ],
};

export const HELP: Record<"monitor" | "review", Section[]> = {
  monitor: [
    {
      title: "Navigate",
      keys: [
        ["↑ / ↓ · j / k", "Select a row"],
        ["Enter / →", "Next level; on a tree row, open Detail"],
        ["o / double-click", "Open the tree row's Detail"],
        ["← / Esc", "Step back"],
        ["Space", "Worktrees without Runs; fold a tree row"],
        ["h / l", "Fold / unfold"],
        ["b / Backspace", "Upstream blocker / back"],
        ["!", "Next ERROR, RED or waiting Human eval"],
        ["t / w / d", "Technical / pending evals / Agent details"],
      ],
    },
    {
      title: "Run graph",
      keys: [
        ["v", "Switch the Run between tree and graph"],
        ["← / →", "Upstream / downstream Artifact"],
        ["↑ / ↓", "Artifact above / below in the same column"],
        ["Enter / click", "Zoom into the Artifact: its evals"],
        ["↑ / ↓ · Enter", "In the evals: select (Detail follows) / open Detail"],
        ["Esc", "Zoom out, then step back"],
        [
          "Colours",
          "green GREEN · yellow running or waiting for a Human · grey waiting · red RED · purple ERROR · dark red blocked by an upstream RED, ERROR or block",
        ],
      ],
    },
    HUMAN,
    COMMON,
  ],
  review: [
    {
      title: "Requests",
      keys: [
        ["↑ / ↓ · j / k", "Select a request"],
        ["Enter / →", "Open it: a Human review, or any other request read-only"],
        ["Tab", "Open a Human review with Tools focused"],
        ["PgDn / PgUp", "Older / newer page"],
        ["Ctrl+O", "Open a request by ID"],
      ],
    },
    {
      title: "Search",
      keys: [
        ["/ · Ctrl+F", "Search requests"],
        ["text · key:value", "Every term must match; keys repo eval request reviewer tag kind status is"],
        ["↑ / ↓ · Tab / Enter", "Choose / take a suggestion"],
        ["Esc", "Close suggestions, then clear, then back to the list"],
      ],
    },
    HUMAN,
    COMMON,
  ],
};
