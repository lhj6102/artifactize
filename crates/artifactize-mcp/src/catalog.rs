//! The complete public tool surface; deliberately no credential or Human actions.
use super::input::SCOPE_IDS;
use super::{DEFAULT_WAIT_SECONDS, MAX_WAIT_SECONDS, RUN_LIST_LIMIT};
use rmcp::model::{Tool, ToolAnnotations};
use serde_json::{Value, json};

/// Match core's ASCII RequestKey bound, keeping retry identities compact in stored receipts.
const REQUEST_KEY_CHARS: usize = 128;

/// Match the core log snapshot bound so one MCP response stays bounded.
const LOG_TAIL_LINES: usize = 200;
/// Match the core literal-search bound to limit scanning input per request.
const LOG_SEARCH_CHARS: usize = 2048;
/// Match core's identifier bound for a backend name, so a name no declaration can use is
/// refused before the provider lookup.
const BACKEND_NAME_CHARS: usize = 64;

pub(super) fn tools() -> Vec<Tool> {
    let scope = json!({"oneOf": [
        {"type":"object","properties":{"kind":{"const":"all"}},"required":["kind"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"artifact"},"artifactId":{"type":"string"}},"required":["kind","artifactId"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"eval"},"evalId":{"type":"string"}},"required":["kind","evalId"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"artifacts"},"artifactIds":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":SCOPE_IDS}},"required":["kind","artifactIds"],"additionalProperties":false},
        {"type":"object","properties":{"kind":{"const":"evals"},"evalIds":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":SCOPE_IDS}},"required":["kind","evalIds"],"additionalProperties":false}
    ]});
    let string = json!({"type":"string"});
    vec![
        tool(
            "verify_start",
            "Submit a durable Run; acceptance is not approval. Same caller key returns the same attempt. Speculation defaults off; concurrency is unlimited unless jobs is explicitly given; backend limits still apply. Sign-offs the Run selected never hold up other evals (one outside the scope does, and the Run ends incomplete until the scope includes it): the Run stays waiting_human while one is claimed or its idle grace runs, then ends yellow when only sign-offs remain, which is not full approval.",
            json!({"scope":scope,"jobs":{"type":"integer","minimum":1,"maximum":u32::MAX},"request_key":{"type":"string","pattern":format!("^[A-Za-z0-9._:-]{{1,{REQUEST_KEY_CHARS}}}$")},"speculate":{"type":"boolean","default":false}}),
            &["scope"],
            false,
        ),
        tool(
            "verify_status",
            "Compact saved Run state and counts.",
            json!({"run":string}),
            &["run"],
            false,
        ),
        tool(
            "verify_wait",
            "Observe Run end or bound, equally for all eval kinds. Disconnect/cancellation only drops this observer.",
            json!({"run":string,"seconds":{"type":"integer","minimum":0,"maximum":MAX_WAIT_SECONDS,"default":DEFAULT_WAIT_SECONDS}}),
            &["run"],
            false,
        ),
        tool(
            "verify_watch",
            "Subscribe for this bounded call: progress notifications carry JSON update/eval_result/outcome events when the client supplies _meta.progressToken. Reattachment replays saved results. Returns compact observation; only outcome/verify_result is final validation. No conversation injection or background subscription.",
            json!({"run":string}),
            &["run"],
            false,
        ),
        tool(
            "verify_result",
            "Terminal verdict and at most 20 concise findings; follow next_cursor. Active Runs return ready:false.",
            json!({"run":string,"cursor":string}),
            &["run"],
            false,
        ),
        tool(
            "verify_cancel",
            "Explicitly request cancellation; committed evidence remains. Observe settlement for cleanup.",
            json!({"run":string}),
            &["run"],
            false,
        ),
        tool(
            "status",
            "Predict execute/reuse/wait using the CLI's status operation. May run trusted fingerprint commands.",
            json!({"scope":scope}),
            &[],
            true,
        ),
        tool(
            "config_graph",
            "Discover static Artifacts, eval criteria, input/consumer relations and cycles without running commands. Scope selects required inputs; impact includes downstream consumers and their inputs, conservatively, not exact cache invalidation.",
            json!({"scope":scope,"impact":{"type":"boolean","default":false}}),
            &[],
            true,
        ),
        tool(
            "request_logs",
            "Inspect runtime stdout/stderr without the full request. Defaults to last 50 lines per stream; explicit evidence distinguishes full local capture, partial, saved audit and missing. Search is literal. Tail/range exclusive. next_start continues forward queries.",
            json!({"id":string,"stream":{"enum":["stdout","stderr","both"],"default":"both"},"tail":{"type":"integer","minimum":1,"maximum":LOG_TAIL_LINES},"start":{"type":"integer","minimum":1},"end":{"type":"integer","minimum":1},"search":{"type":"string","minLength":1,"maxLength":LOG_SEARCH_CHARS}}),
            &["id"],
            true,
        ),
        tool(
            "config_check",
            "Validate declarations, without hashing or executing project tools.",
            json!({}),
            &[],
            true,
        ),
        tool(
            "run_list",
            "List this repository's saved Runs, newest first. limit <=20; offset selects older records.",
            json!({"limit":{"type":"integer","minimum":0,"maximum":RUN_LIST_LIMIT,"default":RUN_LIST_LIMIT},"offset":{"type":"integer","minimum":0}}),
            &[],
            true,
        ),
        tool(
            "run_show",
            "Read saved Run audit, same JSON as CLI run show.",
            json!({"run":string}),
            &["run"],
            true,
        ),
        tool(
            "request_list",
            "Read requests from this repository, optionally one Run. No sign-off authority.",
            json!({"run":string}),
            &[],
            true,
        ),
        tool(
            "request_show",
            "Read saved request audit and criteria. No claim or submit.",
            json!({"id":string}),
            &["id"],
            true,
        ),
        tool(
            "session_show",
            "Read saved Agent conversation or summary. Never send a follow-up.",
            json!({"reference":string,"summary":{"type":"boolean","default":false}}),
            &["reference"],
            true,
        ),
        tool(
            "tools_check",
            "Static preflight only; never execute tools or start sessions.",
            json!({"eval":string,"artifact":string,"audience":{"enum":["agent","human"]},"tool":string}),
            &[],
            true,
        ),
        tool(
            "doctor",
            "Inspect backend and state readiness without changing credentials.",
            json!({}),
            &[],
            true,
        ),
        tool(
            "models",
            "List models available from the named backend: a built-in provider (codex, openai, anthropic, gemini) or a custom provider set up on this machine.",
            json!({"provider":{"type":"string","pattern":format!("^[A-Za-z0-9][A-Za-z0-9_-]{{0,{}}}$", BACKEND_NAME_CHARS - 1)}}),
            &["provider"],
            true,
        ),
        tool(
            "remote_status",
            "Inspect configured review store, token source and reachability; never disclose the token.",
            json!({}),
            &[],
            true,
        ),
        tool(
            "cache_show",
            "Read latest reusable record, or full history; never remove a record.",
            json!({"key":string,"history":{"type":"boolean","default":false}}),
            &["key"],
            true,
        ),
    ]
}
fn tool(
    name: &'static str,
    description: &'static str,
    mut properties: Value,
    required: &[&str],
    paged: bool,
) -> Tool {
    let description = if paged {
        properties["cursor"] = json!({"type":"string"});
        format!(
            "{description} Returns bounded pages of exact CLI JSON: concatenate entries.text, following next_cursor, then parse JSON."
        )
    } else {
        description.to_owned()
    };
    properties["project"] = json!({"type":"string","description":"The absolute project folder; needed only when the host names no project."});
    let schema = json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    Tool::new(
        name,
        description,
        schema.as_object().expect("schema is an object").clone(),
    )
    .with_annotations(
        ToolAnnotations::new()
            .read_only(!matches!(name, "verify_start" | "verify_cancel" | "status"))
            .destructive(name == "verify_cancel")
            .idempotent(!matches!(name, "verify_start" | "status"))
            .open_world(matches!(
                name,
                "verify_start" | "status" | "models" | "remote_status"
            )),
    )
}
