import type { Attachment } from "svelte/attachments";

type Field = HTMLTextAreaElement | HTMLInputElement;

/** One surface's issues field and its DOM caret, independent of the library's draft and of
 * other surfaces: a point's Insert puts its text there (J-HUMAN-POINTS). */
export class IssuesCaret {
  #field: Field | null = $state(null);
  /** The field has had the caret, so its selection is where the person left it. */
  #placed = false;

  /** A field is there to take an insert. */
  get ready(): boolean {
    return this.#field !== null;
  }

  readonly attach: Attachment<Field> = (field) => {
    this.#field = field;
    this.#placed = false;
    const placed = () => (this.#placed = true);
    field.addEventListener("focus", placed);
    return () => {
      field.removeEventListener("focus", placed);
      if (this.#field === field) this.#field = null;
    };
  };

  /** Put `text` at the field's caret, else at its end, as typing would. */
  insert(text: string): void {
    const field = this.#field;
    if (!field) return;
    const start = this.#placed ? field.selectionStart ?? field.value.length : field.value.length;
    const end = this.#placed ? field.selectionEnd ?? start : start;
    field.setRangeText(text, start, end, "end");
    field.focus();
    field.dispatchEvent(new InputEvent("input", { bubbles: true, data: text, inputType: "insertText" }));
  }
}
