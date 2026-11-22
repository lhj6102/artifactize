// Missing setup is reported by Settings' IPC, separately from a failed model listing.
import { call, message, type Commands, type Model, type Provider, type Providers } from "../ipc/client";

export type MenuListing =
  | { kind: "loading" | "unconfigured" }
  | { kind: "models"; models: Model[] }
  | { kind: "failed"; error: string };

export type MenuProvider = { provider: string; listing: MenuListing };

export function providerEntries(listed: Providers): [string, Provider | null][] {
  return [
    ...listed.builtin.map((provider): [string, Provider] => [provider, provider]),
    ...listed.custom.map(({ name, target }): [string, Provider | null] => [name, target.kind === "alias" ? target.provider : null]),
  ];
}

export function readProvider(ipc: Commands, listed: Providers) {
  const setup = new Map<Provider, Promise<boolean>>();
  const configured = async (provider: Provider) => {
    switch (provider) {
      case "codex":
        return (await call(ipc.codexStatus())).state !== "absent";
      case "claude-agent": {
        const { state } = await call(ipc.claudeAgentStatus());
        return state !== "missing" && state !== "signedOut";
      }
      case "antigravity": {
        const { state } = await call(ipc.antigravityStatus());
        return state !== "missing" && state !== "signedOut";
      }
      default:
        return listed.keys.find((key) => key.provider === provider)?.source !== "absent";
    }
  };
  return async (name: string, provider: Provider | null): Promise<MenuListing> => {
    try {
      if (provider !== null) {
        let ready = setup.get(provider);
        if (!ready) {
          ready = configured(provider).catch(() => true);
          setup.set(provider, ready);
        }
        // When the setup probe fails, give the listing a chance to report its own reason.
        if (!(await ready)) return { kind: "unconfigured" };
      }
      return { kind: "models", models: await call(ipc.modelsList(name)) };
    } catch (failure) {
      return { kind: "failed", error: message(failure) };
    }
  };
}
