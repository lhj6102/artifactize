// One fixture-local environment shared by setup, the real app and its executed tools.
// Construct a copy: no fixture ever modifies the runner's or the person's environment.
import type { Fixture } from "./fixture.ts";
import {
  gitProgram, isolatedEnvironment, makeDirectory, nativePath, parentOf, portablePath, runProgramOk, writeTextOnce,
  type Environment, type NativePath,
} from "./platform.ts";

/** Inherited variables that would let a fixture reach the person's credentials,
 * configuration or network routes, by upper-case name. */
function personal(name: string): boolean {
  return name.startsWith("ARTIFACTIZE_") || name.startsWith("GIT_")
    || /(?:^|_)(?:API_KEY|ACCESS_KEY|PRIVATE_KEY|TOKEN|SECRET|PASSWORD|CREDENTIALS)(?:_|$)/.test(name)
    || ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY", "SSH_ASKPASS", "SSH_AUTH_SOCK", "SSH_AGENT_PID",
      "CODEX_HOME", "CLAUDE_CONFIG_DIR", "CLOUDSDK_CONFIG", "KUBECONFIG", "DOCKER_CONFIG", "NPM_CONFIG_USERCONFIG",
      "AWS_CONFIG_FILE", "AWS_SHARED_CREDENTIALS_FILE", "GOOGLE_APPLICATION_CREDENTIALS", "AZURE_CONFIG_DIR"].includes(name);
}

export function environment(fixture: Fixture): Environment {
  return gitEnvironment(fixture.root, {
    ...isolatedEnvironment(fixture.root, personal),
    ARTIFACTIZE_REMOTE: "off", DESKTOP_TEST_FIXTURE_ROOT: fixture.root,
  });
}

/** Fixture Git never reads the caller's repository/index routes, global includes,
 * templates, hooks, signing settings or credential helpers. */
function gitEnvironment(root: NativePath, env: Environment): Environment {
  const directory = nativePath(root, "git");
  const hooks = makeDirectory(nativePath(directory, "hooks"), { private: true });
  const templates = makeDirectory(nativePath(directory, "templates"), { private: true });
  const config = nativePath(directory, "config");
  writeTextOnce(config, `[user]\nname = Fixture\nemail = fixture@example.invalid\n[core]\nhooksPath = ${JSON.stringify(portablePath(hooks))}\n[init]\ntemplateDir = ${JSON.stringify(portablePath(templates))}\n[commit]\ngpgSign = false\n[tag]\ngpgSign = false\n[credential]\nhelper =\ninteractive = false\n`, { private: true });
  return { ...env, GIT_CONFIG_NOSYSTEM: "1", GIT_CONFIG_GLOBAL: config, GIT_TERMINAL_PROMPT: "0" };
}

export function git(repo: NativePath, ...args: string[]): void {
  const root = parentOf(repo);
  runProgramOk(gitProgram, args, { cwd: root, env: environment({ root, repo, state: nativePath(root, "state") }) });
}
