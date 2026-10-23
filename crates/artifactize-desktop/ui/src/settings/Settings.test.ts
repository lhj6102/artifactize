// Settings against a mocked command boundary: the Providers tab (key sources without values,
// saved keys, custom providers, the Codex sign-in states and model listings) and the Remote tab
// (the store's masked token, which leaves the screen's state when submitted).
import { renderHosted } from "../test-host";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it } from "vitest";
import Settings from "./Settings.svelte";
import type { CodexStatus, CustomProvider, KeySource, Provider, RemoteStatus } from "../ipc/client";

const STORE = "http://127.0.0.1:8417/";
const TOKEN = "azt_secret-store-token";
const KEY = "sk-secret-provider-key";

const none: RemoteStatus = { state: "unconfigured" };
const signedIn: RemoteStatus = {
  state: "configured",
  url: STORE,
  share: "summary",
  tokenSource: "file",
  connection: { state: "signedIn", principal: "alice-laptop", scopes: ["read", "publish"] },
};

type Calls = { cmd: string; args: Record<string, unknown> }[];

function backend(options: {
  codex?: () => CodexStatus;
  wait?: () => Promise<unknown>;
  login?: (token: string) => RemoteStatus | Error;
}) {
  const calls: Calls = [];
  let remote = none;
  const keys: Record<string, KeySource> = { openai: "environment", anthropic: "absent", gemini: "absent" };
  let custom: CustomProvider[] = [];
  const VARIABLES: Record<string, string> = { openai: "OPENAI_API_KEY", anthropic: "ANTHROPIC_API_KEY", gemini: "GEMINI_API_KEY" };
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: payload });
    switch (cmd) {
      case "providers_list":
        return {
          keys: Object.entries(keys).map(([provider, source]) => ({ provider: provider as Provider, variable: VARIABLES[provider], source })),
          custom,
        };
      case "provider_key_save":
        keys[payload.provider as string] = "saved";
        return null;
      case "provider_key_remove":
        keys[payload.provider as string] = "absent";
        return null;
      case "provider_custom_save": {
        const name = payload.name as string;
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
        return { url: "http://127.0.0.1:9/oauth/authorize?state=s", callback: false, signIn: 7 };
      case "codex_login_wait":
        return options.wait?.() ?? { state: "stored", expiry: { state: "usable", expiresAt: "1893456000" } };
      case "codex_login_paste":
      case "codex_login_cancel":
        return null;
      case "codex_logout":
        return { revoked: true, status: { state: "absent" } };
      case "models_list":
        if (payload.provider === "anthropic") throw { kind: "account", message: "ANTHROPIC_API_KEY is required to list models." };
        return [{ slug: "gpt-fake", displayName: "GPT Fake" }];
      case "remote_status":
        return remote;
      case "remote_login": {
        const result = options.login?.(payload.token as string) ?? signedIn;
        if (result instanceof Error) throw { kind: "account", message: result.message };
        remote = result;
        return result;
      }
      case "remote_logout":
        remote = none;
        return none;
      default:
        return null;
    }
  });
  return calls;
}

afterEach(() => {
  cleanup();
  clearMocks();
});

describe("Settings", () => {
  it("lists the built-in providers, shows where each key comes from, never a value, and the sign-ins", async () => {
    backend({});
    renderHosted(Settings, { active: true });
    const list = await screen.findByRole("navigation", { name: "Providers" });
    expect([...list.querySelectorAll("button")].map((button) => button.textContent)).toEqual([
      "Codex (ChatGPT)",
      "OpenAI",
      "Anthropic",
      "Gemini",
      "+ Custom provider",
    ]);
    await waitFor(() => expect(screen.getByTestId("codex-status").textContent?.trim()).toBe("Not signed in"));
    await fireEvent.click(screen.getByRole("button", { name: "OpenAI" }));
    await waitFor(() => expect(screen.getByTestId("key-openai").textContent?.trim()).toBe("Set from OPENAI_API_KEY"));
    await fireEvent.click(screen.getByRole("button", { name: "Anthropic" }));
    expect(screen.getByTestId("key-anthropic").textContent?.trim()).toBe("Not set");
    expect(screen.getByLabelText<HTMLInputElement>("API key").type).toBe("password");
    await fireEvent.click(screen.getByRole("tab", { name: "Remote" }));
    expect((await screen.findByTestId("remote-status")).textContent).toBe("No remote review store is configured.");
  });

  it("saves a pasted key without keeping or showing it, then removes it", async () => {
    const calls = backend({});
    renderHosted(Settings, { active: true });
    await fireEvent.click(await screen.findByRole("button", { name: "Gemini" }));
    const field = screen.getByLabelText<HTMLInputElement>("API key");
    await fireEvent.input(field, { target: { value: KEY } });
    await fireEvent.click(screen.getByRole("button", { name: "Save key" }));
    expect(field.value).toBe("");
    await screen.findByText("Saved the key.");
    expect(calls.find((call) => call.cmd === "provider_key_save")?.args).toEqual({ provider: "gemini", key: KEY });
    await waitFor(() => expect(screen.getByTestId("key-gemini").textContent?.trim()).toBe("Saved"));
    expect(document.body.innerHTML).not.toContain(KEY);
    await fireEvent.click(screen.getByRole("button", { name: "Remove saved key" }));
    await screen.findByText("Removed the saved key.");
    await waitFor(() => expect(screen.getByTestId("key-gemini").textContent?.trim()).toBe("Not set"));
  });

  it("adds custom providers by name, keeps a saved endpoint key, lists their models and removes them", async () => {
    const calls = backend({});
    renderHosted(Settings, { active: true });
    await fireEvent.click(await screen.findByRole("button", { name: "+ Custom provider" }));
    await fireEvent.input(screen.getByLabelText("Name"), { target: { value: "local" } });
    await fireEvent.input(screen.getByLabelText("URL"), { target: { value: "http://localhost:11434/v1" } });
    const key = screen.getByLabelText<HTMLInputElement>("Key");
    await fireEvent.input(key, { target: { value: KEY } });
    await fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("Saved local.");
    expect(key.value).toBe("");
    expect(calls.find((call) => call.cmd === "provider_custom_save")?.args).toEqual({
      name: "local",
      target: { kind: "endpoint", api: "openaiChat", url: "http://localhost:11434/v1" },
      key: { kind: "set", key: KEY },
    });
    expect(document.body.innerHTML).not.toContain(KEY);
    // Saved again with an empty key field, the saved key is kept.
    expect(screen.getByLabelText<HTMLInputElement>("Key").placeholder).toBe("Saved; type to replace");
    await fireEvent.click(screen.getByRole("button", { name: "Save" }));
    // The notice clears when the save starts and returns once the form shows what was saved.
    expect(screen.queryByText("Saved local.")).toBeNull();
    await screen.findByText("Saved local.");
    expect(calls.filter((call) => call.cmd === "provider_custom_save")[1].args.key).toEqual({ kind: "keep" });

    await fireEvent.click(screen.getByRole("button", { name: "+ Custom provider" }));
    await fireEvent.input(screen.getByLabelText("Name"), { target: { value: "work" } });
    await fireEvent.click(screen.getByLabelText("a built-in provider"));
    await fireEvent.change(screen.getByLabelText("Provider"), { target: { value: "openai" } });
    await fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("Saved work.");
    expect(calls.filter((call) => call.cmd === "provider_custom_save").at(-1)?.args).toEqual({
      name: "work",
      target: { kind: "alias", provider: "openai" },
      key: { kind: "none" },
    });
    const list = screen.getByRole("navigation", { name: "Providers" });
    expect([...list.querySelectorAll("button")].map((button) => button.textContent)).toContain("work");

    await fireEvent.click(screen.getByRole("button", { name: "List models" }));
    await screen.findByText("GPT Fake");
    expect(calls.filter((call) => call.cmd === "models_list").at(-1)?.args).toEqual({ provider: "work" });

    await fireEvent.click(screen.getByRole("button", { name: "local" }));
    await fireEvent.click(screen.getByRole("button", { name: "Remove provider" }));
    await screen.findByText("Removed local.");
    expect([...list.querySelectorAll("button")].map((button) => button.textContent)).not.toContain("local");
  });

  it("signs in to Codex with a pasted redirect, then signs out", async () => {
    let finish: (status: CodexStatus) => void = () => {};
    const calls = backend({ wait: () => new Promise((done) => (finish = done)) });
    renderHosted(Settings, { active: true });
    await fireEvent.click(await screen.findByRole("button", { name: "Sign in with ChatGPT" }));
    await screen.findByText("http://127.0.0.1:9/oauth/authorize?state=s");
    expect(calls.find((call) => call.cmd === "codex_login_start")?.args).toEqual({ browser: true });
    // The wait names the sign-in the start returned.
    expect(calls.find((call) => call.cmd === "codex_login_wait")?.args).toEqual({ signIn: 7 });
    // The port was taken: only a pasted URL finishes it.
    expect(screen.getByText(/Port 1455 is in use/)).not.toBeNull();
    const field = screen.getByLabelText(/paste the URL it was sent to/);
    expect(screen.getByText(/Finish signing in with the browser, then paste/)).not.toBeNull();
    expect(field.getAttribute("aria-describedby")).toBe("codex-redirect-help");
    expect(screen.getByText(/from DevTools → Network/).textContent).toContain("http://localhost:1455/auth/callback?code=");
    await fireEvent.input(field, { target: { value: " http://localhost:1455/auth/callback?code=c&state=s " } });
    await fireEvent.click(screen.getByRole("button", { name: "Finish sign-in" }));
    expect(calls.find((call) => call.cmd === "codex_login_paste")?.args).toEqual({
      redirect: "http://localhost:1455/auth/callback?code=c&state=s",
    });
    finish({ state: "stored", expiry: { state: "usable", expiresAt: "1893456000" } });
    await screen.findByText("Signed in with Codex.");
    expect(screen.getByTestId("codex-status").textContent).toMatch(/Signed in/);
    expect(screen.queryByText(/Waiting for the browser/)).toBeNull();
    await fireEvent.click(screen.getByRole("button", { name: "Sign out of Codex" }));
    await screen.findByText("Signed out of Codex.");
    expect(screen.getByTestId("codex-status").textContent?.trim()).toBe("Not signed in");
  });

  it("reports a cancelled sign-in and offers signing in again when signed in", async () => {
    let fail: (error: unknown) => void = () => {};
    let state: CodexStatus = { state: "stored", expiry: { state: "usable", expiresAt: "1893456000" } };
    const calls = backend({
      codex: () => state,
      wait: () => new Promise((_, reject) => (fail = reject)),
    });
    renderHosted(Settings, { active: true });
    await fireEvent.click(await screen.findByRole("button", { name: "Sign in again" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Cancel sign-in" }));
    expect(calls.some((call) => call.cmd === "codex_login_cancel")).toBe(true);
    fail({ kind: "account", message: "Codex sign-in was cancelled." });
    await screen.findByText("Codex sign-in was cancelled.");
    // The message shows before the status refresh and the busy reset finish: retry only once
    // the prompt closed and signing in again is offered.
    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Cancel sign-in" })).toBeNull();
      expect(screen.getByRole("button", { name: "Sign in again" }).hasAttribute("disabled")).toBe(false);
    });
    state = { state: "file", path: "/home/a/.codex/auth.json", expiry: { state: "usable", expiresAt: null } };
    await fireEvent.click(screen.getByRole("button", { name: "Sign in again" }));
    await fireEvent.click(await screen.findByRole("button", { name: "Cancel sign-in" }));
    fail({ kind: "account", message: "Codex sign-in was cancelled." });
    // A read-only auth file is never changed here: no sign-in or sign-out.
    await waitFor(() => expect(screen.getByTestId("codex-status").textContent).toMatch(/Read-only Codex auth file/));
    expect(screen.getByRole("button", { name: "Sign in with ChatGPT" }).hasAttribute("disabled")).toBe(true);
    expect(screen.getByRole("button", { name: "Sign out of Codex" }).hasAttribute("disabled")).toBe(true);
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
    await screen.findByText("ANTHROPIC_API_KEY is required to list models.");
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
    await waitFor(() => expect(screen.getByRole("region", { name: "Shared remote review store" }).getAttribute("aria-busy")).toBe("false"));
    expect(document.body.innerHTML).not.toContain(TOKEN);

    await fireEvent.input(token, { target: { value: TOKEN } });
    await fireEvent.click(screen.getByLabelText("full"));
    await fireEvent.click(screen.getByRole("button", { name: "Sign in to the store" }));
    expect(token.value).toBe("");
    await screen.findByText(`Signed in to ${STORE} as alice-laptop.`);
    expect(screen.getByTestId("remote-principal").textContent).toBe("alice-laptop (scopes: read, publish)");
    expect(calls.filter((call) => call.cmd === "remote_login").at(-1)?.args).toEqual({ url: STORE, share: "full", token: TOKEN });
    expect(document.body.innerHTML).not.toContain(TOKEN);

    await fireEvent.click(screen.getByRole("button", { name: "Sign out of the store" }));
    await screen.findByText(/Signed out of the remote review store/);
    expect(screen.getByTestId("remote-status").textContent).toBe("No remote review store is configured.");
  });

  it("never lets a slower status check show the store signed in after signing out", async () => {
    let answer: (status: RemoteStatus) => void = () => {};
    let slow = false;
    mockIPC((cmd) => {
      switch (cmd) {
        case "providers_list":
          return { keys: [], custom: [] };
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
    const store = screen.getByRole("region", { name: "Shared remote review store" });
    await waitFor(() => expect(store.getAttribute("aria-busy")).toBe("false"));
    await fireEvent.click(screen.getByRole("button", { name: "Check status" }));
    await waitFor(() => expect(store.getAttribute("aria-busy")).toBe("true"));
    await fireEvent.click(screen.getByRole("button", { name: "Sign out of the store" }));
    await screen.findByText(/Signed out of the remote review store/);
    // Signing out finished, but the older check still owns an outstanding UI operation.
    expect(store.getAttribute("aria-busy")).toBe("true");
    answer(signedIn);
    // The normal remoteCall finally clears the rendered busy state only after it has
    // accepted or discarded the answer. Observe that boundary without replacing call().
    await waitFor(() => expect(store.getAttribute("aria-busy")).toBe("false"));
    expect(screen.getByTestId("remote-status").textContent).toBe("No remote review store is configured.");
  });
});
