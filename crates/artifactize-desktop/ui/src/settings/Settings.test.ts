// Settings against a mocked command boundary: the Providers tab (key sources without values,
// saved keys, custom providers, the Codex and Claude Code sign-in states and model listings), the Remote tab
// (the store's masked token, which leaves the screen's state when submitted) and the Limits tab
// (the sign-off idle grace the next Runs take).
import { renderHosted } from "../test-host";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import Settings from "./Settings.svelte";
import type { AntigravityPrompt, AntigravityStatus, ClaudeAgentStatus, CodexStatus, CustomProvider, KeySource, NativeStart, Provider, RemoteStatus } from "../ipc/client";

const STORE = "http://127.0.0.1:8417/";
const TOKEN = "azt_secret-store-token";
const KEY = "sk-secret-provider-key";
const AGY = { path: "/fixture/bin/agy", version: "1.5.14", requiredVersion: "1.5.1" };
const AGY_OUT: AntigravityStatus = { ...AGY, state: "signedOut" };
const AGY_IN: AntigravityStatus = { ...AGY, state: "signedIn", account: null };
const AGY_URL = "https://accounts.google.com/o/oauth2/v2/auth?state=test";

/** The tests' clock: local noon of a fixed day, so a midnight never falls inside a test. */
const NOON = new Date(2026, 10, 14, 12).getTime();

const none: RemoteStatus = { state: "unconfigured" };
const signedIn: RemoteStatus = {
  state: "configured",
  url: STORE,
  share: "summary",
  tokenSource: "file",
  connection: { state: "signedIn", principal: "alice-laptop", scopes: ["read", "publish"] },
  // The library reports the tally independently of the visible ledger window.
  pushedToday: 5,
  pushes: [
    { at: NOON - 2 * 86400000, count: 9 },
    { at: NOON - 3600000, count: 2 },
    { at: NOON, count: 3 },
  ],
};

type Calls = { cmd: string; args: Record<string, unknown> }[];

function backend(options: {
  codex?: () => CodexStatus;
  wait?: () => Promise<unknown>;
  login?: (token: string) => RemoteStatus | Error;
  claude?: () => ClaudeAgentStatus;
  claudeComplete?: ClaudeAgentStatus;
  claudeWait?: () => Promise<unknown>;
  antigravity?: () => AntigravityStatus;
  antigravityPrompt?: () => Promise<NativeStart<AntigravityPrompt, AntigravityStatus>>;
  antigravityWait?: () => Promise<AntigravityStatus>;
}) {
  const calls: Calls = [];
  let agy = AGY_OUT as AntigravityStatus;
  const agyStatus = () => options.antigravity?.() ?? agy;
  let remote = none;
  let grace = 120;
  const environment = new Set(["openai"]);
  const saved = new Set<string>();
  const source = (provider: string): KeySource => (environment.has(provider) ? "environment" : saved.has(provider) ? "saved" : "absent");
  let custom: CustomProvider[] = [];
  const VARIABLES: Record<string, string> = { openai: "OPENAI_API_KEY", anthropic: "ANTHROPIC_API_KEY", gemini: "GEMINI_API_KEY" };
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: payload });
    switch (cmd) {
      case "providers_list":
        return {
          builtin: ["codex", "claude-agent", "openai", "anthropic", "gemini"],
          aliasTargets: ["codex", "openai", "anthropic", "gemini"],
          keys: Object.keys(VARIABLES).map((provider) => ({
            provider: provider as Provider,
            variable: VARIABLES[provider],
            source: source(provider),
            saved: saved.has(provider),
          })),
          custom,
        };
      case "chat_agents_list":
        return [
          {
            id: "antigravity",
            name: "Antigravity",
            tier: "preset",
            command: "agy",
            args: ["acp"],
            env: {},
            secretEnvKeys: [],
            mcpStdio: true,
            canDisableProjectConfig: true,
            nativeSandboxNote: "Runs natively in the worktree via agy acp.",
            loginAction: "artifactize login antigravity",
            installHint: "Install agy.",
            registry: null,
          },
        ];
      case "chat_trust_list":
        return [];
      case "chat_agent_probe":
        if (agyStatus().state !== "signedIn") {
          return {
            kind: "notSetUp",
            executable: "/fixture/bin/agy",
            version: "1.5.14",
            reason: "Not signed in.",
            loginAction: "artifactize login antigravity",
          };
        }
        return {
          kind: "ready",
          executable: "/fixture/bin/agy",
          version: "1.5.14",
          models: [{ slug: "gemini-demo", displayName: "Gemini Demo", efforts: [], defaultEffort: null }],
          configOptions: [
            {
              id: "model",
              name: "Model",
              category: "model",
              currentValue: "gemini-demo",
              choices: [{ value: "gemini-demo", name: "Gemini Demo", description: null, bypassWarning: false }],
            },
          ],
        };
      case "provider_key_save":
        saved.add(payload.provider as string);
        return null;
      case "provider_key_remove":
        saved.delete(payload.provider as string);
        return null;
      case "provider_custom_save": {
        const name = payload.name as string;
        // As the library refuses it: a provider needs a name.
        if (name === "") throw { kind: "invalid", message: "A custom provider needs a name." };
        const target = payload.target as CustomProvider["target"];
        const key = payload.key as { kind: string };
        const saved = custom.find((one) => one.name === name)?.target;
        const hasKey = key.kind === "set" || (key.kind === "keep" && saved?.kind === "endpoint" && !!saved.key);
        custom = [...custom.filter((one) => one.name !== name), { name, target: target.kind === "endpoint" ? { ...target, key: hasKey } : target }];
        return null;
      }
      case "provider_custom_remove":
        custom = custom.filter((one) => one.name !== payload.name);
        return null;
      case "codex_status":
        return options.codex?.() ?? { state: "absent" };
      case "codex_login_start":
        return 7;
      case "codex_login_prompt":
        return { url: "http://127.0.0.1:9/oauth/authorize?state=s", callback: false, signIn: 7 };
      case "codex_login_wait":
        return options.wait?.() ?? { state: "stored", expiry: { state: "usable", expiresAt: "1893456000" } };
      case "codex_login_paste":
      case "codex_login_cancel":
        return null;
      case "codex_logout":
        return { revoked: true, status: { state: "absent" } };
      case "claude_agent_status":
        return options.claude?.() ?? { state: "signedOut" };
      case "claude_agent_login_start":
        return 9;
      case "claude_agent_login_prompt":
        return options.claudeComplete ?? { url: "https://claude.example/oauth/authorize?code=true", signIn: 9 };
      case "claude_agent_login_wait":
        return options.claudeWait?.() ?? { state: "signedIn", method: "claude.ai", plan: "max" };
      case "claude_agent_login_paste":
      case "claude_agent_login_cancel":
        return null;
      case "claude_agent_logout":
        return { state: "signedOut" };
      case "antigravity_status":
        return agyStatus();
      case "antigravity_login_start":
        return 11;
      case "antigravity_login_prompt":
        return options.antigravityPrompt?.().then((started) => {
          if ("state" in started) agy = started;
          return started;
        }) ?? { url: AGY_URL, signIn: 11 };
      case "antigravity_login_wait":
        return options.antigravityWait?.().then((status) => (agy = status)) ?? (agy = AGY_IN);
      case "antigravity_login_paste":
      case "antigravity_login_cancel":
      case "chat_open_link":
        return null;
      case "antigravity_logout":
        return (agy = AGY_OUT);
      case "models_list":
        if (payload.provider === "antigravity") {
          if (agyStatus().state === "signedOut") return { kind: "notSetUp", reason: "Not signed in." };
          return { kind: "models", models: [{ slug: "gemini-demo", displayName: "Gemini Demo", efforts: [], defaultEffort: null }] };
        }
        if (payload.provider === "anthropic") return { kind: "failed", error: "The provider refused to list models." };
        return { kind: "models", models: [{ slug: "gpt-fake", displayName: "GPT Fake" }] };
      case "remote_status":
        return remote;
      case "remote_login": {
        const result = options.login?.(payload.token as string) ?? signedIn;
        if (result instanceof Error) throw { kind: "account", message: result.message };
        remote = result;
        return result;
      }
      case "remote_push":
        if (!payload.dryRun && remote.state === "configured") remote = { ...remote, pushedToday: remote.pushedToday + 2, pushes: [...remote.pushes, { at: NOON, count: 2 }] };
        return { pushed: 2, existing: 1, skipped: 0 };
      case "remote_logout":
        remote = none;
        return none;
      case "signoff_grace":
        return { seconds: grace, maxSeconds: 86400 };
      case "signoff_grace_set": {
        // The library's own refusal, as the IPC returns it.
        const seconds = payload.seconds as number;
        if (!Number.isInteger(seconds) || seconds < 0 || seconds > 86400) {
          throw { kind: "setting", message: "The sign-off idle grace must be whole seconds, at most 86400." };
        }
        grace = seconds;
        return { seconds: grace, maxSeconds: 86400 };
      }
      default:
        return null;
    }
  });
  return calls;
}

beforeEach(() => {
  // Only the date is frozen: the timers the tests wait with keep running.
  vi.useFakeTimers({ toFake: ["Date"], now: NOON });
});

afterEach(() => {
  vi.useRealTimers();
  cleanup();
  clearMocks();
});

describe("Settings", () => {
  it("lists the built-in providers, shows where each key comes from, never a value, and the sign-ins", async () => {
    backend({});
    renderHosted(Settings, { active: true });
    const list = await screen.findByRole("navigation", { name: "Providers" });
    await waitFor(() =>
      expect([...list.querySelectorAll("button")].map((button) => button.textContent)).toEqual([
        "Codex",
        "Claude Agent",
        "OpenAI",
        "Anthropic",
        "Gemini",
        "Custom provider",
      ]),
    );
    await waitFor(() => expect(screen.getByTestId("codex-status").textContent?.trim()).toBe("Not signed in"));
    await fireEvent.click(screen.getByRole("button", { name: "OpenAI" }));
    await waitFor(() => expect(screen.getByTestId("key-openai").textContent?.trim()).toBe("set from the environment (OPENAI_API_KEY)"));
    await fireEvent.click(screen.getByRole("button", { name: "Anthropic" }));
    expect(screen.getByTestId("key-anthropic").textContent?.trim()).toBe("not set");
    expect(screen.getByLabelText<HTMLInputElement>("API key").type).toBe("password");
    // No store: the fields that sign in to one, and nothing said about the normal state.
    await fireEvent.click(screen.getByRole("tab", { name: "Remote" }));
    await screen.findByRole("button", { name: "Sign in to the store" });
    expect(screen.queryByTestId("remote-status")).toBeNull();
  });

  it("saves a pasted key without keeping or showing it, then removes it", async () => {
    const calls = backend({});
    renderHosted(Settings, { active: true });
    await fireEvent.click(await screen.findByRole("button", { name: "Gemini" }));
    const field = screen.getByLabelText<HTMLInputElement>("API key");
    await fireEvent.input(field, { target: { value: KEY } });
    await fireEvent.click(screen.getByRole("button", { name: "Save key" }));
    expect(field.value).toBe("");
    await waitFor(() => expect(screen.getByTestId("key-gemini").textContent?.trim()).toBe("saved here"));
    expect(calls.find((call) => call.cmd === "provider_key_save")?.args).toEqual({ provider: "gemini", key: KEY });
    expect(document.body.innerHTML).not.toContain(KEY);
    await fireEvent.click(screen.getByRole("button", { name: "Remove saved key" }));
    await waitFor(() => expect(screen.getByTestId("key-gemini").textContent?.trim()).toBe("not set"));
  });

  it("keeps a set variable first, still showing and removing a key saved beside it", async () => {
    backend({});
    renderHosted(Settings, { active: true });
    await fireEvent.click(await screen.findByRole("button", { name: "OpenAI" }));
    await waitFor(() => expect(screen.getByTestId("key-openai").textContent?.trim()).toBe("set from the environment (OPENAI_API_KEY)"));
    expect(screen.queryByRole("button", { name: "Remove saved key" })).toBeNull();
    await fireEvent.input(screen.getByLabelText<HTMLInputElement>("API key"), { target: { value: KEY } });
    await fireEvent.click(screen.getByRole("button", { name: "Save key" }));
    await waitFor(() =>
      expect(screen.getByTestId("key-openai").textContent?.trim()).toBe("set from the environment (OPENAI_API_KEY); a saved key is kept but not used"),
    );
    await fireEvent.click(screen.getByRole("button", { name: "Remove saved key" }));
    await waitFor(() => expect(screen.getByTestId("key-openai").textContent?.trim()).toBe("set from the environment (OPENAI_API_KEY)"));
    expect(screen.queryByRole("button", { name: "Remove saved key" })).toBeNull();
  });

  it("adds custom providers by name, keeps a saved endpoint key, lists their models and removes them", async () => {
    const calls = backend({});
    renderHosted(Settings, { active: true });
    await fireEvent.click(await screen.findByRole("button", { name: "Custom provider" }));
    await fireEvent.input(screen.getByLabelText("Name"), { target: { value: "local" } });
    await fireEvent.input(screen.getByLabelText("Base URL"), { target: { value: "http://localhost:11434/v1" } });
    const key = screen.getByLabelText<HTMLInputElement>("Key");
    await fireEvent.input(key, { target: { value: KEY } });
    await fireEvent.click(screen.getByRole("button", { name: "Save" }));
    // Saved, it is listed by its name and its form shows what was saved.
    await waitFor(() => expect(screen.getByRole("navigation", { name: "Providers" }).textContent).toContain("local"));
    expect(key.value).toBe("");
    expect(calls.find((call) => call.cmd === "provider_custom_save")?.args).toEqual({
      name: "local",
      target: { kind: "endpoint", api: "openaiChat", url: "http://localhost:11434/v1" },
      key: { kind: "set", key: KEY },
    });
    expect(document.body.innerHTML).not.toContain(KEY);
    // Saved again with an empty key field, the saved key is kept.
    await waitFor(() => expect(screen.getByLabelText<HTMLInputElement>("Key").placeholder).toBe("Saved; type to replace"));
    await fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(calls.filter((call) => call.cmd === "provider_custom_save")).toHaveLength(2));
    expect(calls.filter((call) => call.cmd === "provider_custom_save")[1].args.key).toEqual({ kind: "keep" });
    await waitFor(() => expect(screen.getByRole("button", { name: "Save" }).hasAttribute("disabled")).toBe(false));

    await fireEvent.click(screen.getByRole("button", { name: "Custom provider" }));
    await fireEvent.input(screen.getByLabelText("Name"), { target: { value: "work" } });
    await fireEvent.click(screen.getByLabelText("alias of a built-in provider"));
    await fireEvent.change(screen.getByLabelText("Provider"), { target: { value: "openai" } });
    await fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(screen.getByRole("navigation", { name: "Providers" }).textContent).toContain("work"));
    expect(calls.filter((call) => call.cmd === "provider_custom_save").at(-1)?.args).toEqual({
      name: "work",
      target: { kind: "alias", provider: "openai" },
      key: { kind: "none" },
    });
    const list = screen.getByRole("navigation", { name: "Providers" });
    expect([...list.querySelectorAll("button")].map((button) => button.textContent)).toContain("work");

    await fireEvent.click(await screen.findByRole("button", { name: "List models" }));
    await screen.findByText("GPT Fake");
    expect(calls.filter((call) => call.cmd === "models_list").at(-1)?.args).toEqual({ provider: "work" });

    await fireEvent.click(screen.getByRole("button", { name: "local" }));
    await fireEvent.click(screen.getByRole("button", { name: "Remove provider" }));
    await waitFor(() => expect([...list.querySelectorAll("button")].map((button) => button.textContent)).not.toContain("local"));
  });

  it("signs in to Codex with a pasted redirect under Finish by hand, then signs out", async () => {
    let finish: (status: CodexStatus) => void = () => {};
    const calls = backend({ wait: () => new Promise((done) => (finish = done)) });
    renderHosted(Settings, { active: true });
    await fireEvent.click(await screen.findByRole("button", { name: "Sign in with Codex" }));
    // In progress, said in the account's fact, with Cancel sign-in; finishing by hand is folded.
    await waitFor(() => expect(screen.getByTestId("codex-status").textContent?.trim()).toBe("sign-in in progress"));
    expect(screen.getByRole("button", { name: "Cancel sign-in" })).toBeTruthy();
    expect(screen.queryByText("http://127.0.0.1:9/oauth/authorize?state=s")).toBeNull();
    expect(calls.find((call) => call.cmd === "codex_login_start")?.args).toEqual({ browser: true });
    // The wait names the sign-in the start returned.
    await waitFor(() => expect(calls.find((call) => call.cmd === "codex_login_wait")?.args).toEqual({ signIn: 7 }));
    await fireEvent.click(screen.getByRole("button", { name: "Finish by hand" }));
    await screen.findByText("http://127.0.0.1:9/oauth/authorize?state=s");
    // The port was taken: only a pasted URL finishes it.
    expect(screen.getByText(/Port 1455 is in use/)).not.toBeNull();
    const field = screen.getByLabelText("Redirect URL");
    await fireEvent.input(field, { target: { value: " http://localhost:1455/auth/callback?code=c&state=s " } });
    await fireEvent.click(screen.getByRole("button", { name: "Finish sign-in" }));
    expect(calls.find((call) => call.cmd === "codex_login_paste")?.args).toEqual({
      signIn: 7,
      redirect: "http://localhost:1455/auth/callback?code=c&state=s",
    });
    finish({ state: "stored", expiry: { state: "usable", expiresAt: "1893456000" } });
    await waitFor(() => expect(screen.getByTestId("codex-status").textContent).toMatch(/^\s*Signed in/));
    expect(screen.queryByRole("button", { name: "Cancel sign-in" })).toBeNull();
    await fireEvent.click(screen.getByRole("button", { name: "Sign out of Codex" }));
    await waitFor(() => expect(screen.getByTestId("codex-status").textContent?.trim()).toBe("Not signed in"));
  });

  it("reports a cancelled sign-in as a fact and offers signing in again", async () => {
    let fail: (error: unknown) => void = () => {};
    let state: CodexStatus = { state: "absent" };
    const calls = backend({
      codex: () => state,
      wait: () => new Promise((_, reject) => (fail = reject)),
    });
    renderHosted(Settings, { active: true });
    await fireEvent.click(await screen.findByRole("button", { name: "Sign in with Codex" }));
    await waitFor(() => expect(calls.filter((call) => call.cmd === "codex_login_wait")).toHaveLength(1));
    await fireEvent.click(await screen.findByRole("button", { name: "Cancel sign-in" }));
    expect(calls.find((call) => call.cmd === "codex_login_cancel")?.args).toEqual({ signIn: 7 });
    fail({ kind: "account", message: "Codex sign-in was cancelled." });
    expect((await screen.findByText("Codex sign-in was cancelled.")).classList.contains("error")).toBe(true);
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Cancel sign-in" })).toBeNull();
      expect(screen.getByRole("button", { name: "Sign in with Codex" }).hasAttribute("disabled")).toBe(false);
    });
    state = { state: "file", path: "/home/a/.codex/auth.json", expiry: { state: "usable", expiresAt: null } };
    await fireEvent.click(screen.getByRole("button", { name: "Sign in with Codex" }));
    await waitFor(() => expect(calls.filter((call) => call.cmd === "codex_login_wait")).toHaveLength(2));
    await fireEvent.click(await screen.findByRole("button", { name: "Cancel sign-in" }));
    fail({ kind: "account", message: "Codex sign-in was cancelled." });
    // A read-only auth file is never changed here: no sign-in.
    await waitFor(() => expect(screen.getByTestId("codex-status").textContent).toMatch(/Read-only Codex auth file/));
    expect(screen.getByRole("button", { name: "Sign in with Codex" }).hasAttribute("disabled")).toBe(true);
  });

  it("shows the sign-off idle grace in force in its seconds field and saves a new one for later Runs", async () => {
    const calls = backend({});
    renderHosted(Settings, { active: true });
    await fireEvent.click(screen.getByRole("tab", { name: "Limits" }));
    const seconds = await screen.findByLabelText<HTMLInputElement>("Seconds");
    await waitFor(() => expect(seconds.value).toBe("120"));
    const save = screen.getByRole("button", { name: "Save" });
    // Unchanged, nothing can be saved; past the library's bound, the field keeps it from Save.
    expect(save.hasAttribute("disabled")).toBe(true);
    await fireEvent.input(seconds, { target: { value: "90000" } });
    await fireEvent.click(save);
    expect(calls.some((call) => call.cmd === "signoff_grace_set")).toBe(false);
    await fireEvent.input(seconds, { target: { value: "300" } });
    await fireEvent.click(save);
    await waitFor(() => expect(calls.filter((call) => call.cmd === "signoff_grace_set").at(-1)?.args).toEqual({ seconds: 300 }));
    // Saved: the field holds the new grace, and nothing is said about it.
    const limits = screen.getByRole("region", { name: "Sign-off idle grace" });
    await waitFor(() => expect(limits.getAttribute("aria-busy")).toBe("false"));
    expect(save.hasAttribute("disabled")).toBe(true);
    expect(seconds.value).toBe("300");
    expect(screen.queryByRole("status")).toBeNull();
    await fireEvent.input(seconds, { target: { value: "0" } });
    await waitFor(() => expect(save.hasAttribute("disabled")).toBe(false));
    await fireEvent.click(save);
    await waitFor(() => expect(calls.filter((call) => call.cmd === "signoff_grace_set").at(-1)?.args).toEqual({ seconds: 0 }));
  });

  it("asks the library to save a custom provider without a name, and shows its refusal", async () => {
    const calls = backend({});
    renderHosted(Settings, { active: true });
    await fireEvent.click(await screen.findByRole("button", { name: "Custom provider" }));
    await fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("A custom provider needs a name.");
    expect(calls.filter((call) => call.cmd === "provider_custom_save").map((call) => call.args.name)).toEqual([""]);
  });

  it("shows Claude Code's own sign-in and lists Claude Agent's models by its backend name", async () => {
    let state: ClaudeAgentStatus = { state: "missing" };
    const calls = backend({ claude: () => state });
    renderHosted(Settings, { active: true });
    await fireEvent.click(await screen.findByRole("button", { name: "Claude Agent" }));
    expect(screen.getByText("Login kept by").nextElementSibling?.textContent).toBe("Claude Code");
    expect(screen.queryByLabelText("API key")).toBeNull();
    await waitFor(() => expect(screen.getByTestId("claude-agent-status").textContent?.trim()).toBe("claude not found"));
    expect(screen.getByRole("button", { name: "Sign in with Claude Code" }).hasAttribute("disabled")).toBe(true);
    state = { state: "failed", error: "Claude Code could not report its sign-in: boom" };
    await fireEvent.click(screen.getByRole("button", { name: "Check again" }));
    await waitFor(() => expect(screen.getByTestId("claude-agent-status").textContent?.trim()).toBe("Claude Code could not report its sign-in: boom"));
    expect(screen.getByTestId("claude-agent-status").classList.contains("error")).toBe(true);
    state = { state: "signedIn", method: "claude.ai", plan: "max" };
    await fireEvent.click(screen.getByRole("button", { name: "Check again" }));
    await waitFor(() => expect(screen.getByTestId("claude-agent-status").textContent?.trim()).toBe("Signed in · claude.ai · max"));
    // Signed in, the facts stand alone.
    expect(screen.getByText("Login kept by").nextElementSibling?.textContent).toBe("Claude Code");
    expect(calls.filter((call) => call.cmd === "claude_agent_status")).toHaveLength(3);
    await fireEvent.click(screen.getByRole("button", { name: "List models" }));
    await screen.findByText("GPT Fake");
    expect(calls.filter((call) => call.cmd === "models_list").at(-1)?.args).toEqual({ provider: "claude-agent" });
  });

  it("signs Claude Code in with a pasted code, then out", async () => {
    let finish: (status: ClaudeAgentStatus) => void = () => {};
    const calls = backend({ claudeWait: () => new Promise((done) => (finish = done)) });
    renderHosted(Settings, { active: true });
    await fireEvent.click(await screen.findByRole("button", { name: "Claude Agent" }));
    await waitFor(() => expect(screen.getByTestId("claude-agent-status").textContent?.trim()).toBe("Not signed in"));
    await fireEvent.click(screen.getByRole("button", { name: "Sign in with Claude Code" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Finish by hand" }));
    await screen.findByText("https://claude.example/oauth/authorize?code=true");
    expect(calls.find((call) => call.cmd === "claude_agent_login_wait")?.args).toEqual({ signIn: 9 });
    const field = screen.getByLabelText("Code");
    await fireEvent.input(field, { target: { value: " abc#def " } });
    await fireEvent.click(screen.getByRole("button", { name: "Finish sign-in" }));
    expect(calls.find((call) => call.cmd === "claude_agent_login_paste")?.args).toEqual({ signIn: 9, code: "abc#def" });
    finish({ state: "signedIn", method: "claude.ai", plan: "max" });
    await waitFor(() => expect(screen.getByTestId("claude-agent-status").textContent?.trim()).toBe("Signed in · claude.ai · max"));
    expect(screen.queryByRole("button", { name: "Cancel sign-in" })).toBeNull();
    await fireEvent.click(screen.getByRole("button", { name: "Sign out of Claude Code" }));
    await waitFor(() => expect(screen.getByTestId("claude-agent-status").textContent?.trim()).toBe("Not signed in"));
  });

  it("cancels a Claude Code sign-in and shows the status Claude Code then reports", async () => {
    let fail: (error: unknown) => void = () => {};
    let state: ClaudeAgentStatus = { state: "signedOut" };
    const calls = backend({ claude: () => state, claudeWait: () => new Promise((_, reject) => (fail = reject)) });
    renderHosted(Settings, { active: true });
    await fireEvent.click(await screen.findByRole("button", { name: "Claude Agent" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Sign in with Claude Code" }));
    await waitFor(() => expect(calls.find((call) => call.cmd === "claude_agent_login_wait")?.args).toEqual({ signIn: 9 }));
    await fireEvent.click(await screen.findByRole("button", { name: "Cancel sign-in" }));
    expect(calls.find((call) => call.cmd === "claude_agent_login_cancel")?.args).toEqual({ signIn: 9 });
    // Claude Code was signed in elsewhere meanwhile: only a status read after the cancellation shows it.
    state = { state: "signedIn", method: "claude.ai", plan: "max" };
    fail({ kind: "account", message: "Claude Code sign-in was cancelled." });
    await waitFor(() => expect(screen.getByTestId("claude-agent-status").textContent?.trim()).toBe("Claude Code sign-in was cancelled."));
    expect(screen.queryByText("Signed in · claude.ai · max")).toBeNull();
    expect(screen.getByText("Login kept by").nextElementSibling?.textContent).toBe("Claude Code");
    await fireEvent.click(screen.getByRole("button", { name: "Check again" }));
    await waitFor(() => expect(screen.getByTestId("claude-agent-status").textContent?.trim()).toBe("Signed in · claude.ai · max"));
    expect(screen.queryByRole("button", { name: "Cancel sign-in" })).toBeNull();
  });

  it("shows each status Antigravity reports under Chat Agents and does not offer it as a custom alias", async () => {
    let state: AntigravityStatus = { ...AGY, path: null, version: null, state: "missing" };
    const calls = backend({ antigravity: () => state });
    renderHosted(Settings, { active: true });
    await fireEvent.click(screen.getByRole("tab", { name: "Chat Agents" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Antigravity" }));
    const account = screen.getByRole("region", { name: "Antigravity" });
    const fact = within(account).getByTestId("antigravity-status");
    await waitFor(() => expect(fact.textContent?.trim()).toBe("agy not found"));
    const signIn = within(account).getByRole("button", { name: "Sign in with Antigravity" });
    expect(signIn.hasAttribute("disabled")).toBe(true);
    expect(screen.queryByLabelText("API key")).toBeNull();

    const oldError = "Antigravity CLI 1.4.0 is too old; update `agy` to 1.5.1 or newer.";
    const transitions: [AntigravityStatus, string][] = [
      [{ ...AGY, state: "tooOld", version: "1.4.0", error: oldError }, oldError],
      [AGY_OUT, "Not signed in"],
      [AGY_IN, "Signed in with Google"],
      [{ ...AGY_IN, account: "reviewer@example.test" }, "Signed in with Google · reviewer@example.test"],
      [{ ...AGY, state: "quota", error: "Google account quota exhausted." }, "Google account quota exhausted."],
      [{ ...AGY, state: "rateLimited", error: "Antigravity rate limited this account." }, "Antigravity rate limited this account."],
      [{ ...AGY, state: "failed", error: "Antigravity's account probe timed out." }, "Antigravity's account probe timed out."],
    ];
    for (const [next, text] of transitions) {
      state = next;
      await fireEvent.click(within(account).getByRole("button", { name: "Check again" }));
      await waitFor(() => expect(fact.textContent?.trim()).toBe(text));
      expect(fact.classList.contains("error")).toBe("error" in state);
      expect(within(account).getByText("Antigravity CLI")).toBeTruthy();
      expect(within(account).getByText("Chat only")).toBeTruthy();
      if (state.state === "tooOld") {
        expect(within(account).getByText("/fixture/bin/agy · 1.4.0 · requires 1.5.1 or newer")).toBeTruthy();
        expect(signIn.hasAttribute("disabled")).toBe(true);
      } else {
        expect(within(account).getByText("/fixture/bin/agy · 1.5.14")).toBeTruthy();
      }
    }
    expect(calls.filter(({ cmd }) => cmd === "antigravity_status").length).toBeGreaterThanOrEqual(transitions.length + 1);

    await fireEvent.click(screen.getByRole("tab", { name: "Providers" }));
    const nav = screen.getByRole("navigation", { name: "Providers" });
    await fireEvent.click(within(nav).getByRole("button", { name: "Custom provider" }));
    await fireEvent.click(screen.getByRole("radio", { name: "alias of a built-in provider" }));
    const choices = within(screen.getByRole("combobox", { name: "Provider" })).getAllByRole("option");
    expect(choices.map((option) => option.textContent?.trim())).toEqual(["Codex", "OpenAI", "Anthropic", "Gemini"]);
  });

  it("opens the Antigravity prompt, signs in by hand, probes agent config, and signs out", async () => {
    let finish: (status: AntigravityStatus) => void = () => {};
    const calls = backend({ antigravityWait: () => new Promise((done) => (finish = done)) });
    renderHosted(Settings, { active: true });
    await fireEvent.click(screen.getByRole("tab", { name: "Chat Agents" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Antigravity" }));
    const fact = screen.getByTestId("antigravity-status");
    await waitFor(() => expect(fact.textContent?.trim()).toBe("Not signed in"));
    const probe = await screen.findByRole("button", { name: "Probe agent" });
    await fireEvent.click(probe);
    expect((await screen.findByText("Not signed in.")).classList.contains("muted")).toBe(true);

    await fireEvent.click(screen.getByRole("button", { name: "Sign in with Antigravity" }));
    const manual = await screen.findByRole("button", { name: "Finish by hand" });
    expect(fact.textContent?.trim()).toBe("sign-in in progress");
    expect(fact.classList.contains("warn")).toBe(true);
    expect(screen.queryByRole("button", { name: "Check again" })).toBeNull();
    expect(screen.queryByText(AGY_URL)).toBeNull();
    expect(screen.queryByLabelText("Code")).toBeNull();
    expect(calls.find(({ cmd }) => cmd === "chat_open_link")?.args).toEqual({ url: AGY_URL });
    expect(calls.find(({ cmd }) => cmd === "antigravity_login_wait")?.args).toEqual({ signIn: 11 });
    await fireEvent.click(manual);
    await screen.findByText(AGY_URL);
    const field = screen.getByRole<HTMLInputElement>("textbox", { name: "Code" });
    await fireEvent.input(field, { target: { value: " test-code " } });
    await fireEvent.click(screen.getByRole("button", { name: "Finish sign-in" }));
    expect(calls.find(({ cmd }) => cmd === "antigravity_login_paste")?.args).toEqual({ signIn: 11, code: "test-code" });
    expect(field.value).toBe("");
    finish(AGY_IN);

    const signOut = await screen.findByRole("button", { name: "Sign out of Antigravity" });
    expect(fact.textContent?.trim()).toBe("Signed in with Google");
    expect(screen.queryByRole("button", { name: "Cancel sign-in" })).toBeNull();
    expect(screen.queryByText("Not signed in.")).toBeNull();
    await fireEvent.click(probe);
    const modelsList = await screen.findByRole("list", { name: "Models" });
    expect(modelsList.textContent).toContain("Gemini Demo");
    expect(modelsList.textContent).toContain("gemini-demo");
    await fireEvent.click(signOut);
    await waitFor(() => expect(fact.textContent?.trim()).toBe("Not signed in"));
    expect(calls.filter(({ cmd }) => cmd === "antigravity_logout")).toHaveLength(1);
  });

  it.each([true, false])("cancels Antigravity while the CLI is pending (before prompt: %s)", async (beforePrompt) => {
    let fail: (failure: unknown) => void = () => {};
    const pending = <T,>() => new Promise<T>((_, reject) => (fail = reject));
    const calls = backend(beforePrompt ? { antigravityPrompt: pending } : { antigravityWait: pending });
    renderHosted(Settings, { active: true });
    await fireEvent.click(screen.getByRole("tab", { name: "Chat Agents" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Antigravity" }));
    await screen.findByText("Not signed in");
    await fireEvent.click(screen.getByRole("button", { name: "Sign in with Antigravity" }));
    if (!beforePrompt) await screen.findByRole("button", { name: "Finish by hand" });
    await fireEvent.click(await screen.findByRole("button", { name: "Cancel sign-in" }));
    expect(calls.find(({ cmd }) => cmd === "antigravity_login_cancel")?.args).toEqual({ signIn: 11 });
    fail({ kind: "account", message: "Antigravity sign-in was cancelled." });
    await waitFor(() => expect(screen.getByTestId("antigravity-status").textContent?.trim()).toBe("Antigravity sign-in was cancelled."));
    await waitFor(() => expect(screen.getByRole("button", { name: "Sign in with Antigravity" }).hasAttribute("disabled")).toBe(false));
    expect(screen.queryByRole("button", { name: "Finish by hand" })).toBeNull();
    expect(calls.filter(({ cmd }) => cmd === "antigravity_status").length).toBeGreaterThanOrEqual(2);
  });

  it.each([
    { name: "Antigravity", tab: "Chat Agents", provider: "Antigravity", prefix: "antigravity", fact: "Signed in with Google" },
    { name: "Claude Code", tab: "Providers", provider: "Claude Agent", prefix: "claude_agent", fact: "Signed in · claude.ai · max" },
  ])("shows $name signing in without ever needing a prompt", async ({ name, tab, provider, prefix, fact }) => {
    const calls = backend({
      antigravityPrompt: async () => AGY_IN,
      claudeComplete: { state: "signedIn", method: "claude.ai", plan: "max" },
    });
    renderHosted(Settings, { active: true });
    await fireEvent.click(screen.getByRole("tab", { name: tab }));
    await fireEvent.click(await screen.findByRole("button", { name: provider }));
    await screen.findByText("Not signed in");
    await fireEvent.click(screen.getByRole("button", { name: `Sign in with ${name}` }));
    await screen.findByText(fact);
    expect(screen.getByRole("button", { name: `Sign out of ${name}` })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Cancel sign-in" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Finish by hand" })).toBeNull();
    expect(calls.filter(({ cmd }) => cmd.startsWith(`${prefix}_login_`)).map(({ cmd }) => cmd)).toEqual([
      `${prefix}_login_start`, `${prefix}_login_prompt`,
    ]);
    expect(calls.some(({ cmd }) => cmd === "chat_open_link")).toBe(false);
  });

  it("lists models and shows the CLI's error for a provider without its key", async () => {
    backend({});
    renderHosted(Settings, { active: true });
    await fireEvent.click(await screen.findByRole("button", { name: "OpenAI" }));
    await fireEvent.click(screen.getByRole("button", { name: "List models" }));
    await screen.findByText("GPT Fake");
    await fireEvent.click(screen.getByRole("button", { name: "Anthropic" }));
    expect(screen.queryByText("GPT Fake")).toBeNull();
    await fireEvent.click(screen.getByRole("button", { name: "List models" }));
    await screen.findByText("The provider refused to list models.");
  });

  it("masks the store token and drops it from the screen once submitted, refused or not", async () => {
    let attempts = 0;
    const calls = backend({ login: () => (++attempts === 1 ? new Error("Invalid or revoked token.") : signedIn) });
    renderHosted(Settings, { active: true });
    await fireEvent.click(await screen.findByRole("tab", { name: "Remote" }));
    const token = await screen.findByLabelText<HTMLInputElement>("Token");
    expect(token.type).toBe("password");
    expect(token.autocomplete).toBe("off");
    await fireEvent.input(screen.getByLabelText("Store URL"), { target: { value: STORE } });
    await fireEvent.input(token, { target: { value: TOKEN } });
    await fireEvent.click(screen.getByRole("button", { name: "Sign in to the store" }));
    expect(token.value).toBe("");
    await screen.findByText("Invalid or revoked token.");
    await waitFor(() => expect(screen.getByRole("region", { name: "Shared review store" }).getAttribute("aria-busy")).toBe("false"));
    expect(document.body.innerHTML).not.toContain(TOKEN);

    await fireEvent.input(token, { target: { value: TOKEN } });
    await fireEvent.click(screen.getByLabelText("full"));
    await fireEvent.click(screen.getByRole("button", { name: "Sign in to the store" }));
    expect(token.value).toBe("");
    // Signed in: the store's URL and share level as facts, its status, and no fields that sign in.
    expect((await screen.findByTestId("remote-principal")).textContent).toBe("alice-laptop (scopes: read, publish)");
    expect(screen.getByTestId("remote-pushed").textContent).toBe("5 pushed today");
    expect(screen.getByTestId("remote-status").textContent).toBe(STORE);
    expect(screen.queryByLabelText("Token")).toBeNull();
    expect(calls.filter((call) => call.cmd === "remote_login").at(-1)?.args).toEqual({ url: STORE, share: "full", token: TOKEN });
    expect(document.body.innerHTML).not.toContain(TOKEN);

    await fireEvent.click(screen.getByRole("button", { name: "Push (dry run)" }));
    await screen.findByText(/Would push 2/);
    expect(screen.queryByTestId("remote-pushed")).toBeNull();
    await fireEvent.click(screen.getByRole("button", { name: "Push results" }));
    const report = await screen.findByText(/Pushed 2, already in the store 1/);
    await waitFor(() => expect(within(report).getByTestId("remote-pushed").textContent).toBe("7 pushed today"));
    await fireEvent.click(screen.getByRole("button", { name: "Check status" }));
    await screen.findByTestId("remote-principal");
    expect(screen.getByTestId("remote-pushed").textContent).toBe("7 pushed today");

    await fireEvent.click(screen.getByRole("button", { name: "Sign out of the store" }));
    // Signed out: the fields and the button that sign in come back.
    await screen.findByRole("button", { name: "Sign in to the store" });
    expect(screen.queryByTestId("remote-principal")).toBeNull();
  });

  it("never lets a slower status check show the store signed in after signing out", async () => {
    let answer: (status: RemoteStatus) => void = () => {};
    let slow = false;
    mockIPC((cmd) => {
      switch (cmd) {
        case "providers_list":
          return { builtin: [], keys: [], custom: [] };
        case "codex_status":
          return { state: "absent" };
        case "remote_status":
          return slow ? new Promise((done) => (answer = done)) : signedIn;
        case "remote_logout":
          return none;
        default:
          return null;
      }
    });
    renderHosted(Settings, { active: true });
    await fireEvent.click(await screen.findByRole("tab", { name: "Remote" }));
    await screen.findByTestId("remote-principal");
    slow = true;
    const store = screen.getByRole("region", { name: "Shared review store" });
    await waitFor(() => expect(store.getAttribute("aria-busy")).toBe("false"));
    await fireEvent.click(screen.getByRole("button", { name: "Check status" }));
    await waitFor(() => expect(store.getAttribute("aria-busy")).toBe("true"));
    await fireEvent.click(screen.getByRole("button", { name: "Sign out of the store" }));
    await screen.findByRole("button", { name: "Sign in to the store" });
    // Signing out finished, but the older check still owns an outstanding UI operation.
    expect(store.getAttribute("aria-busy")).toBe("true");
    answer(signedIn);
    // The normal remoteCall finally clears the rendered busy state only after it has
    // accepted or discarded the answer. Observe that boundary without replacing call().
    await waitFor(() => expect(store.getAttribute("aria-busy")).toBe("false"));
    expect(screen.queryByTestId("remote-principal")).toBeNull();
    expect(screen.getByRole("button", { name: "Sign in to the store" })).toBeTruthy();
  });
});
