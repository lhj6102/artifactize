import assert from "node:assert/strict";
import { test } from "node:test";
import { inputViolations, providerViolations } from "./e2e-integrity.ts";

test("input integrity rejects native OS injection across the suite and device languages", () => {
  for (const source of ["[NSApp sendEvent:key];", "keyEventWithType:kind", "gdk_event_new(KEY_PRESS)", "gtk_widget_event(view, event)", "SendInput(1, &key, size)", "keybd_event(key, 0, 0, 0)", "Input.dispatchKeyEvent", "NSApp().sendEvent_(event)"]) {
    assert.equal(inputViolations("fixture", source).length, 1, source);
  }
  assert.deepEqual(inputViolations("fixture", "setMarkedText:text; webkit_web_view_set_input_method_context(view, context); Input.imeSetComposition; Input.insertText; new KeyboardEvent('keydown', { isComposing: true });"), []);
});

test("input integrity reads code, not comments, and knows gtk-rs's widget event call", () => {
  assert.deepEqual(inputViolations("ui/e2e/cases/input.ts", "// Use WebDriver instead of SendInput\n/* never keybd_event */ press();"), []);
  // A suite helper that dispatches DOM events may carry the name; CDP's method may not.
  assert.deepEqual(inputViolations("ui/e2e/input.ts", "dispatchKeyEvent(element, 'PageDown');"), []);
  assert.equal(inputViolations("ui/e2e/input.ts", 'await browser.cdp("Input", "dispatchKeyEvent", { type: "keyDown" });').length, 1);
  // A Rust lifetime is not a char literal: the comment after it is still prose.
  assert.deepEqual(inputViolations("src/e2e_input.rs", "impl Device<'_> { // no SendInput here\n}"), []);
  assert.equal(inputViolations("src/e2e_input.rs", "let quote = '\\''; SendInput(1, &key, size);").length, 1);
  assert.equal(inputViolations("ui/e2e/cases/input.ts", 'open("https://example.test"); SendInput(1, &key, size);').length, 1);
  for (const source of ["view.event(&constructed);", "view.event(event);", "WidgetExt::event(&view, &constructed);", "gtk::Widget::event(&view, &event);"]) {
    assert.equal(inputViolations("src/e2e_input/platform.rs", source).length, 1, source);
  }
});

test("only the native Windows engine device may issue literal CDP Copy/Paste editing commands", () => {
  const device = "src/e2e_input/platform.rs";
  for (const command of ["Copy", "Paste"]) {
    const call = `Input::${command} => (\n    "Input.dispatchKeyEvent",\n    r#"{"type":"keyDown","commands":["${command}"]}"#.to_owned(),\n),`;
    assert.deepEqual(inputViolations(device, call), []);
    const spaced = `Input::${command} => ("Input.dispatchKeyEvent", r#"{ "type": "keyDown", "commands": [ "${command}" ] }"#.to_owned(),),`;
    assert.deepEqual(inputViolations(device, spaced), []);
    const reordered = `Input::${command} => ("Input.dispatchKeyEvent", r#"{"commands":["${command}"],"type":"keyDown"}"#.to_owned(),),`;
    assert.deepEqual(inputViolations(device, reordered), []);
    for (const path of ["ui/e2e/platform.ts", "ui/e2e/cases/monitor.ts", "vendor/tauri-plugin-wdio-webdriver/src/device.rs"]) {
      assert.equal(inputViolations(path, call).length, 1, path);
    }
  }
  for (const call of [
    '("Input.dispatchKeyEvent", json!({"type":"keyDown", "key":"a"}))',
    '("Input.dispatchKeyEvent", json!({"commands":["SelectAll"]}))',
    '("Input.dispatchKeyEvent", json!({"commands":["Copy", "MoveRight"]}))',
    '("Input.dispatchKeyEvent", parameters)',
    '("Input.dispatchKeyEvent", json!({"type":"keyDown", "key":"c", "modifiers":2, "commands":["Copy"]}))',
    '("Input.dispatchKeyEvent", json!({"type":"keyDown", "commands":["Copy"], "windowsVirtualKeyCode":67}))',
  ]) assert.equal(inputViolations(device, call).length, 1, call);
  // The exemption covers the method name only: a banned API in the same call is still reported.
  assert.equal(inputViolations(device, 'Input::Copy => ("Input.dispatchKeyEvent", r#"{"type":"keyDown","commands":["Copy"]}"#.to_owned(), SendInput(1, &key, size)),').length, 1);
  // Outside its own arm, with another arm's command, or extended at run time, the literal is not exempt.
  for (const call of [
    '("Input.dispatchKeyEvent", r#"{"type":"keyDown","commands":["Copy"]}"#.to_owned()),',
    'Input::Copy => ("Input.dispatchKeyEvent", r#"{"type":"keyDown","commands":["Paste"]}"#.to_owned(),),',
    'Input::Copy => ("Input.dispatchKeyEvent", r#"{"type":"keyDown","commands":["Copy"]}"#.to_owned() + extra,),',
    'OtherInput::Copy => ("Input.dispatchKeyEvent", r#"{"type":"keyDown","commands":["Copy"]}"#.to_owned(),),',
  ]) assert.equal(inputViolations(device, call).length, 1, call);
  // A payload built at run time is not the fixed literal, even when it names only Copy.
  assert.equal(inputViolations(device, '("Input.dispatchKeyEvent", json!({"type":"keyDown","commands":["Copy"]}))').length, 1);
  for (const name of ["PostMessageW", "SendMessageA", "mouse_event", "XTestFakeButtonEvent"]) assert.equal(inputViolations("fixture", `${name}(window, message, 0, 0)`).length, 1, name);
});

test("provider integrity detects changed, missing, additional and non-file source entries", () => {
  const release = { "src/lib.rs": "original" };
  assert.deepEqual(providerViolations(release, release), []);
  for (const actual of [{ "src/lib.rs": "changed" }, {}, { ...release, "src/device.rs": "extra" }, { "src/lib.rs": "not a regular file" }]) {
    assert.ok(providerViolations(actual, release).length > 0);
  }
  // Names that are also Object.prototype members are not recorded files.
  assert.ok(providerViolations({ ...release, constructor: "extra", toString: "extra" }, release).length === 2);
  const generated = { "permissions/autogenerated/reference.md": "known-build-output" };
  assert.deepEqual(providerViolations({ ...release, ...generated }, release, generated), []);
  assert.ok(providerViolations({ ...release, "permissions/autogenerated/reference.md": "changed" }, release, generated).length > 0);
  // A recorded exception outside the build's output folder authorizes nothing.
  assert.ok(providerViolations({ ...release, "src/device.rs": "extra" }, release, { "src/device.rs": "extra" }).length > 0);
  assert.ok(providerViolations({ ...release, "permissions/autogenerated/other.md": "extra" }, release, { "permissions/autogenerated/other.md": "extra" }).length > 0);
});
