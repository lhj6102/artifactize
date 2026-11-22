// The shown branch's definitions: one file per Artifact in `.artifactize/`, with each
// Artifact's place and comment, as the backend read them. Every edit names the text it was
// shown, so a file that changed on disk meanwhile refuses it, and after every edit, written or
// not, the definitions are read again and the Current is scanned again. Typed text in a Source
// is never overwritten: an edit on the graph that would write its file is refused until it is
// saved or reverted.
import { SvelteMap } from "svelte/reactivity";
import { call, commands, message, type BranchKey, type Commands, type DefinitionNode, type DefinitionsView, type EditCheck, type EditOperation, type EditRefusal } from "../ipc/client";
import { keyOf } from "./monitor-state.svelte";
import { annotationsOf, fieldLines, withField, type Annotations, type Measure } from "./annotations";
import { NODE } from "./graph";

/** Typed text in an Artifact's Source: the file's text it was typed over, and what it holds. */
export type Draft = { base: string; text: string };

/** Where an edit was made: in the Detail, its refusal is said there; on the graph, as the Main
 * area's notice. */
export type Where = "detail" | "graph" | "dialog";

/** A refusal in words, a sentence a person can read. */
export function refusalText(refusal: EditRefusal, location = true): string {
  switch (refusal.kind) {
    case "invalid":
      if (!location) {
        const detail = refusal.message.replace(/^line \d+(?:, column \d+)?:\s*/, "");
        return `${refusal.file}: ${detail}`;
      }
      return refusal.line !== null && !refusal.message.includes(`line ${refusal.line}`)
        ? `line ${refusal.line}${refusal.column !== null ? `, column ${refusal.column}` : ""}: ${refusal.message}`
        : refusal.message;
    case "changed":
      return `${refusal.file} changed on disk; nothing written.`;
    case "notAllowed":
      return refusal.reason;
    case "io":
      return refusal.message;
  }
}

/** Where in a Definition an edit was refused. */
export type Place = "path" | "parts" | "source";

/** What is being typed in an Artifact's Definition: its Path, a part being added with the
 * suggestion chosen by ↑ ↓, and why the last edit there wrote nothing. */
export class DefinitionForm {
  /** The Path being typed, until it is written or put back. */
  path = $state<string | null>(null);
  /** The Parts field's text while a part is being added. */
  adding = $state<string | null>(null);
  options = $state.raw<{ input: string; parts: string[] } | null>(null);
  chosen = $state(0);
  refused = $state<{ at: Place; text: string } | null>(null);
}

/** What an edit dialog holds: the name and Path typed, and why the last try wrote nothing and
 * which field that concerns. */
export class DialogForm {
  name = $state("");
  path = $state("");
  refused = $state<{ field: "name" | "path" | "delete"; text: string } | null>(null);
}

/** Nothing typed: what a Definition shows before anything is. */
const UNTYPED: Readonly<DefinitionForm> = new DefinitionForm();

export class Definitions {
  constructor(
    readonly ipc: Commands = commands,
    /** Scan the branch's Current again once an edit wrote its files. */
    readonly rescan: (branch: BranchKey) => void = () => {},
  ) {}

  /** The definitions of the branch last read. */
  view = $state.raw<DefinitionsView | null>(null);
  readonly #nodes = $derived(new SvelteMap(this.view?.nodes.map((node) => [node.name, node])));
  /** Why the definitions cannot be read now; the graph is drawn without places or comments. */
  error = $state<string | null>(null);
  /** Why the last edit made on the graph wrote nothing, for the Main area's notice. */
  notice = $state<string | null>(null);
  /** An edit is being written: no other starts meanwhile. */
  busy = $state(false);
  /** The comment field open under an Artifact's node, and where it was opened from: the
   * Definition's Comment button, or a click on the comment. */
  field = $state.raw<{ name: string; from: "button" | "graph"; keyboard: boolean } | null>(null);
  /** What the open comment field holds, from the comment as it opened. */
  fieldText = $state("");
  /** What each Artifact's Definition has typed, by name, while the app runs. */
  readonly forms = new SvelteMap<string, DefinitionForm>();
  /** What the open edit dialog holds. */
  readonly dialogForm = new DialogForm();
  /** The edit dialog open: New Artifact, or Rename or Delete of an Artifact. */
  dialog = $state.raw<{ kind: "new" } | { kind: "rename"; name: string } | { kind: "delete"; name: string } | null>(null);
  /** An Artifact just created or renamed, whose Detail opens once the graph draws it; a new
   * one is also brought into view, as a found Artifact is. */
  created = $state.raw<{ name: string; centre: boolean } | null>(null);
  /** Typed text in each Artifact's Source, by name, while the app runs. */
  readonly drafts = new SvelteMap<string, Draft>();
  /** The branch whose definitions are shown: state, so that what asked whether they are of a
   * branch before any were read asks again once they are. */
  #branch = $state.raw<BranchKey | null>(null);
  #reads = 0;

  /** The lines the open comment field holds, under a node's width: its room grows with them. */
  /** How wide a text is in the comment's font as the page draws it, once a graph has measured. */
  measure = $state.raw<Measure | null>(null);
  readonly #fieldLines = $derived(this.field ? fieldLines(this.fieldText, NODE.width - 12, this.measure ?? undefined) : 0);
  readonly annotations: Annotations = $derived({
    ...withField(annotationsOf(this.view), this.field?.name ?? null, this.#fieldLines),
    measure: this.measure,
  });

  /** The definitions are of `branch`. */
  of(branch: BranchKey): boolean {
    return this.#branch !== null && keyOf(this.#branch) === keyOf(branch) && this.view !== null;
  }

  /** What `name`'s Definition shows typed; nothing until something is. */
  shownForm(name: string): Readonly<DefinitionForm> {
    return this.forms.get(name) ?? UNTYPED;
  }

  /** `name`'s Definition form, to type into: made the first time, from an event. */
  form(name: string): DefinitionForm {
    let form = this.forms.get(name);
    if (!form) this.forms.set(name, (form = new DefinitionForm()));
    return form;
  }

  /** Open the comment field of `name`, from the Definition's button or the graph, holding its
   * comment as it is now. */
  openField(name: string, from: "button" | "graph", keyboard = false) {
    this.fieldText = this.node(name)?.comment ?? "";
    this.field = { name, from, keyboard };
  }

  node(name: string): DefinitionNode | undefined {
    return this.#nodes.get(name);
  }

  /** The read of the branch last asked for, while it has not answered. */
  #loading: { key: string; read: Promise<void> } | null = null;

  /** `branch`'s definitions are read, or cannot be: a graph is first laid out with them. */
  ready(branch: BranchKey): Promise<void> {
    if (this.#loading?.key === keyOf(branch)) return this.#loading.read;
    if (this.of(branch)) return Promise.resolve();
    return this.load(branch);
  }

  /** Read `branch`'s definitions as they are now; another branch's are dropped at once. */
  load(branch: BranchKey): Promise<void> {
    const read = this.#read(branch);
    const loading = { key: keyOf(branch), read };
    this.#loading = loading;
    return read.finally(() => {
      if (this.#loading === loading) this.#loading = null;
    });
  }

  async #read(branch: BranchKey): Promise<void> {
    const read = ++this.#reads;
    if (this.#branch && keyOf(this.#branch) !== keyOf(branch)) {
      this.view = null;
      this.drafts.clear();
      this.forms.clear();
      this.notice = null;
    }
    this.#branch = branch;
    try {
      const view = await call(this.ipc.definitions(branch));
      if (read !== this.#reads) return;
      this.#checks = {};
      this.view = view;
      this.error = null;
    } catch (failure) {
      if (read === this.#reads) {
        this.view = null;
        this.error = message(failure);
      }
    }
  }

  /** What the library says of each edit asked about since the definitions were last read. */
  #checks: Record<string, Promise<EditCheck>> = {};

  /** What the library says of `operation` on `branch`'s definitions now, writing nothing. */
  check(branch: BranchKey, operation: EditOperation): Promise<EditCheck> {
    const key = JSON.stringify(operation);
    // A check that failed is asked again next time.
    this.#checks[key] ??= call(this.ipc.definitionCheck(branch, operation)).catch((failure: unknown) => {
      delete this.#checks[key];
      throw failure;
    });
    return this.#checks[key];
  }

  /** The text each file was shown with: the definitions' files, a Source's saved text being
   * the text it was typed over. The library says which of them an edit writes. */
  shown(operation: EditOperation): Record<string, string> {
    const files: Record<string, string> = { ...(this.view?.files ?? {}) };
    if (operation.kind === "saveSource") {
      const draft = this.drafts.get(operation.name);
      const file = this.node(operation.name)?.definition;
      if (draft && file) files[file] = draft.base;
    }
    return files;
  }

  /** Apply `operation` to `branch`'s definitions, made `where`. Answers why it wrote nothing,
   * or `null` once written; either way the definitions are read again and, once written, the
   * Current scanned again. */
  async edit(branch: BranchKey, operation: EditOperation, where: Where): Promise<string | null> {
    if (this.busy) return "Another edit is being written.";
    this.busy = true;
    try {
      if (where === "graph") {
        // The files the library says the edit writes: one with typed text in its Source refuses it.
        const checked = await this.check(branch, operation);
        const writes = checked.kind === "checked" ? checked.files : [];
        const typed = [...this.drafts.keys()].map((name) => this.node(name)?.definition).find((file) => file !== undefined && writes.includes(file));
        if (typed !== undefined) {
          const why = `${typed} has unsaved text in its Source; nothing written.`;
          this.notice = why;
          return why;
        }
      }
      const outcome = await call(this.ipc.definitionEdit(branch, this.shown(operation), operation));
      if (outcome.kind === "written") {
        if (operation.kind === "saveSource") this.drafts.delete(operation.name);
        if (operation.kind === "rename" && this.drafts.has(operation.old)) this.drafts.delete(operation.old);
        if (where === "graph") this.notice = null;
        await this.load(branch);
        this.rescan(branch);
        return null;
      }
      const why = refusalText(outcome.refusal, where !== "graph");
      if (where === "graph") this.notice = why;
      await this.load(branch);
      // A file changed on disk: the Current shows what is there now, and so do the Definition's
      // Path and Parts, the edit typed not kept (D-INS-10).
      if (outcome.refusal.kind === "changed") {
        const edited = operation.kind === "setPath" ? operation.name : operation.kind === "addPart" || operation.kind === "removePart" ? operation.whole : null;
        if (edited !== null && this.forms.has(edited)) Object.assign(this.form(edited), { path: null, adding: null });
        this.rescan(branch);
      }
      return why;
    } catch (failure) {
      const why = message(failure);
      if (where === "graph") this.notice = why;
      return why;
    } finally {
      this.busy = false;
    }
  }

  /** Nothing of a branch is shown any more. */
  clear(): void {
    this.#reads += 1;
    this.#branch = null;
    this.view = null;
    this.error = null;
    this.notice = null;
    this.field = null;
    this.dialog = null;
    this.created = null;
    this.drafts.clear();
  }
}
