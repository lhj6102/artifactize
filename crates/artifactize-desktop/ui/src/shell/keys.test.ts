// Shortcut matching: Ctrl/⌘ chords follow the physical key, whatever character the layout
// or input method reports; compositions never trigger shortcuts.
import { describe, expect, it } from "vitest";
import { chord, plain } from "./keys";

const press = (init: KeyboardEventInit) => new KeyboardEvent("keydown", init);

describe("chord", () => {
  it("matches the physical key under another layout or input method", () => {
    expect(chord(press({ key: "ㅏ", code: "KeyK", ctrlKey: true }), "k")).toBe(true);
    expect(chord(press({ key: "K", code: "KeyK", ctrlKey: true, shiftKey: true }), "k")).toBe(true);
    expect(chord(press({ key: "k", code: "KeyK", metaKey: true }), "k")).toBe(true);
    expect(chord(press({ key: "1", code: "Digit1", ctrlKey: true }), "1")).toBe(true);
  });

  it("does not match another physical key, a missing modifier, Alt, or a composition", () => {
    // AZERTY: the key labelled k still sends KeyK; a different key does not.
    expect(chord(press({ key: "k", code: "KeyL", ctrlKey: true }), "k")).toBe(false);
    expect(chord(press({ key: "k", code: "KeyK" }), "k")).toBe(false);
    expect(chord(press({ key: "k", code: "KeyK", ctrlKey: true, altKey: true }), "k")).toBe(false);
    expect(chord(press({ key: "k", code: "KeyK", ctrlKey: true, isComposing: true }), "k")).toBe(false);
  });

  it("falls back to the key when an event names no code", () => {
    expect(chord(press({ key: "s", ctrlKey: true }), "s")).toBe(true);
  });
});

describe("plain", () => {
  it("is never a shortcut in a field, a true/false field and a choice included", () => {
    for (const type of ["text", "checkbox", "radio"]) {
      const field = document.createElement("input");
      field.type = type;
      document.body.append(field);
      const event = press({ key: "m", code: "KeyM", bubbles: true });
      let shortcut: boolean | undefined;
      field.addEventListener("keydown", (pressed) => (shortcut = plain(pressed, "m")));
      field.dispatchEvent(event);
      field.remove();
      expect(shortcut, type).toBe(false);
    }
    const button = document.createElement("button");
    document.body.append(button);
    let shortcut: boolean | undefined;
    button.addEventListener("keydown", (pressed) => (shortcut = plain(pressed, "m")));
    button.dispatchEvent(press({ key: "m", code: "KeyM", bubbles: true }));
    button.remove();
    expect(shortcut).toBe(true);
  });


  it("takes the character, so a jamo typed in Hangul mode is no shortcut", () => {
    expect(plain(press({ key: "c", code: "KeyC" }), "c")).toBe(true);
    expect(plain(press({ key: "ㅊ", code: "KeyC" }), "c")).toBe(false);
    expect(plain(press({ key: "c", code: "KeyC", keyCode: 229 }), "c")).toBe(false);
  });
});
