import type { Attachment } from "svelte/attachments";

/** One form surface's DOM caret, independent of the library's draft and of other surfaces. */
export class FormCaret {
  #root: HTMLElement | null = null;
  #last: HTMLTextAreaElement | null = null;

  readonly attach: Attachment<HTMLElement> = (root) => {
    this.#root = root;
    const focus = (event: FocusEvent) => {
      if (event.target instanceof HTMLTextAreaElement) this.#last = event.target;
    };
    root.addEventListener("focusin", focus);
    return () => {
      root.removeEventListener("focusin", focus);
      if (this.#root === root) {
        this.#root = null;
        this.#last = null;
      }
    };
  };

  insert(text: string): void {
    const root = this.#root;
    if (!root) return;
    const last = this.#last;
    const field = last && root.contains(last) ? last : root.querySelector<HTMLTextAreaElement>("textarea[data-comment]");
    if (!field) return;
    const caret = field.selectionStart;
    field.setRangeText(text, caret, caret, "end");
    field.focus();
    // Use the same input path as typing, including a text item or a field in a group.
    field.dispatchEvent(new InputEvent("input", { bubbles: true, data: text, inputType: "insertText" }));
  }
}
