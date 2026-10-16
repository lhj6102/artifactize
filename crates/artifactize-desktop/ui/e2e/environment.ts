// One fixture-local environment shared by setup, the real app and its executed tools.
// Construct a copy: no fixture ever modifies the runner's or the person's environment.
import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, parse } from "node:path";
import type { Fixture } from "./fixture.ts";

export function environment(fixture: Fixture): NodeJS.ProcessEnv {
  const env = { ...process.env };
  for (const name of Object.keys(env)) {
    const key = name.toUpperCase();
    if (key.startsWith("ARTIFACTIZE_") || key.startsWith("GIT_")
      || /(?:^|_)(?:API_KEY|ACCESS_KEY|PRIVATE_KEY|TOKEN|SECRET|PASSWORD|CREDENTIALS)(?:_|$)/.test(key)
      || ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY", "SSH_ASKPASS", "SSH_AUTH_SOCK", "SSH_AGENT_PID",
        "CODEX_HOME", "CLAUDE_CONFIG_DIR", "CLOUDSDK_CONFIG", "KUBECONFIG", "DOCKER_CONFIG", "NPM_CONFIG_USERCONFIG",
        "AWS_CONFIG_FILE", "AWS_SHARED_CREDENTIALS_FILE", "GOOGLE_APPLICATION_CREDENTIALS", "AZURE_CONFIG_DIR"].includes(key)) delete env[name];
  }
  const directories = {
    HOME: "home", USERPROFILE: "home", APPDATA: "appdata", LOCALAPPDATA: "localappdata",
    XDG_CONFIG_HOME: "config", XDG_DATA_HOME: "data", XDG_STATE_HOME: "user-state", XDG_CACHE_HOME: "cache",
    XDG_RUNTIME_DIR: "runtime", TMPDIR: "tmp", TEMP: "tmp", TMP: "tmp", GNUPGHOME: "gnupg",
  };
  for (const [name, directory] of Object.entries(directories)) {
    // Remove case-insensitive aliases too: Windows environment names are insensitive.
    for (const inherited of Object.keys(env)) if (inherited.toUpperCase() === name) delete env[inherited];
    env[name] = join(fixture.root, directory);
    mkdirSync(env[name], { recursive: true, mode: 0o700 });
  }
  const home = join(fixture.root, "home");
  const drive = parse(home).root.replace(/[\\/]$/, "");
  for (const name of Object.keys(env)) {
    if (["HOMEDRIVE", "HOMEPATH", "USER", "USERNAME", "LOGNAME"].includes(name.toUpperCase())) delete env[name];
  }
  return gitEnvironment(fixture.root, {
    ...env, HOMEDRIVE: drive, HOMEPATH: home.slice(drive.length), USER: "alice", USERNAME: "alice", LOGNAME: "alice",
    ARTIFACTIZE_REMOTE: "off", DESKTOP_TEST_FIXTURE_ROOT: fixture.root,
  });
}

/** Fixture Git never reads the caller's repository/index routes, global includes,
 * templates, hooks, signing settings or credential helpers. */
function gitEnvironment(root: string, env: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
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

export function git(repo: string, ...args: string[]): void {
  execFileSync("git", args, { cwd: dirname(repo), env: environment({ root: dirname(repo), repo, state: join(dirname(repo), "state") }) });
}
