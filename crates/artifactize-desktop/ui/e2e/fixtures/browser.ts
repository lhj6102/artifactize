// The system browser of a scenario: the app's opener runs a stand-in that fetches what it is
// handed and records it, the same on every OS; only where the app finds the opener differs
// (platform.ts).
import { installBrowserOpener, type Environment } from "../platform.ts";
import { nativePath, readTextIfExists, suitePath, writeText } from "../portable.ts";
import type { Fixture } from "./repo.ts";

const log = (fixture: Pick<Fixture, "root">) => nativePath(fixture.root, "browser-handoffs.jsonl");

/** `env` with the fixture's stand-in as the system browser. */
export function standInBrowser(fixture: Pick<Fixture, "root">, env: Environment): Environment {
  writeText(log(fixture), "");
  return { ...installBrowserOpener(fixture.root, env, suitePath("fixtures", "browser-client.ts")), DESKTOP_TEST_BROWSER_LOG: log(fixture) };
}

/** What the stand-in browser was handed and fully fetched, in order. */
export function browserHandoffs(fixture: Pick<Fixture, "root">): string[] {
  return (readTextIfExists(log(fixture)) ?? "").split("\n").filter(Boolean).map((line) => JSON.parse(line) as string);
}
