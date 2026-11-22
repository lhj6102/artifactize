// A fixture-owned installation of the Antigravity CLI. Initially off PATH: the scenario can
// install it and change its reported version while the app is open, as on a user's machine.
import { folderHoldsCommand, nodeProgram, withPathFirst, writeNodeCommand, type Environment } from "../platform.ts";
import { makeDirectory, nativePath, suitePath, writeText } from "../portable.ts";
import type { Fixture } from "./repo.ts";

export const SIGN_IN_URL = "https://accounts.google.com/o/oauth2/v2/auth?redirect_uri=https%3A%2F%2Fantigravity.google%2Foauth-callback&state=e2e-only";
export const SIGN_IN_CODE = "fixture-code";
export const AGY_MODELS = [{ value: "fixture-flash", name: "Fixture Flash" }, { value: "fixture-pro", name: "Fixture Pro" }];

export function antigravity(fixture: Pick<Fixture, "root">, env: Environment) {
  const bin = makeDirectory(nativePath(fixture.root, "agy-bin"));
  const staged = makeDirectory(nativePath(fixture.root, "agy-staged"));
  const command = writeNodeCommand(staged, "agy", "DESKTOP_TEST_AGY_PROGRAM");
  const version = nativePath(fixture.root, "agy-version");
  writeText(version, "1.5.0");
  return {
    env: {
      ...withPathFirst(env, [bin], (folder) => !folderHoldsCommand(folder, "agy")),
      DESKTOP_TEST_NODE: nodeProgram,
      DESKTOP_TEST_AGY_PROGRAM: suitePath("fixtures", "agy.ts"),
      DESKTOP_TEST_AGY_VERSION: version,
      DESKTOP_TEST_AGY_SIGNED_IN: nativePath(fixture.root, "agy-signed-in"),
      // The engine checks the URL's shape; the stand-in browser never contacts Google.
      DESKTOP_TEST_BROWSER_RECORD_ONLY: SIGN_IN_URL,
    },
    data: { staged: nativePath(staged, command), installed: nativePath(bin, command), version },
  };
}
