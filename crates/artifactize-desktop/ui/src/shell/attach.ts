// Attachments the screens share (`{@attach ...}`): work on an element that follows reactive
// state, re-run when what they read changes and need no `$effect`.
import type { Attachment } from "svelte/attachments";

/** Keep the descendant `selector()` names in view. The selector is read inside, so the
 * attachment follows whatever state it reads. */
export function scrollTo(selector: () => string): Attachment<HTMLElement> {
  return (root) => {
    root.querySelector<HTMLElement>(selector())?.scrollIntoView({ block: "nearest" });
  };
}
