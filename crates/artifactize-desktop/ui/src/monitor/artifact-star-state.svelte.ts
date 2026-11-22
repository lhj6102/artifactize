import { call, message, type BranchKey, type Commands } from "../ipc/client";

/** The project, not one of its checked-out branches, owns its Artifact stars. */
function projectOf(branch: BranchKey): string {
  return branch.kind === "folder" ? branch.path : branch.common;
}

type Stars = { names: string[]; error: string | null };
const EMPTY: Stars = { names: [], error: null };

export class ArtifactStarState {
  #projects = $state.raw<Record<string, Stars>>({});
  #reads: Record<string, number> = Object.create(null);
  #writes: Record<string, Promise<void> | undefined> = Object.create(null);
  constructor(readonly ipc: Commands) {}

  of(branch: BranchKey): Stars { return this.#projects[projectOf(branch)] ?? EMPTY; }

  async load(branch: BranchKey): Promise<void> {
    const project = projectOf(branch);
    if (this.#writes[project]) return;
    const read = this.#next(project);
    await this.#save(branch, read, () => call(this.ipc.starredArtifacts(branch)));
  }

  toggle(branch: BranchKey, artifact: string): void {
    const project = projectOf(branch);
    const previous = this.#writes[project] ?? Promise.resolve();
    const write = previous.then(() => this.#save(branch, this.#next(project), () =>
      call(this.ipc.starArtifact(branch, artifact, !this.of(branch).names.includes(artifact)))));
    this.#writes[project] = write;
    void write.finally(() => { if (this.#writes[project] === write) delete this.#writes[project]; });
  }

  #next(project: string): number {
    return this.#reads[project] = (this.#reads[project] ?? 0) + 1;
  }

  async #save(branch: BranchKey, read: number, fetch: () => Promise<string[]>): Promise<void> {
    const project = projectOf(branch);
    let next: Stars;
    try { next = { names: await fetch(), error: null }; }
    catch (failure) { next = { ...this.of(branch), error: message(failure) }; }
    if (this.#reads[project] === read) this.#projects = { ...this.#projects, [project]: next };
  }
}
