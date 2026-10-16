// The keys each screen answers to, for the help dialog.
type Section = { title: string; keys: [string, string][] };

const COMMON: Section = {
  title: "Window",
  keys: [
    ["Ctrl+K", "Command palette"],
    ["Ctrl+1 / Ctrl+2", "Monitor / Review"],
    ["Ctrl+,", "Settings"],
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

export const HELP: Record<"monitor" | "review" | "settings" | "graph", Section[]> = {
  graph: [
    { title: "Current Artifact graph", keys: [["← / →", "Upstream / downstream Artifact"], ["↑ / ↓", "Artifact above / below; select an eval in the popup"], ["Enter / click", "Pop up an Artifact's evals"], ["Enter / click on eval", "Expand structure and evaluation criteria"], ["Esc", "Close the evals"], ["Ctrl+R / F5", "Reload current declarations"]] },
    COMMON,
  ],
  monitor: [
    {
      title: "Navigate",
      keys: [
        ["↑ / ↓ · j / k", "Select a scope or a Run"],
        ["Enter / →", "Next level: Scope, Runs, the Run's graph"],
        ["← / Esc", "Step back"],
        ["Space", "Worktrees without Runs"],
        ["b / Backspace", "Upstream blocker / back"],
        ["!", "Next ERROR, RED or waiting Human eval"],
      ],
    },
    {
      title: "Run graph",
      keys: [
        ["← / →", "Upstream / downstream Artifact"],
        ["↑ / ↓", "Artifact above / below in the same column"],
        ["Enter / click", "Pop up the Artifact's evals"],
        ["↑ / ↓ · Enter / click", "In the evals: select (its summary shows) / open Detail"],
        ["o", "Open the Detail of the selected Artifact or eval"],
        ["Click the headline", "Open the Run's Detail"],
        ["Esc", "Close the evals, then step back"],
        [
          "Colours",
          "green GREEN · yellow running or waiting for a Human · grey waiting · red RED · purple ERROR · dark red blocked by an upstream RED, ERROR or block",
        ],
      ],
    },
    {
      title: "Detail",
      keys: [
        ["t / w / d", "Technical / pending evals / Agent details"],
        ["Esc / ←", "Close the Detail"],
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
        ["Right-click · Ctrl+K", "Open a Human review with Tools focused (palette: Open the selected request with Tools focused)"],
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
  settings: [
    {
      title: "Settings",
      keys: [
        ["Codex", "Sign in through the browser as `artifactize login codex`; paste the redirect URL if it cannot return"],
        ["Keys", "OPENAI_API_KEY and ANTHROPIC_API_KEY come from the environment only; shown as set or not"],
        ["Models", "List what a backend offers, as `artifactize models`"],
        ["Remote", "Sign in with a store URL and token, check status, sign out or push, as `artifactize remote`"],
        ["Tab / Shift+Tab", "Next / previous control"],
      ],
    },
    COMMON,
  ],
};
