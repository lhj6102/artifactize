import { createContext } from "svelte";
import type { DetailView, TreeNode } from "../ipc/client";

export class DetailPaneState {
  detail = $state.raw<DetailView | null>(null);
  /** The row of the node the Detail is of, in the Run or Current it was read from (by its key). */
  row = $state.raw<{ source: string; view: TreeNode } | null>(null);
  error = $state<string | null>(null);
  folds = $state({ node: null as string | null, technical: false, pending: false, log: false, debug: false });
}

export const [getDetailPaneState, setDetailPaneState] = createContext<DetailPaneState>();
