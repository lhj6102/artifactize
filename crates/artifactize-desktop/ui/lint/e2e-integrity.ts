// Mechanical E2E input and provider-provenance rules. Filesystem access stays in platform.ts.
import { createHash } from "node:crypto";
import { absolutePath, readBytes, readJson, readText, sourceTree, suitePath } from "../e2e/platform.ts";

const injection = /\b(?:sendEvent|keyEventWithType|gdk_event_new|gtk_widget_event|gtk_main_do_event|gdk_display_put_event|SendInput|keybd_event|CGEventPost|CGEventCreateKeyboardEvent|XSendEvent|XTestFakeKeyEvent|PostMessage|SendMessage|dispatchKeyEvent)\b/;

/** Forbidden API names are rejected even in otherwise portable-looking native adapters. */
export function inputViolations(path: string, source: string): string[] {
  return source.split("\n").flatMap((line, at) => injection.test(line) ? [`${path}:${at + 1}: E2E-INPUT forbids OS/key-event injection APIs`] : []);
}

/** Compare every source byte, and reject additions rather than just checking known files. */
export function providerViolations(actual: Record<string, string>, expected: Record<string, string>, generated: Record<string, string> = {}): string[] {
  const violations: string[] = [];
  for (const [path, digest] of Object.entries(expected)) {
    if (actual[path] !== digest) violations.push(`${path}: E2E-PROVIDER-UPSTREAM differs from the recorded release`);
  }
  for (const [path, digest] of Object.entries(actual)) {
    if (!(path in expected) && generated[path] !== digest) violations.push(`${path}: E2E-PROVIDER-UPSTREAM has unrecorded content`);
  }
  return violations;
}

export function checkIntegrity(): void {
  const desktop = absolutePath(suitePath(), "../..");
  const failures: string[] = [];
  const roots = [suitePath(), absolutePath(desktop, "src/e2e_input")];
  for (const root of roots) {
    for (const item of sourceTree(root)) {
      if (item.kind !== "file") throw new Error(`E2E source must be a regular file: ${item.name}`);
      if (/\.(?:ts|js|mjs|mts|rs|c|h|m)$/.test(item.name)) failures.push(...inputViolations(item.name, readText(item.path)));
    }
  }
  const device = absolutePath(desktop, "src/e2e_input.rs");
  failures.push(...inputViolations("src/e2e_input.rs", readText(device)));
  const release = readJson(suitePath("provider-upstream.json")) as { files: Record<string, string>; generated: Record<string, string> };
  const actual = Object.fromEntries(sourceTree(absolutePath(desktop, "vendor/tauri-plugin-wdio-webdriver")).map((item) => [
    item.name, item.kind === "file" ? createHash("sha256").update(readBytes(item.path)).digest("hex") : "not a regular file",
  ]));
  failures.push(...providerViolations(actual, release.files, release.generated));
  if (failures.length) throw new Error(failures.join("\n"));
}
