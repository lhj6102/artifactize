//! An Agent eval's `resultCheck`: a project command that checks the parsed result and the
//! tool-call audit before the review completes. Its errors go to the one repair turn.
//!
//! It runs as a runtime command does: literal argv with `{artifact}` references resolved
//! in the eval's scope, the target Artifact's folder as cwd, private directories below
//! the Run's output, a filtered environment, a deadline and process-group cleanup.

use std::path::Path;

use serde_json::{Value, json};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::error::{Code, Failure};
use crate::{
    config::{Eval, RepoConfig, ResultCheck},
    process, runtime, scope,
};

/// The stdin protocol version. 2 gives `family` the `{name, material}` object that
/// fingerprint scripts and `json` tools receive.
const VERSION: u64 = 2;
const MAX_ERRORS: usize = 8;
const MAX_OUTPUT: usize = 4 * 1024;

fn failed(message: impl Into<String>) -> Failure {
    Failure::new(Code::ResultCheckFailed, message)
}

/// Run the check on a parsed result. `Ok` holds its errors, empty when the result passes.
#[expect(
    clippy::too_many_arguments,
    reason = "the eval and its check, the result with its audit, and the review's bounds"
)]
pub(super) async fn run(
    config: &RepoConfig,
    eval: &Eval,
    check: &ResultCheck,
    result: &Value,
    tool_calls: &[Value],
    output: &Path,
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<Vec<String>, Failure> {
    let unusable = |error: String| failed(format!("resultCheck could not start: {error}"));
    let scope = scope::eval_scope(config, eval).map_err(|e| unusable(e.to_string()))?;
    let args = scope::resolve_argv(config, &scope, &eval.target, &check.args)
        .map_err(|e| unusable(e.to_string()))?;
    let cwd = scope
        .resolve_input(&config.root, &eval.target, "")
        .map_err(|e| unusable(e.to_string()))?;
    let mut command = runtime::Command::prepare(
        check.command.clone().into(),
        args.into_iter().map(Into::into).collect(),
        &config.root,
        output,
        check.timeout_ms,
    )
    .map_err(|e| unusable(e.to_string()))?;
    let family = match &config.artifacts[&eval.target].family {
        Some(family) => {
            for material in &family.material {
                scope::scoped_path(&cwd, Path::new(material))
                    .map_err(|e| unusable(e.to_string()))?;
            }
            json!({"name":family.name,"material":family.material})
        }
        None => Value::Null,
    };
    command.cwd = cwd;
    let timeout = command.timeout();
    let calls: Vec<_> = tool_calls
        .iter()
        .map(|call| json!({"name":call["name"],"arguments":call["arguments"],"isError":call["isError"]}))
        .collect();
    let input = json!({
        "version": VERSION,
        "artifactId": eval.target,
        "family": family,
        "result": result,
        "toolCalls": calls,
    });
    // One byte past the limit is enough to tell an oversized output.
    let finished = tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Err(Failure::cancelled()),
        _ = tokio::time::sleep_until(deadline) => return Err(Failure::timeout()),
        finished = command.tool_output(input.to_string().into_bytes(), MAX_OUTPUT + 1, cancellation.clone()) => finished,
    };
    let finished = match finished {
        Ok(finished) => finished,
        Err(process::Error::Cancelled) => return Err(Failure::cancelled()),
        Err(process::Error::Timeout) => {
            return Err(failed(format!(
                "resultCheck timed out after {} ms.",
                timeout.as_millis()
            )));
        }
        Err(error) => return Err(failed(format!("resultCheck failed: {error}."))),
    };
    if !finished.status.success() {
        let stderr = String::from_utf8(runtime::clean_output(&finished.stderr))
            .expect("clean output is UTF-8");
        let stderr: String = stderr.trim().chars().take(500).collect();
        let status = match finished.status.code() {
            Some(code) => format!("exit status {code}"),
            None => "a signal".into(),
        };
        return Err(failed(if stderr.is_empty() {
            format!("resultCheck ended with {status}.")
        } else {
            format!("resultCheck ended with {status}: {stderr}")
        }));
    }
    errors(&finished.stdout)
}

/// `{"errors": [string]}`: at most 8 non-empty errors in at most 4 KiB.
fn errors(stdout: &[u8]) -> Result<Vec<String>, Failure> {
    if stdout.len() > MAX_OUTPUT {
        return Err(failed("resultCheck output exceeds 4 KiB."));
    }
    let invalid =
        || failed(r#"resultCheck must print one JSON object, {"errors": [string, ...]}."#);
    let value: Value = serde_json::from_slice(stdout).map_err(|_| invalid())?;
    let object = value
        .as_object()
        .filter(|object| object.len() == 1)
        .ok_or_else(invalid)?;
    let errors = object
        .get("errors")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    if errors.len() > MAX_ERRORS {
        return Err(failed(format!(
            "resultCheck returned {} errors; at most {MAX_ERRORS} are allowed.",
            errors.len()
        )));
    }
    errors
        .iter()
        .map(|error| {
            let error = error.as_str().ok_or_else(invalid)?;
            let error = String::from_utf8(runtime::clean_output(error.trim().as_bytes()))
                .expect("clean output is UTF-8");
            if error.is_empty() {
                Err(failed("resultCheck returned an empty error."))
            } else {
                Ok(error)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_is_one_bounded_error_list() {
        assert_eq!(errors(br#"{"errors":[]}"#).unwrap(), Vec::<String>::new());
        assert_eq!(
            errors(b"{\"errors\":[\" R2 is not cited \\u001b[31m\"]}\n").unwrap(),
            ["R2 is not cited \u{1b}[31m".replace("\u{1b}[31m", "")]
        );
        for (stdout, expected) in [
            ("", "must print one JSON object"),
            ("[]", "must print one JSON object"),
            (r#"{"errors":"one"}"#, "must print one JSON object"),
            (
                r#"{"errors":[],"warnings":[]}"#,
                "must print one JSON object",
            ),
            (r#"{"errors":[1]}"#, "must print one JSON object"),
            (r#"{"errors":[" "]}"#, "an empty error"),
        ] {
            let failure = errors(stdout.as_bytes()).unwrap_err();
            assert_eq!(failure.code, Code::ResultCheckFailed);
            assert!(failure.message.contains(expected), "{stdout}: {failure}");
        }
        let nine = json!({"errors": vec!["e"; 9]}).to_string();
        assert!(
            errors(nine.as_bytes())
                .unwrap_err()
                .message
                .contains("9 errors")
        );
        let long = json!({"errors": ["x".repeat(MAX_OUTPUT)]}).to_string();
        assert!(
            errors(long.as_bytes())
                .unwrap_err()
                .message
                .contains("4 KiB")
        );
    }
}
