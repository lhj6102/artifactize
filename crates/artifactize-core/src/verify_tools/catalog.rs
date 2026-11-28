//! The complete public tool surface; deliberately no credential or Human actions.
use super::{
    input::{DEFAULT_WAIT_SECONDS, MAX_WAIT_SECONDS, MIN_SCOPE_IDS, RUN_LIST_LIMIT, SCOPE_IDS},
    tool::{PluginTool, ReadTool, ToolId, VerifyTool},
};
use crate::{
    inspect::DEFAULT_LOG_TAIL_LINES, runs::RESULT_PAGE_SIZE, types::MAX_RUN_COMMENT_CHARS,
};
use serde_json::{Map, Value, json};
use std::borrow::Cow;

/// One tool's protocol-neutral wire contract, shared by MCP and Chat.
pub struct Tool {
    pub name: &'static str,
    pub description: Cow<'static, str>,
    pub schema: Map<String, Value>,
    pub hints: Hints,
}

pub struct Hints {
    pub open_world: bool,
    pub read_only: bool,
    pub destructive: bool,
    pub idempotent: bool,
}

/// Match core's ASCII RequestKey bound, keeping retry identities compact in stored receipts.
const REQUEST_KEY_CHARS: usize = 128;

/// Match the core log snapshot bound so one MCP response stays bounded.
const LOG_TAIL_LINES: usize = 200;
/// Match the core literal-search bound to limit scanning input per request.
const LOG_SEARCH_CHARS: usize = 2048;
/// Match core's identifier bound for a backend name, so a name no declaration can use is
/// refused before the provider lookup.
const BACKEND_NAME_CHARS: usize = 64;

// The outer object is typed; only each individual JSON Schema value needs `json!`.
macro_rules! object {
    ({$($key:literal : $value:tt),+ $(,)?}) => {
        Map::from_iter([$(($key.into(), json!($value))),+])
    };
}

pub fn tools() -> Vec<Tool> {
    let scope = json!({"oneOf": [
        {"type":"object","properties":{"kind":{"const":"all"}},"required":["kind"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"artifact"},"artifactId":{"type":"string"}},"required":["kind","artifactId"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"eval"},"evalId":{"type":"string"}},"required":["kind","evalId"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"artifacts"},"artifactIds":{"type":"array","items":{"type":"string"},"minItems":MIN_SCOPE_IDS,"maxItems":SCOPE_IDS}},"required":["kind","artifactIds"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"evals"},"evalIds":{"type":"array","items":{"type":"string"},"minItems":MIN_SCOPE_IDS,"maxItems":SCOPE_IDS}},"required":["kind","evalIds"],"additionalProperties":false}
    ]});
    let string = json!({"type":"string"});
    vec![
        tool(
            ReadTool::Plugins,
            "List the built-in and installed plugins by namespace.",
            Map::new(),
            &[],
        ),
        tool(
            PluginTool::Add,
            "Install a plugin folder from an absolute path. Only run for a package the user trusts.",
            object!({"path":string}),
            &["path"],
        ),
        tool(
            PluginTool::Remove,
            "Remove an installed plugin by its namespace.",
            object!({"namespace":string}),
            &["namespace"],
        ),
        tool(
            VerifyTool::Start,
            "Submit a durable Run; acceptance is not approval. Same caller key returns the same attempt. A comment, a short note on what you are working on, is shown to people with the Run and changes nothing it does. Speculation defaults off; concurrency is unlimited unless jobs is explicitly given; backend limits still apply. Sign-offs the Run selected never hold up other evals (one outside the scope does, and the Run ends incomplete until the scope includes it): the Run stays waiting_human while one is claimed or its idle grace runs, then ends yellow when only sign-offs remain, which is not full approval.",
            object!({"scope":scope,"jobs":{"type":"integer","minimum":1,"maximum":u32::MAX},"request_key":{"type":"string","pattern":format!("^[A-Za-z0-9._:-]{{1,{REQUEST_KEY_CHARS}}}$")},"comment":{"type":"string","minLength":1,"maxLength":MAX_RUN_COMMENT_CHARS},"speculate":{"type":"boolean","default":false}}),
            &["scope"],
        ),
        tool(
            VerifyTool::Status,
            "Compact saved Run state and counts.",
            object!({"run":string}),
            &["run"],
        ),
        tool(
            VerifyTool::Wait,
            "Observe Run end or bound, equally for all eval kinds. Disconnect/cancellation only drops this observer.",
            object!({"run":string,"seconds":{"type":"integer","minimum":0,"maximum":MAX_WAIT_SECONDS,"default":DEFAULT_WAIT_SECONDS}}),
            &["run"],
        ),
        tool(
            VerifyTool::Watch,
            "Subscribe for this bounded call: progress notifications carry JSON update/eval_result/outcome events when the client supplies _meta.progressToken. Reattachment replays saved results. Returns compact observation; only outcome/verify_result is final validation. No conversation injection or background subscription.",
            object!({"run":string}),
            &["run"],
        ),
        tool(
            VerifyTool::Result,
            format!(
                "Terminal verdict and at most {RESULT_PAGE_SIZE} concise findings; follow next_cursor. Active Runs return ready:false."
            ),
            object!({"run":string,"cursor":string}),
            &["run"],
        ),
        tool(
            VerifyTool::Cancel,
            "Explicitly request cancellation; committed evidence remains. Observe settlement for cleanup.",
            object!({"run":string}),
            &["run"],
        ),
        tool(
            ReadTool::Status,
            "Predict execute/reuse/wait using the CLI's status operation. May run trusted fingerprint commands.",
            object!({"scope":scope}),
            &[],
        ),
        tool(
            ReadTool::Graph,
            "Discover static Artifacts, eval criteria, input/consumer relations and cycles without running commands. Scope selects required inputs; impact includes downstream consumers and their inputs, conservatively, not exact cache invalidation.",
            object!({"scope":scope,"impact":{"type":"boolean","default":false}}),
            &[],
        ),
        tool(
            ReadTool::Logs,
            format!(
                "Inspect runtime stdout/stderr without the full request. Defaults to last {DEFAULT_LOG_TAIL_LINES} lines per stream; explicit evidence distinguishes full local capture, partial, saved audit and missing. Search is literal. Tail/range exclusive. next_start continues forward queries."
            ),
            object!({"id":string,"stream":{"enum":["stdout","stderr","both"],"default":"both"},"tail":{"type":"integer","minimum":1,"maximum":LOG_TAIL_LINES},"start":{"type":"integer","minimum":1},"end":{"type":"integer","minimum":1},"search":{"type":"string","minLength":1,"maxLength":LOG_SEARCH_CHARS}}),
            &["id"],
        ),
        tool(
            ReadTool::Config,
            "Validate declarations, without hashing or executing project tools.",
            Map::new(),
            &[],
        ),
        tool(
            ReadTool::Expand,
            "Each Artifact's binding as written (file, line, macro) and the declaration it expands to, as .artf code with macros and toolsets filled in. One Artifact, or all.",
            object!({"artifact":string}),
            &[],
        ),
        tool(
            ReadTool::Symbols,
            "Every global binding of the project and its plugins: Artifacts, tools, profiles and evals, macros with their parameters and defaults, and where each is declared.",
            Map::new(),
            &[],
        ),
        tool(
            ReadTool::Runs,
            "List this repository's saved Runs, newest first. Offset selects older records.",
            object!({"limit":{"type":"integer","minimum":0,"maximum":RUN_LIST_LIMIT,"default":RUN_LIST_LIMIT},"offset":{"type":"integer","minimum":0}}),
            &[],
        ),
        tool(
            ReadTool::Run,
            "Read saved Run audit, same JSON as CLI run show.",
            object!({"run":string}),
            &["run"],
        ),
        tool(
            ReadTool::Requests,
            "Read requests from this repository, optionally one Run. No sign-off authority.",
            object!({"run":string}),
            &[],
        ),
        tool(
            ReadTool::Request,
            "Read saved request audit and criteria. No claim or submit.",
            object!({"id":string}),
            &["id"],
        ),
        tool(
            ReadTool::Session,
            "Read saved Agent conversation or summary. Never send a follow-up.",
            object!({"reference":string,"summary":{"type":"boolean","default":false}}),
            &["reference"],
        ),
        tool(
            ReadTool::Tools,
            "Static preflight only; never execute tools or start sessions.",
            object!({"eval":string,"artifact":string,"audience":{"enum":["agent","human"]},"tool":string}),
            &[],
        ),
        tool(
            ReadTool::Doctor,
            "Inspect backend and state readiness without changing credentials.",
            Map::new(),
            &[],
        ),
        tool(
            ReadTool::Models,
            "List models available from the named backend: a built-in provider (codex, claude-agent, antigravity, openai, anthropic, gemini) or a custom provider set up on this machine.",
            object!({"provider":{"type":"string","pattern":format!("^[A-Za-z0-9][A-Za-z0-9_-]{{0,{}}}$", BACKEND_NAME_CHARS - 1)}}),
            &["provider"],
        ),
        tool(
            ReadTool::Remote,
            "Inspect configured review store, token source and reachability; never disclose the token.",
            Map::new(),
            &[],
        ),
        tool(
            ReadTool::Cache,
            "Read latest reusable record, or full history; never remove a record.",
            object!({"key":string,"history":{"type":"boolean","default":false}}),
            &["key"],
        ),
    ]
}
fn tool(
    id: impl Into<ToolId>,
    description: impl Into<Cow<'static, str>>,
    mut properties: Map<String, Value>,
    required: &[&str],
) -> Tool {
    let id = id.into();
    let description = description.into();
    let description = if matches!(id, ToolId::Read(_)) {
        properties.insert("cursor".into(), json!({"type":"string"}));
        format!(
            "{description} Returns bounded pages of exact CLI JSON: concatenate entries.text, following next_cursor, then parse JSON."
        ).into()
    } else {
        description
    };
    properties.insert(
        "project".into(),
        json!({"type":"string","description":"The absolute project folder; needed only when the host names no project."}),
    );
    let schema = object!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    Tool {
        name: id.name(),
        description,
        schema,
        hints: annotations(id),
    }
}

enum Effect {
    ReadOnly,
    Mutating(Mutation),
}

enum Mutation {
    NonIdempotent,
    IdempotentDestructive,
}

enum Reach {
    Closed,
    Open,
}

fn annotations(id: ToolId) -> Hints {
    use ReadTool as Read;
    use VerifyTool as Verify;
    let (effect, reach) = match id {
        ToolId::Verify(Verify::Start) => (Effect::Mutating(Mutation::NonIdempotent), Reach::Open),
        ToolId::Verify(Verify::Cancel) => (
            Effect::Mutating(Mutation::IdempotentDestructive),
            Reach::Closed,
        ),
        ToolId::Plugin(PluginTool::Add) => {
            (Effect::Mutating(Mutation::NonIdempotent), Reach::Closed)
        }
        ToolId::Plugin(PluginTool::Remove) => (
            Effect::Mutating(Mutation::IdempotentDestructive),
            Reach::Closed,
        ),
        ToolId::Read(Read::Status) => (Effect::Mutating(Mutation::NonIdempotent), Reach::Closed),
        ToolId::Read(Read::Models | Read::Remote) => (Effect::ReadOnly, Reach::Open),
        ToolId::Verify(Verify::Status | Verify::Wait | Verify::Watch | Verify::Result)
        | ToolId::Read(
            Read::Graph
            | Read::Logs
            | Read::Config
            | Read::Expand
            | Read::Symbols
            | Read::Runs
            | Read::Run
            | Read::Requests
            | Read::Request
            | Read::Session
            | Read::Tools
            | Read::Doctor
            | Read::Cache
            | Read::Plugins,
        ) => (Effect::ReadOnly, Reach::Closed),
    };
    Hints {
        open_world: matches!(reach, Reach::Open),
        read_only: matches!(effect, Effect::ReadOnly),
        destructive: matches!(effect, Effect::Mutating(Mutation::IdempotentDestructive)),
        idempotent: !matches!(effect, Effect::Mutating(Mutation::NonIdempotent)),
    }
}
