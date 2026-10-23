// Settings against a mocked command boundary: key presence without values, the Codex sign-in
// states (prompt, pasted redirect, cancel, sign-out), model listings and their errors, and the
// remote store's masked token, which leaves the screen's state when submitted, and the sign-off
// idle grace the next Runs take.
import { renderHosted } from "../test-host";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it } from "vitest";
import Settings from "./Settings.svelte";
import type { CodexStatus, RemoteStatus } from "../ipc/client";

const STORE = "http://127.0.0.1:8417/";
const TOKEN = "azt_secret-store-token";

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
  let grace = 120;
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: payload });
    switch (cmd) {
      case "settings_keys":
        return [
          { provider: "openai", variable: "OPENAI_API_KEY", present: true },
          { provider: "anthropic", variable: "ANTHROPIC_API_KEY", present: false },
        ];
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

afterEach(() => {
  cleanup();
  clearMocks();
});

describe("Settings", () => {
  it("shows whether each API key is set, never a value, and the current sign-ins", async () => {
    backend({});
    renderHosted(Settings, { active: true });
    expect((await screen.findByTestId("key-openai")).textContent).toBe("Set");
    expect(screen.getByTestId("key-anthropic").textContent).toBe("Not set");
    await waitFor(() => expect(screen.getByTestId("codex-status").textContent?.trim()).toBe("Not signed in"));
    expect((await screen.findByTestId("remote-status")).textContent).toBe("No remote review store is configured.");
    expect(screen.queryByRole("textbox", { name: /key/i })).toBeNull();
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

  it("shows the sign-off idle grace in force and saves a new one for later Runs", async () => {
    const calls = backend({});
    renderHosted(Settings, { active: true });
    await waitFor(() => expect(screen.getByTestId("grace-status").textContent).toBe("2 minutes"));
    const seconds = screen.getByLabelText<HTMLInputElement>("Seconds");
    const save = screen.getByRole("button", { name: "Save" });
    expect(seconds.value).toBe("120");
    // Unchanged, nothing can be saved; past the library's bound, the field keeps it from Save.
    expect(save.hasAttribute("disabled")).toBe(true);
    await fireEvent.input(seconds, { target: { value: "90000" } });
    await fireEvent.click(save);
    expect(calls.some((call) => call.cmd === "signoff_grace_set")).toBe(false);
    expect(screen.getByTestId("grace-status").textContent).toBe("2 minutes");
    await fireEvent.input(seconds, { target: { value: "300" } });
    await fireEvent.click(save);
    await screen.findByText("Saved. Runs started from now on wait this long.");
    expect(calls.filter((call) => call.cmd === "signoff_grace_set").at(-1)?.args).toEqual({ seconds: 300 });
    expect(screen.getByTestId("grace-status").textContent).toBe("5 minutes");
    await fireEvent.input(seconds, { target: { value: "0" } });
    await fireEvent.click(save);
    await waitFor(() => expect(screen.getByTestId("grace-status").textContent).toBe("0 seconds"));
  });

  it("lists models and shows the CLI's error for a backend without its key", async () => {
    backend({});
    renderHosted(Settings, { active: true });
    await fireEvent.click(await screen.findByRole("button", { name: "List models" }));
    await screen.findByText("GPT Fake");
    await fireEvent.change(screen.getByLabelText("Backend"), { target: { value: "anthropic" } });
    await fireEvent.click(screen.getByRole("button", { name: "List models" }));
    await screen.findByText("ANTHROPIC_API_KEY is required to list models.");
  });

  it("masks the store token and drops it from the screen once submitted, refused or not", async () => {
    let attempts = 0;
    const calls = backend({ login: () => (++attempts === 1 ? new Error("Invalid or revoked token.") : signedIn) });
    renderHosted(Settings, { active: true });
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
        case "settings_keys":
          return [];
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
