// Linux fixture environment and Git execution: one native boundary shared by setup,
// the real app, and durable-worker cleanup. These inputs never modify the caller.
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import type { Fixture } from "./fixture.ts";

/** Native fixture inputs never borrow the person's auth files, profiles or service
 * configuration. Keep executable lookup, but require scenarios to declare credentials
 * and endpoints explicitly. No process-wide environment is changed. */
export function environment(fixture: Fixture): NodeJS.ProcessEnv {
  const env = { ...process.env };
  for (const name of Object.keys(env)) {
    if (name.startsWith("ARTIFACTIZE_") || name.endsWith("_API_KEY") || ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY"].includes(name.toUpperCase()) || ["CLAUDE_CODE_OAUTH_TOKEN", "CODEX_HOME", "XAUTHORITY", "DBUS_SESSION_BUS_ADDRESS"].includes(name)) delete env[name];
  }
  for (const [name, directory] of Object.entries({ HOME: "home", XDG_CONFIG_HOME: "config", XDG_DATA_HOME: "data", XDG_CACHE_HOME: "cache", XDG_RUNTIME_DIR: "runtime", TMPDIR: "tmp" })) {
    env[name] = join(fixture.root, directory);
    mkdirSync(env[name], { recursive: true, mode: 0o700 });
  }
  return gitEnvironment(fixture.root, { ...env, USER: "alice", ARTIFACTIZE_REMOTE: "off", DESKTOP_TEST_FIXTURE_ROOT: fixture.root });
}

/** Git and the real app's Git reads share this fixture's configuration, never the
 * caller's repository/index routes, global includes, hooks, signing or credentials. */
function gitEnvironment(root: string, inherited: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
  const env = { ...inherited };
  for (const name of Object.keys(env)) if (name.toUpperCase().startsWith("GIT_")) delete env[name];
  delete env.SSH_ASKPASS;
  const directory = join(root, "git");
  const hooks = join(directory, "hooks");
  const templates = join(directory, "templates");
  for (const path of [hooks, templates]) mkdirSync(path, { recursive: true, mode: 0o700 });
  const config = join(directory, "config");
  try {
    writeFileSync(config, `[user]\nname = Fixture\nemail = fixture@example.invalid\n[core]\nhooksPath = ${JSON.stringify(hooks)}\n[init]\ntemplateDir = ${JSON.stringify(templates)}\n[commit]\ngpgSign = false\n[tag]\ngpgSign = false\n[credential]\nhelper =\ninteractive = false\n`, { flag: "wx", mode: 0o600 });
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
  }
  return { ...env, GIT_CONFIG_NOSYSTEM: "1", GIT_CONFIG_GLOBAL: config, GIT_TERMINAL_PROMPT: "0" };
}

/** All fixture Git writes cross one native boundary with fixture-local inputs. */
export function git(repo: string, ...args: string[]): void {
  execFileSync("git", args, { cwd: dirname(repo), env: environment({ root: dirname(repo), repo, state: join(dirname(repo), "state") }) });
}

