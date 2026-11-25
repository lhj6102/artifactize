// Target & model menu helpers: ready ACP Chat agents and direct HTTP API providers.
import {
  call,
  message,
  type ChatAgentAvailability,
  type ChatAgentCatalogEntry,
  type ChatConfigOption,
  type ChatDirectBackend,
  type ChatTarget,
  type Commands,
  type ModelsResult,
  type Providers,
} from "../ipc/client";
import { providerName } from "../settings/provider-names";

export type MenuListing = ModelsResult | { kind: "loading" };
export type MenuProvider = { provider: string; directBackend: ChatDirectBackend; listing: MenuListing };
export type MenuAgent = { entry: ChatAgentCatalogEntry; availability: ChatAgentAvailability | { kind: "loading" } };

/** Only direct HTTP providers can be Chat direct targets; `claude-agent` is eval-only. */
export function providerEntries(listed: Providers): { provider: string; directBackend: ChatDirectBackend }[] {
  const builtin = listed.builtin
    .filter((provider) => provider !== "claude-agent")
    .map((provider) => ({
      provider,
      directBackend: { kind: "builtin" as const, provider },
    }));
  const custom = listed.custom
    .filter((entry) => !(entry.target.kind === "alias" && entry.target.provider === "claude-agent"))
    .map(({ name }) => ({
      provider: name,
      directBackend: { kind: "custom" as const, name },
    }));
  return [...builtin, ...custom];
}

export async function readProvider(ipc: Commands, name: string): Promise<ModelsResult> {
  try {
    return await call(ipc.modelsList(name));
  } catch (failure) {
    return { kind: "failed", error: message(failure) };
  }
}

export async function readAgent(ipc: Commands, id: string): Promise<ChatAgentAvailability> {
  try {
    return await call(ipc.chatAgentProbe(id, false));
  } catch (failure) {
    return { kind: "failed", executable: null, version: null, error: message(failure) };
  }
}

export function directBackendName(backend: ChatDirectBackend): string {
  return backend.kind === "builtin" ? backend.provider : backend.name;
}

function displayOptionValue(
  item: { id: string; value: string; category?: string | null } | undefined,
  options: readonly ChatConfigOption[],
): string | undefined {
  if (!item) return undefined;
  const option = options.find(
    (opt) => opt.id === item.id || (item.category != null && opt.category === item.category),
  );
  const choice = option?.choices.find((each) => each.value === item.value);
  return choice?.name ?? item.value;
}

function acpSummary(
  target: Extract<ChatTarget, { engine: "acp" }>,
  agents: readonly ChatAgentCatalogEntry[],
  options: readonly ChatConfigOption[],
): string {
  const entry = agents.find((agent) => agent.id === target.agent);
  const name = entry?.name ?? target.agent;
  const config = target.config ?? [];
  const model = displayOptionValue(config.find((item) => item.category === "model" || item.id === "model"), options);
  const mode = displayOptionValue(config.find((item) => item.category === "mode" || item.id === "mode"), options);
  const thought = displayOptionValue(
    config.find((item) => item.category === "thought_level" || item.id === "thought_level"),
    options,
  );
  return [name, model, mode, thought].filter(Boolean).join(" · ");
}

/** Full target summary including direct provider name (used in New chat dialog). */
export function targetSummary(
  target: ChatTarget,
  agents: readonly ChatAgentCatalogEntry[] = [],
  options: readonly ChatConfigOption[] = [],
): string {
  if (target.engine === "acp") return acpSummary(target, agents, options);
  if (target.engine === "legacyClaudeAgent") {
    return ["Legacy Claude Agent", target.model, target.effort].filter(Boolean).join(" · ");
  }
  const backend = directBackendName(target.backend);
  return [providerName(backend), target.model, target.effort].filter(Boolean).join(" · ");
}

/** Compact trigger label (J-CHAT-MODEL): direct model + effort, or ACP agent + active config. */
export function triggerSummary(
  target: ChatTarget,
  agents: readonly ChatAgentCatalogEntry[] = [],
  options: readonly ChatConfigOption[] = [],
): string {
  if (target.engine === "acp") return acpSummary(target, agents, options);
  if (target.engine === "legacyClaudeAgent") {
    return ["Legacy Claude Agent", target.model, target.effort].filter(Boolean).join(" · ");
  }
  return [target.model, target.effort].filter(Boolean).join(" · ");
}
