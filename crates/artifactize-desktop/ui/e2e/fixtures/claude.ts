// Claude Code's stand-in for the Claude Agent provider: the program Settings runs as `claude
// auth status|login|logout`. Its sign-in is a file in the fixture; `login` serves a loopback
// sign-in page, opens the system browser on it (the fixture's stand-in) and signs in when the
// browser comes back to its callback.
import { folderHoldsCommand, nodeProgram, withPathFirst, writeNodeCommand, type Environment } from "../platform.ts";
import { makeDirectory, nativePath, suitePath } from "../portable.ts";
import type { Fixture } from "./repo.ts";

/** `env` with the stand-in as the only `claude` on PATH, signed out. */
export function claudeCode(fixture: Pick<Fixture, "root">, env: Environment): Environment {
  const bin = makeDirectory(nativePath(fixture.root, "claude-bin"));
  writeNodeCommand(bin, "claude", "DESKTOP_TEST_CLAUDE_PROGRAM");
  return {
    // Another Claude Code on PATH, such as the developer's own, is left out.
    ...withPathFirst(env, [bin], (folder) => !folderHoldsCommand(folder, "claude")),
    DESKTOP_TEST_NODE: nodeProgram,
    DESKTOP_TEST_CLAUDE_PROGRAM: suitePath("fixtures", "claude-code.ts"),
    DESKTOP_TEST_CLAUDE_SIGNED_IN: nativePath(fixture.root, "claude-signed-in"),
    CLAUDE_CONFIG_DIR: makeDirectory(nativePath(fixture.root, "claude-config"), { private: true }),
  };
}
