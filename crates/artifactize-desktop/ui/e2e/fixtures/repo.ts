// Repositories and state for the scenarios, made only through the real `artifactize` CLI and
// Git, each in its scenario's own folder with an environment that never reaches the person's.
import { readCandidate } from "../build.ts";
import { isolatedEnvironment, portablePath, type Environment } from "../platform.ts";
import { makeDirectory, nativePath, readTextIfExists, runProgram, runProgramAsync, runProgramOk, serveLoopbackHttp, updateDatabase, writeText, type NativePath } from "../portable.ts";
import { check, commandFields, declareArtifact, runtimeProfile, type Operation } from "./declarations.ts";
import type { Owned } from "./owned.ts";

/** A scenario's repository and the state folder the app and the CLI share. */
export type Fixture = { root: NativePath; repo: NativePath; state: NativePath };

/** Inherited variables that would reach the person's credentials, configuration, network
 * routes or tool settings, by upper-case name. */
function personal(name: string): boolean {
  return name.startsWith("ARTIFACTIZE_") || name.startsWith("GIT_") || name.startsWith("NODE_") || name.startsWith("NPM_CONFIG_")
    || name.startsWith("DESKTOP_TEST_CLAUDE_") || name.startsWith("DESKTOP_TEST_BROWSER_")
    || /(?:^|_)(?:API_KEY|ACCESS_KEY|PRIVATE_KEY|TOKEN|SECRET|PASSWORD|CREDENTIALS)(?:_|$)/.test(name)
    || ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY", "SSH_ASKPASS", "SSH_AUTH_SOCK", "SSH_AGENT_PID", "CODEX_HOME",
      "CLAUDE_CONFIG_DIR", "CLOUDSDK_CONFIG", "KUBECONFIG", "DOCKER_CONFIG", "AWS_CONFIG_FILE", "AWS_SHARED_CREDENTIALS_FILE",
      "GOOGLE_APPLICATION_CREDENTIALS", "AZURE_CONFIG_DIR"].includes(name);
}

/** The environment the app, the CLI and Git get for `fixture`: the person's own homes,
 * credentials and Git settings replaced by the fixture's. */
export function environment(fixture: Pick<Fixture, "root">): Environment {
  const git = makeDirectory(nativePath(fixture.root, "git"), { private: true });
  const config = nativePath(git, "config");
  if (readTextIfExists(config) === null) {
    const hooks = portablePath(makeDirectory(nativePath(git, "hooks")));
    writeText(config, `[user]\nname = Fixture\nemail = fixture@example.invalid\n[core]\nhooksPath = ${JSON.stringify(hooks)}\n[commit]\ngpgSign = false\n[credential]\nhelper =\n`);
  }
  return {
    ...isolatedEnvironment(fixture.root, personal),
    ARTIFACTIZE_REMOTE: "off", GIT_CONFIG_NOSYSTEM: "1", GIT_CONFIG_GLOBAL: config, GIT_TERMINAL_PROMPT: "0",
  };
}

/** Run Git in `repo`, a folder of `fixture`. */
export function git(fixture: Fixture, repo: NativePath, ...args: string[]): void {
  runProgramOk("git", args, { cwd: repo, env: environment(fixture) });
}

/** A fresh Git repository `name` beside the fixture's others. */
export function initRepo(fixture: Fixture, repo = fixture.repo): void {
  makeDirectory(repo);
  git(fixture, repo, "init", "-q", "--initial-branch=master", repo);
}

/** The scenario's fixture: its repository `repo` and its state, whose Runs teardown cancels. */
export function newFixture(own: Owned): Fixture {
  const fixture = { root: own.root, repo: nativePath(own.root, "repo"), state: nativePath(own.root, "state") };
  own.defer(() => cancelRuns(fixture));
  return fixture;
}

export const cli = () => readCandidate().cli;
export const app = () => readCandidate().app;

/** Run the CLI on `repo` (the fixture's by default) and the fixture's state. */
export function artifactize(fixture: Fixture, args: string[], repo = fixture.repo): ReturnType<typeof runProgram> {
  return runProgram(cli(), ["--repo", repo, "--state-dir", fixture.state, ...args], { env: environment(fixture) });
}

/** As `artifactize`, without blocking a server the calling scenario serves. */
export function artifactizeAsync(fixture: Fixture, args: string[], env: Environment = {}): ReturnType<typeof runProgramAsync> {
  return runProgramAsync(cli(), ["--repo", fixture.repo, "--state-dir", fixture.state, ...args], { env: { ...environment(fixture), ...env } });
}

/** The CLI's JSON answer to `args`; a failure is an error. */
export function cliJson<T>(fixture: Fixture, args: string[], repo = fixture.repo, ok = [0]): T {
  const result = artifactize(fixture, [...args, "--json"], repo);
  if (!ok.includes(result.status)) throw new Error(`artifactize ${args.join(" ")} exited ${result.status}: ${result.stderr}${result.stdout}`);
  return JSON.parse(result.stdout) as T;
}

/** A saved request as `request list --json` prints it. */
export type Saved = { id: string; runId: string; evalId: string; status: string; claim: { reviewer: string } | null; result: Record<string, unknown> | null };

export function requests(fixture: Fixture): Saved[] {
  return cliJson<Saved[]>(fixture, ["request", "list"]);
}

/** The saved request of the eval `evalId` (the release's sign-off by default). */
export function request(fixture: Fixture, evalId = "release/signoff"): Saved {
  const found = requests(fixture).find((saved) => saved.evalId === evalId);
  if (!found) throw new Error(`no ${evalId} request`);
  return found;
}

/** Every saved Run, newest first. */
export function savedRuns(fixture: Fixture): { id: string; status: string }[] {
  return cliJson(fixture, ["run", "list", "--all", "--limit", "4294967295"]);
}

/** Cancel every Run of the fixture's state still open and wait until each has ended. */
export function cancelRuns(fixture: Fixture): void {
  if (readTextIfExists(nativePath(fixture.state, "state.sqlite")) === null) return;
  for (const run of savedRuns(fixture)) {
    if (run.status !== "RUNNING" && run.status !== "WAITING_HUMAN") continue;
    artifactize(fixture, ["run", "cancel", run.id]);
    artifactize(fixture, ["run", "show", run.id, "--wait", "--json"]);
  }
}

/** Claim the waiting request `id` as `reviewer` and submit `fields` GREEN, as another process. */
export function signOff(fixture: Fixture, id: string, fields = '{"approved":true,"comment":"ok"}', reviewer = "bob"): void {
  cliJson(fixture, ["request", "claim", id, "--reviewer", reviewer]);
  cliJson(fixture, ["request", "submit", id, "--reviewer", reviewer, "--verdict", "GREEN", "--fields", fields]);
}

/** Verify every Artifact of `repo` to its end; its Run's ID. `status` is the exit the outcome
 * gives (0 GREEN, 1 RED). */
export function verify(fixture: Fixture, repo = fixture.repo, status = 0, ...args: string[]): string {
  return cliJson<{ id: string }>(fixture, ["verify", "--all", "--wait", ...args], repo, [status]).id;
}

/** Start verifying every Artifact of `repo` and wait until its Run waits for a person: the
 * sign-off grace is set so long that it stays open until the scenario signs off. */
export async function verifyWaiting(fixture: Fixture, repo = fixture.repo): Promise<string> {
  runProgramOk(cli(), ["--state-dir", fixture.state, "limits", "--signoff-idle-grace", "86400"], { env: environment(fixture) });
  const { run } = cliJson<{ run: string }>(fixture, ["verify", "--all"], repo);
  const deadline = Date.now() + 60000;
  for (;;) {
    const shown = cliJson<{ status: string; requests: Saved[] }>(fixture, ["run", "show", run], repo);
    if (shown.status === "WAITING_HUMAN" && shown.requests.some((saved) => saved.status === "DEFERRED")) return run;
    if (shown.status !== "RUNNING" && shown.status !== "WAITING_HUMAN") throw new Error(`Run ${run} ended ${shown.status} before it waited for a person`);
    if (Date.now() > deadline) throw new Error(`Run ${run} did not wait for a person`);
    await new Promise((done) => setTimeout(done, 50));
  }
}

/** Give the Runs `runs`, oldest first, creation times a minute apart ending a minute ago, so
 * their order does not depend on the clock's resolution. */
export function orderRuns(fixture: Fixture, runs: readonly string[]): void {
  const saved = requests(fixture);
  updateDatabase(nativePath(fixture.state, "state.sqlite"), (db) => {
    runs.forEach((id, index) => {
      const created = new Date(Date.now() - (runs.length - index) * 60_000).toISOString().replace(/\.(\d{3})Z$/, ".$1000000Z");
      db.prepare("UPDATE runs SET data=json_set(data,'$.createdAt',?) WHERE id=?").run(created, id);
      for (const each of saved.filter((item) => item.runId === id)) {
        db.prepare("UPDATE requests SET data=json_set(data,'$.createdAt',?) WHERE id=?").run(created, each.id);
      }
    });
  });
}

// ---------------------------------------------------------------------------------
// The release project: a release with a Human sign-off and its tools, ship that reads it, docs

/** Options of the release's sign-off. */
export type Signoff = { title?: string; instruction?: string; scalars?: boolean; missing?: boolean; report?: boolean };

const release = (root: NativePath, signoff: Signoff) => `tags = ["type:release"]

[views.human_tools]
notes = { description = "Print the release notes of {artifactName}.", kind = "output", ${commandFields("read", "{artifactPath}/notes.md")} }
open = { description = "Open {artifactName}.", kind = "launch", ${commandFields("launch", portablePath(nativePath(root, "launched")))} }
fail = { description = "Fail on purpose.", kind = "output", ${commandFields("fail")} }
talk = { description = "Open a stand-in that prints what the reviewer chose.", kind = "launch", ${commandFields("talk", portablePath(nativePath(root, "talk.gate")))} }
slow = { description = "Run until cancelled.", kind = "output", ${commandFields("slow", portablePath(nativePath(root, "slow.gate")))} }
${signoff.report ? `report = { description = "Print long lines on both streams.", kind = "output", ${commandFields("report")} }` : ""}
${signoff.missing ? `gone = { description = "Run a program that is not installed.", kind = "output", command = ${JSON.stringify(portablePath(nativePath(root, "absent", "not-installed")))}, args = [] }` : ""}

[evals.signoff]
title = ${JSON.stringify(signoff.title ?? "A person approves the release")}
profile = { kind = "human" }
payload.instruction = ${JSON.stringify(signoff.instruction ?? "Read the release notes and approve them.")}

[evals.signoff.pass_schema]
type = "object"
properties.approved = { const = true }
properties.comment = { type = "string", minLength = 1, description = "What you checked." }
${signoff.scalars ? `properties.score = { type = "number" }
properties.note = { type = "string" }
properties.confirmed = { type = "boolean" }
properties.channel = { enum = ["stable", "preview"] }` : ""}
required = ["approved", "comment"]
additionalProperties = false

[evals.signoff.fail_schema]
type = "object"
properties.reason = { type = "string", minLength = 3 }
properties.findings = { type = "array", items = { type = "object", properties = { problem = { type = "string", minLength = 1 }, severity = { enum = ["low", "high"] } }, required = ["problem"], additionalProperties = false } }
properties.steps = { type = "array", items = { type = "string" } }
required = ["reason"]
additionalProperties = false
`;

/** Declare the release project in `repo` (the fixture's by default), a new Git repository. */
export function declareRelease(fixture: Fixture, signoff: Signoff = {}, repo = fixture.repo, notes = "# Release notes\n- Ready to ship.\n"): void {
  declareArtifact(repo, ["release"], "release", release(fixture.root, signoff));
  declareArtifact(repo, ["ship"], "ship", check("check", "ship", "pass", "Ship {release}."));
  declareArtifact(repo, ["docs"], "docs", `[views.human_tools]\nreference = { description = "Print documentation review notes.", builtin = "read", args = ["{artifactPath}/notes.md"] }\n${check("lint", "docs")}`);
  writeText(nativePath(repo, "release", "notes.md"), notes);
  writeText(nativePath(repo, "docs", "notes.md"), "Documentation checked.\n");
  initRepo(fixture, repo);
}

/** The release project with one Run waiting for its sign-off. */
export async function createRelease(own: Owned, signoff: Signoff = {}): Promise<Fixture> {
  const fixture = newFixture(own);
  declareRelease(fixture, signoff);
  await verifyWaiting(fixture);
  return fixture;
}

/** Another repository `name` beside the fixture's, in its state, with the release project and
 * its own waiting sign-off; its path. */
export async function addRepository(fixture: Fixture, name: string): Promise<NativePath> {
  const repo = nativePath(fixture.root, name);
  declareRelease(fixture, {}, repo, `# Release notes of ${name}\n- Ready to ship.\n`);
  await verifyWaiting(fixture, repo);
  return repo;
}

/** Git worktrees `names` of the fixture's repository, each with its own Run waiting for a
 * person on notes of its own; their Runs. */
export async function addWorktrees(fixture: Fixture, names: string[]): Promise<string[]> {
  git(fixture, fixture.repo, "add", ".");
  git(fixture, fixture.repo, "commit", "-qm", "Fixture inputs");
  const runs: string[] = [];
  for (const name of names) {
    const repo = nativePath(fixture.root, name);
    git(fixture, fixture.repo, "worktree", "add", "-q", "-b", name, repo);
    writeText(nativePath(repo, "release", "notes.md"), `# Notes of ${name}\n`);
    runs.push(await verifyWaiting(fixture, repo));
  }
  return runs;
}

// ---------------------------------------------------------------------------------
// Graph shapes

/** A repository of Artifacts `[folders, name, body]`, verified once to `status` unless `verified`
 * is false. */
export function createProject(own: Owned, artifacts: [string[], string, string][], status: number | false = 0): Fixture {
  const fixture = newFixture(own);
  for (const [folders, name, body] of artifacts) declareArtifact(fixture.repo, folders, name, body);
  initRepo(fixture);
  if (status !== false) verify(fixture, fixture.repo, status);
  return fixture;
}

/** An Artifact of each verdict: red fails, crash exceeds its deadline (ERROR), down is blocked
 * by red, fine passes, gate waits for a person's sign-off and later waits for gate. */
export async function createStates(own: Owned): Promise<Fixture> {
  const fixture = createProject(own, [
    [["red"], "red", check("check", "red", "fail", "Fail.")],
    [["crash"], "crash", `\n[evals.check]\ntitle = "Check crash"\nprofile = ${runtimeProfile("hold", 1000)}\npayload.instruction = "Exceed the deadline."\n`],
    [["down"], "down", check("check", "down", "pass", "Run after {red}.")],
    [["fine"], "fine", check("check", "fine")],
    [["gate"], "gate", `\n[evals.signoff]\ntitle = "A person approves"\nprofile = { kind = "human" }\npayload.instruction = "Approve."\n`],
    [["later"], "later", check("check", "later", "pass", "Run after {gate}.")],
  ], false);
  await verifyWaiting(fixture);
  return fixture;
}

/** Wholes and parts: suite's folder holds app and site, app's holds kit, docs names site among
 * its parts too; lib is a part of nothing, and guide reads suite. Verified GREEN. */
export function createParts(own: Owned): Fixture {
  return createProject(own, [
    [["lib"], "lib", check("check", "lib")],
    [["suite"], "suite", check("check", "suite", "pass", "Check suite with {lib}.")],
    [["suite", "app"], "app", check("check", "app", "pass", "Check app with {site}.")],
    [["suite", "app", "kit"], "kit", check("check", "kit")],
    [["suite", "site"], "site", check("check", "site", "pass", "Check site with {lib}.")],
    [["docs"], "docs", `parts = ["site"]\n${check("check", "docs", "pass", "Check docs with {lib}.")}`],
    [["guide"], "guide", check("check", "guide", "pass", "Check guide with {suite}.")],
  ]);
}

/** `count` Artifacts n0000 … that need nothing to verify, verified GREEN: a graph too large to
 * draw at once. */
export function createLarge(own: Owned, count: number): Fixture {
  const names = Array.from({ length: count }, (_, index) => `n${String(index).padStart(4, "0")}`);
  return createProject(own, names.map((name) => [[name], name, "basis = true\n"]));
}

/** The fixture's fingerprint scripts' gate and the file they record their runs in. */
export type Checking = Fixture & { gate: NativePath; runs: NativePath };

/** slow's fingerprint script answers once the scenario writes `gate`, so a Current keeps slow
 * being checked until then; quick reads slow; other waits for a person. No Run yet. */
export function createChecking(own: Owned): Checking {
  const gate = nativePath(own.root, "fingerprint.gate");
  const runs = nativePath(own.root, "fingerprint.runs");
  const fixture = createProject(own, [
    [["slow"], "slow", `fingerprint = { script = { ${commandFields("fingerprint", portablePath(gate), portablePath(runs))}, files = [], timeout_ms = 600000 } }\n${check("check", "slow")}`],
    [["quick"], "quick", check("check", "quick", "pass", "Check quick with {slow}.")],
    [["other"], "other", `\n[evals.signoff]\ntitle = "A person approves other"\nprofile = { kind = "human" }\npayload.instruction = "Approve other."\n`],
  ], false);
  return { ...fixture, gate, runs };
}

/** Artifact `app` with one runtime eval per name, each saying `checked {name}`. Verified GREEN. */
export function createEvals(own: Owned, names: string[], operation: Operation = "say"): Fixture {
  const evals = names.map((name) => `\n[evals.${name}]\ntitle = "Check ${name}"\nprofile = ${runtimeProfile(operation, undefined, `checked ${name}`)}\npayload.instruction = "Check ${name}."\n`).join("");
  return createProject(own, [[["app"], "app", evals]]);
}

/** The state's sign-off idle grace, in seconds, as the CLI reads it. */
export function signoffGrace(fixture: Fixture): number {
  return (JSON.parse(runProgramOk(cli(), ["--state-dir", fixture.state, "limits", "--json"], { env: environment(fixture) })) as { signoffIdleGraceSeconds: number }).signoffIdleGraceSeconds;
}

/** An empty fixture outside every repository, for what lives in the state alone. */
export function createEmpty(own: Owned): Fixture {
  const fixture = newFixture(own);
  makeDirectory(fixture.repo);
  return fixture;
}


// ---------------------------------------------------------------------------------
// Definitions to edit on a Current

export const APP_NOTE = "Ship the app with its kit.";
export const CORE_NOTE = "Core stays small.";

/** A branch without a Run whose definitions are one file each in `.artifactize/`: app and
 * bundle each hold kit, core is on its own, docs reads core. With `comments`, app and core
 * carry one; with `placed`, the top level is placed by hand. */
export function createEditing(own: Owned, { comments = false, placed = false } = {}): Fixture {
  const fixture = newFixture(own);
  const note = (text: string) => (comments ? `\n[annotations.comment]\ntext = ${JSON.stringify(text)}\n` : "");
  const at = (x: number, y: number) => (placed ? `\n[annotations.style]\nat = [${x.toFixed(1)}, ${y.toFixed(1)}]\n` : "");
  const files: Record<string, string> = {
    app: `path = "app/"\nparts = ["kit"]\nbasis = true\n${at(0, 0)}${note(APP_NOTE)}`,
    bundle: `path = "bundle/"\nparts = ["kit"]\nbasis = true\n${at(420, 0)}`,
    kit: `path = "kit/"\nbasis = true\n`,
    core: `path = "core/"\nbasis = true\n${at(0, 320)}${note(CORE_NOTE)}`,
    docs: `path = "docs/"\n${at(420, 320)}${check("check", "docs", "pass", "Check {docs} against {core}.")}`,
  };
  const folder = makeDirectory(nativePath(fixture.repo, ".artifactize"));
  for (const [name, text] of Object.entries(files)) {
    writeText(nativePath(makeDirectory(nativePath(fixture.repo, name)), "readme.md"), `${name}\n`);
    writeText(nativePath(folder, `${name}.artf`), text);
  }
  initRepo(fixture);
  return fixture;
}

/** The definition file of `name` as it is on disk now, or `null` once it is gone. */
export function definitionText(fixture: Fixture, name: string): string | null {
  return readTextIfExists(nativePath(fixture.repo, ".artifactize", `${name}.artf`));
}

/** Replace the definition file of `name`, as another writer would. */
export function rewriteDefinition(fixture: Fixture, name: string, text: string): void {
  writeText(nativePath(fixture.repo, ".artifactize", `${name}.artf`), text);
}

// ---------------------------------------------------------------------------------
// An Agent eval's saved result

export const REVIEW_MODEL = "fixture-review-model";
export const REVIEW_OWNER = "Orchard reviewer";

/** A repository whose Artifact report has an Agent eval, verified GREEN once through a loopback
 * stand-in of the provider that answers with an owner field and a summary. */
export async function createReviewed(own: Owned): Promise<Fixture> {
  const answer = JSON.stringify({ verdict: "GREEN", owner: REVIEW_OWNER, summary: "The receipt reads right." });
  const provider = await serveLoopbackHttp((request, response) => {
    request.resume();
    if (request.method !== "POST" || !request.url?.endsWith("/responses")) {
      response.writeHead(404).end();
      return;
    }
    const completed = { type: "response.completed", sequence_number: 0, response: {
      id: "resp_fixture", object: "response", created_at: 1, model: REVIEW_MODEL, status: "completed",
      output: [{ type: "message", id: "msg_fixture", role: "assistant", status: "completed", content: [{ type: "output_text", text: answer, annotations: [] }] }],
      usage: { input_tokens: 48000, output_tokens: 200, total_tokens: 48200, input_tokens_details: { cached_tokens: 0 }, output_tokens_details: { reasoning_tokens: 0 } },
    } };
    response.writeHead(200, { "content-type": "text/event-stream" });
    response.end(`event: ${completed.type}\ndata: ${JSON.stringify(completed)}\n\n`);
  });
  try {
    const fixture = newFixture(own);
    declareArtifact(fixture.repo, ["report"], "report", `
[evals.agent]
title = "Review the receipt"
profile = { kind = "agent", backend = "openai", model = "${REVIEW_MODEL}", timeout_ms = 60000 }
payload.instruction = "Review the receipt."
pass_schema = { type = "object", properties = { summary = { type = "string" }, owner = { type = "string" } }, required = ["summary", "owner"], additionalProperties = false }
fail_schema = { type = "object", properties = { summary = { type = "string" } }, required = ["summary"], additionalProperties = false }
`);
    initRepo(fixture);
    const env = { OPENAI_API_KEY: "fixture-not-a-secret", ARTIFACTIZE_OPENAI_BASE_URL: provider.url };
    const verified = await artifactizeAsync(fixture, ["verify", "--all", "--wait", "--json"], env);
    if (verified.status !== 0) throw new Error(`the Agent eval did not pass: ${verified.stdout}${verified.stderr}`);
    return fixture;
  } finally {
    await provider.close();
  }
}
