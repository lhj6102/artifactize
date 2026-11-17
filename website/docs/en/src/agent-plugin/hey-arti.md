# Hey Arti: shape a project's reviews

Ask your coding assistant "Hey Arti, how should this project be reviewed?". Arti
learns your project, asks what matters to you, and proposes Artifacts and evals so that
reviews stay meaningful and reusable; it changes declarations only once you agree. It does
not build or run your application, and it never approves anything in a person's place.

Its instruction guide, called a **skill**, is
[`SKILL.md`](https://github.com/lhj6102/artifactize/blob/main/agent-plugin/skills/hey-arti/SKILL.md),
and comes with the [plugin](plugin.md).

## Get the skill

In Claude Code and Codex, install the [plugin](plugin.md): it brings this skill and
the MCP server together, and the plugin's skill is this same file. Arti first checks
that artifactize is installed; if it is not, it points you to the
[installation page](../getting-started/install.md) and offers your system's default,
which it runs only when you ask it to.

In other hosts, follow [MCP setup](mcp.md#register-the-mcp-server) to connect your
coding host. Registration supplies tools; it does not install the skill. Open the
linked `SKILL.md`, attach the unchanged file to your assistant's conversation and
ask, for example: "Follow this guide. Learn this repository and propose Artifacts
and evals before editing." Use that canonical guide rather than maintaining an
edited copy.

## What Arti does with you

Ask in your own words. Arti works out which situation you are in, and each has its
own process; it changes only what you agree to:

- **A question about a concept**: an answer from the page that owns it, with an
  example from your project, and a next step.
- **Whether it fits, and where to start**: it asks what goes wrong in your reviews
  and CI today, names two or three places it would help most and where it would
  not, and suggests one Artifact with one or two evals as the first step.
- **Shaping a new project**: the steps below.
- **Adopting it gradually in a large repository**: it declares only the area that
  hurts most, leaves the rest undeclared rather than under one broad review, and
  adds the next area once you trust the first one's verdicts.
- **Auditing an existing setup**: it runs `config_check` and `config_graph`, checks
  the result against the [practices](#the-practices-it-recommends), and reports
  each finding with its fix before you agree to any edit.
- **The project grew or changed**: it agrees with you whether a new component is its
  own Artifact and which evals consume it, splits an Artifact that grew along what
  is edited independently, and compares `status` before and after.
- **"Why did this run again?" or "verify is slow"**: it reads `status`. Too much
  rerunning usually means a boundary too wide or a fingerprint covering files that
  do not matter; too little, a consumer that does not name an input it uses. It
  proposes that fix, not an execution option.
- **A result to explain**, **a reviewer that needs the running application**,
  **team sharing and CI**: the sections below.

When shaping a new project, it:

1. **Learns the project.** It reads code, docs, specifications, assets and build
   and test commands, then interviews you for what it cannot read: the goals, what
   changes together, which judgments need a person and what CI must guarantee.
2. **Proposes a composition.** It suggests each Artifact and its evals (who judges,
   against which inputs, by which criteria) with reasons, and waits for your
   agreement before changing structure. It keeps your project instructions and
   existing Human responsibilities, explains before adding a new sign-off, and
   marks an input `basis = true` only when your team agrees nobody reviews it,
   never to get around unfinished reviews.
3. **Writes small declarations** in today's [format](../features/declarations.md),
   validates them with `config_check`, and shows with `status` what would run and
   what would be reused. It then tells you what changed.
4. **Hands verification over** and follows the Run (below).

## The practices it recommends

Arti draws boundaries around independently edited and reviewed work, not the whole
repository, with one obligation per eval. It [defines nodes](../features/declarations.md)
first with an explicit `path`, even before targets exist; definition folders only organize
files. It helps with `config migrate` and `config rename OLD NEW`.

Each eval names the inputs it uses as `{name}`. Only explicit `parts` creates
[composition](../features/artifact.md#composition), including shared, file and aspect
parts: each part checks its own work, the whole checks integration, and its evals still
name the parts they use. Location alone creates neither composition nor dependencies.

Arti recommends Runtime for commands, [Agent](../features/agent.md) for judgment and
[Human](../features/human.md) for a person's authority, after the machine evals pass.
It keeps Artifact-specific criteria inline, shared rules in referenced rule Artifacts,
and [result schemas](../features/declarations.md#owner-result-schemas) small. Local
targets keep [artifactsum](../features/fingerprint.md); reviewers get only needed read-only tools.
[Live checks](../features/declarations.md#live-artifacts) observe rather than deploy:
commands receive URIs at the project root with the author's environment, Runtime evals
run on every verify, and reusable Agent/Human reviews need script snapshots, not file tools.

Its patterns pair libraries with consumers, apps with specs and E2E test code, docs
with the code they describe, assets with sign-offs, UI and API with a shared contract,
and generators and inputs with their outputs. The app owns E2E execution; test-code
evals judge style and coverage. Shared rules are reviewed Artifacts read by the checks they govern.

Arti avoids implicit dependencies, whole-repository reviews, checks added only to catch
builder cheating, unrequested execution options, and weakening criteria to reach GREEN.
It points you to [examples](../getting-started/examples.md), including
[Live observations](https://github.com/lhj6102/artifactize/tree/main/examples/live),
`website/demo/shop` (a page checked against style tokens), `website/demo/brand` (a signed-off
logo used by a page), or artifactize's rule documents and app-owned E2E suite. The skill
ends with starter declarations to adapt.

After an edit, Arti checks `status` for the changed input and its actual consumers,
leaving independent parts reused. Instructions, result schemas and runtime commands
change judgment; tags, titles, execution options and tool declarations do not. See
[Fingerprints and reuse](../features/fingerprint.md).

## Use MCP first

Ask Arti which Artifacts, evals and dependencies your project declares and what a change
affects, whether the declarations are valid, which evals would run, be reused or wait,
and whether their tools and sessions are set up. Arti answers from artifactize's
[MCP tools](mcp.md) when they are registered, and otherwise from
`artifactize config check`, `artifactize config graph`, `artifactize status --all` and
`artifactize tools check`.

## Tool sessions

When a reviewer must inspect something running, the skill shows a session declared
with `views.agent_sessions.<name>.start`/`stop` and a command tool bound with
`session = "<name>"`, its program inside the Artifact so the fingerprint covers it.
artifactize owns the session's lifetime; see
[Tool sessions](../features/tools/agent.md#tool-sessions).

## Follow a durable Run

Arti starts with `verify_start`, keeps the returned Run ID, and follows with
`verify_watch` when the host shows MCP progress or `verify_wait` otherwise, reading
findings with `verify_result`. An accepted Run is not a passing one. It diagnoses a
failed runtime finding with `request_logs` and an Agent finding with `session_show`.
artifactize owns the Run: closing the terminal, disconnecting or ending the agent's
turn does not stop it; only `verify_cancel` requests cancellation. See
[Run](../features/run.md) and [streaming](mcp.md#streaming-and-focused-diagnosis).

Arti may ask for [speculation](../features/run.md#speculation) per request. A sign-off the Run
selected never holds up other evals; one outside the selection does, and the Run ends
INCOMPLETE until the scope includes its Artifact or is `{"kind": "all"}`. A Run that ends
YELLOW is **not full approval**: a required person still signs off.

How Arti treats each result:

- **GREEN**: done for this content.
- **RED**: explains the finding in your terms and fixes the Artifact the eval judges,
  such as inaccurate docs or criteria, rather than bending correct code to match them
  or weakening a review, then verifies again.
- **BLOCKED**: something the eval waits on failed, usually a RED or ERROR eval of an
  input it uses. Arti fixes that first, then verifies again.
- **ERROR**: the check could not run. Arti reads `request_logs` or `session_show` and
  repairs the cause, such as a missing program, never the verdict.
- **YELLOW**: everything else passed and a person's sign-off waits. It is
  not full approval; Arti hands off (below).
- **INCOMPLETE**: required evidence is missing, for example where CI only reuses
  existing reviews. Arti gets that evidence with a verify that runs the missing evals.

## Hand off to the person, then yield

Arti names waiting requests with `request_list`/`request_show` and gives you the
criteria and `artifactize review REQUEST_ID` (or your usual review interface). It
never claims, approves or submits a Human review. It continues independent work, or
yields with a short handoff naming the repository, Run, waiting requests and next
action. While the Run is open, it resumes by observing that Run. Once the Run ended
YELLOW, its verdict is final and your later sign-off updates only the request, so Arti
verifies again and the new Run reuses your sign-off. Yielding is not cancellation, and
no keep-alive turns or polling loops are needed.

## Team and CI

Use the existing [team review store](../features/team/index.md) and keep credentials
out of declarations, skill copies, transcripts and MCP registration. CI uses a
read-only token and `artifactize verify --all --wait --reuse-only agent,human`, so it
runs runtime checks and reuses Agent and Human evidence; see the
[CI guide](../features/team/ci.md).
