import { createContext } from "svelte";

/** The starred Artifacts of a branch's project, as the library keeps them: replaced whole on
 * every read and every star. */
export class ArtifactStarState {
  /** The names starred, with the branch (by its key) they were read for. */
  starred = $state.raw<{ branch: string; names: string[] } | null>(null);
  /** Why reading or keeping a star failed. */
  error = $state<string | null>(null);
  /** Numbers the reads and stars asked: only the newest answer shows, so a read answered after
   * a click never puts back what the click changed. */
  asked = 0;
}

export const [getArtifactStarState, setArtifactStarState] = createContext<ArtifactStarState>();
