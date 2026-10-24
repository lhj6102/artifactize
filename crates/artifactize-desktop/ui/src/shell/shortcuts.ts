// The keys of what has focus, for the help dialog.
type Section = { title: string; keys: [string, string][] };

const COMMON: Section = {
  title: "Window",
  keys: [
    ["Ctrl+K", "Command palette"],
    ["Ctrl+,", "Settings, and back"],
    ["Ctrl+O", "Open a request by ID"],
    ["/ · Ctrl+F", "Search branches and reviews"],
    ["Ctrl+R / F5", "Refresh, and scan the branches again"],
    ["?", "These shortcuts"],
    ["q", "Quit (outside fields)"],
    ["Tab / Shift+Tab", "Next / previous part or control"],
  ],
};

const HUMAN: Section = {
  title: "Human review",
  keys: [
    ["c", "Claim"],
    ["g / r", "GREEN / RED form, after claim"],
    ["u", "Release the claim"],
    ["Ctrl+G / Ctrl+R", "Switch verdict while editing"],
    ["Ctrl+S", "Submit the form"],
    ["Ctrl+U", "Release while editing"],
    ["Click · Enter", "Run a tool (its ▶ button)"],
    ["Tab", "Move between tools and fields"],
    ["i / t", "Expand instruction / Technical"],
    ["Esc", "Cancel work, stop editing, then leave"],
  ],
};

const GRAPH: Section = {
  title: "Graph and Detail",
  keys: [
    ["Click an Artifact", "Open its Detail; another Artifact switches it"],
    ["Click the headline", "Open the Run's Detail"],
    ["Click the background", "Close the Detail"],
    ["Tab · Enter", "Focus an Artifact · open its Detail"],
    ["Click a tab or an eval", "Show the Artifact or that eval"],
    ["Click ▶ under Tools", "Run one of the Artifact's tools in its worktree"],
    ["t / w", "Debug / pending evals"],
    ["Drag the left edge", "Resize the Detail"],
    ["Esc", "Close the Detail, then back to the Pane"],
    [
      "Colours",
      "Run: green GREEN · yellow running or waiting for a Human · grey waiting · red RED · purple ERROR · dark red blocked. Current: teal reused · blue would run · grey waiting · dark red blocked · pale grey checking",
    ],
  ],
};

export const HELP: Record<"pane" | "main" | "settings", Section[]> = {
  pane: [
    {
      title: "Pane",
      keys: [
        ["↑ / ↓ · j / k", "Select a branch or a timeline entry, then a waiting review"],
        ["← / →", "Fold / expand the selected branch, or step out of / into its timeline"],
        ["Enter", "Go to the Main area, or open the selected review"],
        ["Click a branch · an entry", "Expand it and show its newest entry · show that entry"],
        ["Older runs", "Bring older Runs in below the oldest"],
      ],
    },
    {
      title: "Search",
      keys: [
        ["text · key:value", "Every term must match; keys repo branch sort"],
        ["sort:recent · sort:name", "Most recent Run first · by project and branch"],
        ["↑ / ↓ · Tab / Enter", "Choose / take a suggestion"],
        ["Esc", "Close suggestions, then put sort:recent back, then back to the entries"],
      ],
    },
    COMMON,
  ],
  main: [GRAPH, HUMAN, COMMON],
  settings: [
    {
      title: "Settings",
      keys: [
        ["Providers", "Codex, OpenAI, Anthropic, Gemini and the custom providers; choose one to set it up"],
        ["Codex", "Sign in through the browser as `artifactize login codex`; paste the redirect URL if it cannot return"],
        ["Keys", "Save or remove an API key, as `artifactize login`; a set environment variable is used first"],
        ["+ Custom provider", "Name an alias of a built-in provider or an endpoint, as `artifactize provider add`"],
        ["Models", "List what the chosen provider offers, as `artifactize models`"],
        ["Remote", "Sign in with a store URL and token, check status, sign out or push, as `artifactize remote`"],
        ["← / →", "Switch between the Providers and Remote tabs"],
        ["Esc", "Close Settings"],
        ["Tab / Shift+Tab", "Next / previous control"],
      ],
    },
    COMMON,
  ],
};
