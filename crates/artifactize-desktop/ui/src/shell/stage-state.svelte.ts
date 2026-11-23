// The stage header's tools slot (D-MAIN-1): the stage owns the element at its header's right;
// a view deeper down (a graph) shows its own tools there through `stageTools`, keeping them its
// own components and state while they stand in the shared row.
import { createContext } from "svelte";
import type { Attachment } from "svelte/attachments";

export class StageSlot {
  /** The header's tools place, once the stage is drawn. */
  tools = $state<HTMLElement | null>(null);
}

export const [getStageSlot, setStageSlot] = createContext<StageSlot>();

/** The slot of the enclosing stage, or null outside one (a viewer window, a test host). */
export function stageSlot(): StageSlot | null {
  try {
    return getStageSlot();
  } catch {
    return null;
  }
}

/** Move the element into the stage header's tools while it exists, and leave it where it is
 * outside a stage. Its teardown takes it out of the header again, so Svelte removes its own
 * block as it laid it out. */
export function stageTools(slot: StageSlot | null): Attachment<HTMLElement> {
  return (element) => {
    const target = slot?.tools;
    if (!target) return;
    target.append(element);
    return () => element.remove();
  };
}
