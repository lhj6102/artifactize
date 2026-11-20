// The built-in providers' names as Settings and Chat's model menu show them; a custom provider
// shows by its own name.
import type { Provider } from "../ipc/client";

export const PROVIDER_NAMES: Record<Provider, string> = {
  codex: "Codex",
  "claude-agent": "Claude Agent",
  openai: "OpenAI",
  anthropic: "Anthropic",
  gemini: "Gemini",
};

/** A provider's name as shown: a built-in one's, else the custom provider's own. */
export const providerName = (provider: string): string => PROVIDER_NAMES[provider as Provider] ?? provider;
