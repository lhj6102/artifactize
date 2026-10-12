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
    HUMAN,
    COMMON,
  ],
  review: [
    {
      title: "Waiting reviews",
      keys: [
        ["↑ / ↓ · j / k", "Select a request"],
        ["Enter / →", "Open its review"],
        ["Tab", "Open it with Tools focused"],
        ["Ctrl+O", "Open a request by ID"],
      ],
    },
    HUMAN,
    COMMON,
  ],
};
