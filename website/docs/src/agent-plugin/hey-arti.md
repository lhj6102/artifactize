# Hey Arti: shape a project's reviews

**Arti is artifactize's Forward Deployed Engineer.** Given to your coding assistant,
it learns your project and recommends how to shape it into Artifacts and evals, so
that reviews stay meaningful and reusable. It works like a good waiter: it knows the
menu and which combinations work, listens before recommending, confirms the order,
and leaves the cooking to artifactize. It does not build or run your application,
and it never approves anything in a person's place. (The name comes from
*Artifactizer*.)

Its instruction guide, called a **skill**, is
[`SKILL.md`](https://github.com/lhj6102/artifactize/blob/main/skills/hey-arti/SKILL.md),
in the repository's `skills/` folder. There is one body, not separate instructions per
host. It is short on purpose: these docs are its **source of truth**. It maps each
concept to the page that owns it and has Arti read a page before explaining or
writing what it is unsure of; when a page and the skill disagree, the page wins.
Then come how Arti treats you as its client, its work processes by situation, and
the best practices and examples it draws from. Plugin-marketplace installation is separate; no plugin exporter or release
bundle is required for the MCP registration below.

## Register the MCP server

Follow [MCP setup](mcp.md#register-the-mcp-server) to connect your coding host.
Registration supplies tools; it does not install the skill. Open the linked
`SKILL.md`, attach the unchanged file to your assistant's conversation and ask, for
example: "Follow this guide. Learn this repository and propose Artifacts and evals
before editing." Use that canonical guide rather than maintaining an edited copy.

## What Arti does with you

Arti first works out which situation you are in, and each has its own process:

- **A question about artifactize or one of its concepts**: it answers from the page
  that owns the concept, with an example from your project, and offers a next step.
- **Whether artifactize fits, and where to start**: it asks what goes wrong today,
  names the few places it would help most (and where it would not), and suggests
  one small first Artifact.
- **Shaping a new project**: the steps below.
- **Adopting it gradually in a large repository**: it declares the area that hurts
  most first and leaves the rest undeclared rather than under one broad review.
- **Auditing an existing setup**: it checks the current Artifacts and evals against
  the practices below, reports each finding with its fix, and changes only what you
  agree to.
- **The project grew or changed**: it decides with you whether a new component is
  its own Artifact, splits one that grew, and compares `status` before and after.
- **"Why did this run again?" or "verify is slow"**: it reads `status` and proposes
  a boundary fix (too wide, or a missing consumer reference), not an execution option.
- **A result to explain or fix**, **a reviewer that needs the running application**
  (a tool session), **team sharing and CI**, and **the Human boundary**: covered in
  the sections below.

When shaping a new project, it:

1. **Learns the project.** It reads code, docs, specifications, assets and build
   and test commands, then interviews you for what it cannot read: the goals, what
   changes together, which judgments need a person and what CI must guarantee.
2. **Proposes a composition.** It suggests each Artifact and its evals (who judges,
   against which inputs, by which criteria) with reasons, and waits for your
   agreement before changing structure. It keeps your project instructions and
   existing Human responsibilities, explains before adding a new sign-off, and
   never marks unfinished work as `basis`.
3. **Writes small declarations** in today's [format](../features/declarations.md),
   validates them with `config_check`, and shows with `status` what would run and
   what would be reused. It then tells you what changed.
4. **Hands verification over** and follows the Run (below).

On an existing project it checks the current structure against the same practices
and changes only what you agree to. On a RED result it explains the finding in your
terms and fixes the Artifact the eval judges, such as inaccurate docs or criteria,
rather than bending code or weakening a review.

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

Arti uses `config_graph` to discover Artifacts, evals and dependencies (`impact:true`
adds downstream consumers conservatively), `config_check` to validate declarations
and `status` to predict which evals execute, reuse or wait. `tools_check` inspects
tools and sessions statically. Inspection tools return bounded UTF-8 fragments of
their CLI JSON in `entries[].text`; append continuation pages until `next_cursor` is
null, then parse. See the [MCP reference](mcp.md).

MCP does not expose login/logout, Human claim/submit/release, prune/cache removal,
remote push, TUIs, the store server, tool execution or session follow-ups. If MCP is
not registered, the CLI fallback is `artifactize config check`, `artifactize config
graph`, `artifactize status --all` and `artifactize tools check`.

## Tool sessions

When a reviewer must inspect something running, the skill shows a session declared
with `views.agent_sessions.<name>.start`/`stop` and a command tool bound with
`session = "<name>"`, its program inside the Artifact so the fingerprint covers it.
artifactize starts sessions on the first bound tool call, then stops and cleans
them up at review end; there are no lifetime or port
options, and `execution_paths` record provenance, not reuse inputs. Declared
commands are trusted local programs, **not an OS sandbox**. See
[Tool sessions](../features/tools/agent.md#tool-sessions).

## Follow a durable Run

Arti starts with `verify_start`, keeps the returned Run ID, and follows with
`verify_watch` when the host shows MCP progress or `verify_wait` otherwise, reading
findings with `verify_result`. An accepted Run is not a passing one. It diagnoses a
failed runtime finding with `request_logs` and an Agent finding with `session_show`.
artifactize owns the Run: closing the terminal, disconnecting or ending the agent's
turn does not stop it; only `verify_cancel` requests cancellation. See
[Run](../features/run.md) and [streaming](mcp.md#streaming-and-focused-diagnosis).

Two per-request choices default off: `speculate` starts downstream work early, at
the cost of work an upstream RED cancels; `defer_human` lets machine work finish
while sign-offs wait, ending SIGNOFFS_PENDING, which is **not full approval**.
`gate = "blocking"` keeps a sign-off a hard gate. Neither choice may be used to get
around a required person.

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
