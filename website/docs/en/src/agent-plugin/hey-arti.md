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

- Cut Artifacts where work is edited and judged independently, each with a concrete
  obligation. Avoid one review over the whole repository.
- An eval names each input it really uses as `{name}`; sharing a repository is not a
  dependency, and a command does not add the files it reads. A child folder counts
  as an input of its parent's evals.
- One obligation per eval. Use a runtime eval when a command can check it, an
  [Agent eval](../features/agent.md) when it needs judgment, and a
  [Human eval](../features/human.md) only where a person must add judgment or
  authority. A person is asked only after the Artifact's runtime and Agent evals are
  GREEN.
- Criteria for one Artifact go inside its eval; rules many Artifacts share go in a
  rule document their evals read.
- Keep result schemas small: a summary on GREEN, concrete findings on RED (syntax
  in [Declarations](../features/declarations.md)).
- Keep the default fingerprint (artifactsum); give reviewers only the read-only
  tools they need.

Combinations it recommends, with who owns each check and what it references:

- **Library and consumer**: the library owns its tests; the consumer's review
  references `{library}`.
- **Spec and app**: the app owns its tests (runtime) and a review against `{spec}`
  (Agent).
- **App and E2E tests**: running the suite is the app's eval, with `{e2e-tests}` in
  its arguments; the test code's own evals review its style and coverage.
- **Docs and assets**: the docs are reviewed against the code they describe; each
  asset is its own file Artifact, with a person's sign-off when it needs one.
- **Monorepo contract**: UI and API each own their checks; an integration eval
  references both and the contract between them.
- **Generated files**: either review the generator and its inputs, or review the
  output with the generator referenced.
- **Rule documents**: the rules are an Artifact, often with a Human sign-off, read
  by the evals of the Artifacts they govern.

It steers away from implicit dependencies, whole-repository reviews, anti-cheat
evals, unrequested execution options, and weakening an eval to reach GREEN.

To show you a pattern, it points at the example that matches your situation: the
[examples](../getting-started/examples.md) (runtime relations, Agent tools with a
sign-off, posts as file Artifacts), the demo projects `website/demo/shop` (a page
checked against the style tokens it uses) and `website/demo/brand` (a signed-off
logo embedded by a page), and artifactize's own repository (rule documents with a
sign-off read by the evals of the code they govern, and an app whose eval runs its
E2E suite). The skill ends with a small starter set of declarations to adapt.

Changing an eval's instruction, result schema or runtime command changes its
judgment; tags, titles, execution options and tool declarations do not. After an
edit, `status` should show the edited input and its real consumers needing review
and independent parts still reused. See [Fingerprints and reuse](../features/fingerprint.md).

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
action, then resumes by observing the same Run. Yielding is not cancellation, and no
keep-alive turns or polling loops are needed.

## Team and CI

Use the existing [team review store](../features/team/index.md) and keep credentials
out of declarations, skill copies, transcripts and MCP registration. CI uses a
read-only token and `artifactize verify --all --wait --reuse-only agent,human`, so it
runs runtime checks and reuses Agent and Human evidence; see the
[CI guide](../features/team/ci.md).
