// Keyboard shortcuts that coexist with text entry and input methods.

/** The event belongs to an input method composition (Korean, Japanese, …): never a shortcut. */
export function composing(event: KeyboardEvent): boolean {
  return event.isComposing || event.keyCode === 229;
}

/** The target takes typed letters itself: inputs, text areas, selects and editable content. */
export function editable(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement) return true;
  if (target instanceof HTMLInputElement) {
    return !["checkbox", "radio", "button", "submit", "reset"].includes(target.type);
  }
  return false;
}

/** The primary modifier: Ctrl, or ⌘ on macOS. */
export function primary(event: KeyboardEvent): boolean {
  return event.ctrlKey || event.metaKey;
}

/**
 * A plain letter or symbol shortcut such as `c` or `?`: no Ctrl, ⌘ or Alt, not typed into
 * a field, not part of a composition.
 */
export function plain(event: KeyboardEvent, key: string): boolean {
  return (
    event.key === key &&
    !event.ctrlKey &&
    !event.metaKey &&
    !event.altKey &&
    !composing(event) &&
    !editable(event.target)
  );
}

/** A Ctrl (⌘) chord such as Ctrl-S, which works inside fields too, never mid-composition. */
export function chord(event: KeyboardEvent, key: string): boolean {
  return primary(event) && !event.altKey && event.key.toLowerCase() === key && !composing(event);
}

/** The label of a shortcut for this platform: `Ctrl+S` or `⌘S`. */
export function label(keys: string): string {
  const mac = typeof navigator !== "undefined" && /Mac/.test(navigator.platform);
  return mac ? keys.replace(/Ctrl\+/g, "⌘") : keys;
}
