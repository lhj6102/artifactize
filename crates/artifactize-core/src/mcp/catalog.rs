//! The complete public tool surface; deliberately no credential or Human actions.
use super::input::SCOPE_IDS;
use super::{DEFAULT_WAIT_SECONDS, MAX_WAIT_SECONDS, RUN_LIST_LIMIT};
use rmcp::model::{Tool, ToolAnnotations};
use serde_json::{Value, json};

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
            "Submit a durable Run; acceptance is not approval. Same caller key returns the same attempt. Modes default off; concurrency is unlimited unless jobs is explicitly given; backend limits still apply. Provisional sign-offs are not full approval.",
            json!({"scope":scope,"jobs":{"type":"integer","minimum":1,"maximum":u32::MAX},"request_key":{"type":"string","pattern":"^[A-Za-z0-9._:-]{1,128}$"},"speculate":{"type":"boolean","default":false},"defer_human":{"type":"boolean","default":false}}),
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
            "List models available from the named backend.",
            json!({"provider":{"enum":["openai","anthropic","codex"]}}),
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
