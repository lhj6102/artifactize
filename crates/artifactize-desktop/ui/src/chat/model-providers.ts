// The menu presents the model-listing result reported by the library, with a local loading row.
import { call, message, type Commands, type ModelsResult, type Providers } from "../ipc/client";

export type MenuListing = ModelsResult | { kind: "loading" };
export type MenuProvider = { provider: string; listing: MenuListing };

export function providerEntries(listed: Providers): string[] {
  return [...listed.builtin, ...listed.custom.map(({ name }) => name)];
}

export async function readProvider(ipc: Commands, name: string): Promise<ModelsResult> {
  try { return await call(ipc.modelsList(name)); }
  catch (failure) { return { kind: "failed", error: message(failure) }; }
}
