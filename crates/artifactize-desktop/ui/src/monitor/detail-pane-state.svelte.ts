import { createContext } from "svelte";
import type { DetailView } from "../ipc/client";

export class DetailPaneState {
  detail = $state.raw<DetailView | null>(null);
  error = $state<string | null>(null);
  folds = $state({ node: null as string | null, technical: false, pending: false, log: false });
}

export const [getDetailPaneState, setDetailPaneState] = createContext<DetailPaneState>();
