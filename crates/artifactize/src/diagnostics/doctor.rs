#[cfg(test)]
mod tests;

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    auth,
    config::{Backend, read_workspace_config},
    llm, store, workspace,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub ok: bool,
    pub state_dir: PathBuf,
    pub checks: Vec<Check>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}
impl CheckStatus {
    fn ready(self) -> bool {
        match self {
            Self::Pass | Self::Warn => true,
            Self::Fail => false,
        }
    }
}
impl std::fmt::Display for CheckStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Pass => "PASS",
            Self::Warn => "WARN",
            Self::Fail => "FAIL",
        })
    }
}

#[derive(Debug, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub status: CheckStatus,
    pub message: String,
    pub details: Value,
}

impl DoctorReport {
    fn add(&mut self, name: &'static str, status: CheckStatus, message: &str, details: Value) {
        self.ok &= status.ready();
        self.checks.push(Check {
            name,
            status,
            message: message.into(),
            details,
        });
    }
}

/// Local readiness only. Missing optional backends are warnings, not failed reviews.
pub async fn doctor(state: Option<&Path>, repo: Option<&Path>) -> Result<DoctorReport, String> {
    let state = store::state_dir(state)?;
    let mut report = DoctorReport {
        ok: true,
        state_dir: state.clone(),
        checks: Vec::new(),
    };
    let config = repo.map(read_workspace_config).transpose();
    if repo.is_some() {
        match &config {
            Ok(Some(config)) => report.add(
                "config",
                CheckStatus::Pass,
                "Folder configuration is valid.",
                json!({"artifacts": config.artifacts.len(), "evals": config.evals.len()}),
            ),
            Err(error) => report.add("config", CheckStatus::Fail, &error.to_string(), Value::Null),
            _ => unreachable!(),
        }
    }
    let writable = (|| {
        if let Some(repo) = repo {
            let repo = workspace::canonical_target(repo)?;
            let repo = if repo.is_file() {
                repo.parent().unwrap()
            } else {
                &repo
            };
            workspace::outside_workspace(repo, &state)?;
        }
        probe_writable(&state)
    })();
    match writable {
        Ok(()) => report.add(
            "state",
            CheckStatus::Pass,
            "State directory is writable or can be created.",
            json!({"writable":true}),
        ),
        Err(error) => report.add(
            "state",
            CheckStatus::Fail,
            &error.to_string(),
            json!({"writable":false}),
        ),
    }
    // Read-only: a state of another schema is refused, never migrated.
    let current = store::STATE_SCHEMA_VERSION;
    match store::state_schema(&state) {
        Ok(None) => report.add(
            "schema",
            CheckStatus::Pass,
            "No state database yet.",
            json!({"schema":null}),
        ),
        Ok(Some(0)) => report.add(
            "schema",
            CheckStatus::Pass,
            "The state database is not initialized yet.",
            json!({"schema":0}),
        ),
        Ok(Some(found)) if found == current => report.add(
            "schema",
            CheckStatus::Pass,
            &format!("State database schema {current}."),
            json!({"schema":current}),
        ),
        Ok(Some(found)) => report.add(
            "schema",
            CheckStatus::Fail,
            &store::schema_error(found),
            json!({"schema":found,"supported":current}),
        ),
        Err(error) => report.add("schema", CheckStatus::Fail, &error, Value::Null),
    }
    match crate::limits::Limits::read(&state) {
        Ok(limits) if limits.backends().is_empty() => report.add(
            "limits",
            CheckStatus::Pass,
            "No backend capacity limits (limits.json).",
            json!({"backends":{}}),
        ),
        Ok(limits) => report.add(
            "limits",
            CheckStatus::Pass,
            &format!(
                "Backend capacity on this machine: {}.",
                limits
                    .backends()
                    .iter()
                    .map(|(backend, slots)| format!("{backend} {slots}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            json!({"backends":limits.backends()}),
        ),
        Err(error) => report.add("limits", CheckStatus::Fail, &error, Value::Null),
    }
    let (status, message, details) = sessions(&state);
    report.add("sessions", status, &message, details);
    for (name, backend) in [
        ("openai", Backend::Openai),
        ("anthropic", Backend::Anthropic),
    ] {
        let variable = llm::key_variable(backend).expect("an API-key backend");
        let present = env::var(variable).is_ok_and(|key| !key.trim().is_empty());
        let key = format!(
            "{variable} is {} (not validated with the provider)",
            if present { "present" } else { "absent" }
        );
        match llm::test_endpoint(backend) {
            Ok(None) => report.add(
                name,
                if present { CheckStatus::Pass } else { CheckStatus::Warn },
                &format!("{key}."),
                json!({"present":present}),
            ),
            // A test endpoint is never a production setup, so it always warns.
            Ok(Some(endpoint)) => report.add(
                name,
                CheckStatus::Warn,
                &format!(
                    "{key}; {} sends this backend's requests to the local test endpoint {endpoint}.",
                    llm::base_url_variable(backend)
                ),
                json!({"present":present,"testEndpoint":endpoint}),
            ),
            Err(error) => report.add(name, CheckStatus::Fail, &error, json!({"present":present})),
        }
    }
    let (status, message, details) = codex(&state, repo);
    report.add("codex", status, &message, details);
    // Offline: configuration and token storage only; `remote status` checks reachability.
    match auth::remote::remote(Some(&state), repo) {
        Ok(None) => report.add(
            "remote",
            CheckStatus::Pass,
            "No remote review store is configured.",
            json!({"configured":false}),
        ),
        Ok(Some(remote)) => {
            let details = json!({"configured":true,"url":remote.url,"share":remote.share,"tokenSource":remote.token_source});
            if remote.token_source == auth::remote::TokenSource::None {
                report.add(
                    "remote",
                    CheckStatus::Warn,
                    "No remote token; run `artifactize remote login URL` or set ARTIFACTIZE_REMOTE_TOKEN.",
                    details,
                );
            } else {
                report.add(
                    "remote",
                    CheckStatus::Pass,
                    "Remote review store is configured; `remote status` checks reachability.",
                    details,
                );
            }
        }
        Err(error) => report.add("remote", CheckStatus::Fail, &error, Value::Null),
    }
    Ok(report)
}

/// The saved Agent conversations: whether new ones are saved, the store's size against its
/// bounds, and the state id their references name.
fn sessions(state: &Path) -> (CheckStatus, String, Value) {
    let bounds = match crate::limits::Limits::read(state) {
        Ok(limits) => limits.agent_sessions(),
        Err(_) => {
            return (
                CheckStatus::Fail,
                "limits.json is invalid; see limits.".into(),
                Value::Null,
            );
        }
    };
    let usage = match crate::agent::session::usage(state) {
        Ok(usage) => usage,
        Err(error) => return (CheckStatus::Fail, error, Value::Null),
    };
    // A database the schema check refuses has no id to report here.
    let state_id = store::read_state_id(state).ok().flatten();
    let mib = |bytes: u64| format!("{:.1} MiB", bytes as f64 / 1_048_576.0);
    let message = format!(
        "{} Agent session{} take {} of {}; a collection brings them down to {}.{}",
        usage.sessions,
        if usage.sessions == 1 { "" } else { "s" },
        mib(usage.bytes),
        mib(bounds.max_bytes),
        mib(bounds.target_bytes),
        if bounds.enabled {
            ""
        } else {
            " Saving is off (limits.json agentSessions.enabled)."
        }
    );
    let details = json!({
        "enabled": bounds.enabled, "sessions": usage.sessions, "bytes": usage.bytes,
        "maxBytes": bounds.max_bytes, "targetBytes": bounds.target_bytes,
        "directory": crate::agent::session::directory(state), "stateId": state_id,
    });
    (CheckStatus::Pass, message, details)
}

/// The Codex sign-in, read offline: no lock, refresh or network, and an auth file is
/// only read.
fn codex(state: &Path, repo: Option<&Path>) -> (CheckStatus, String, Value) {
    let endpoints = llm::test_endpoint(Backend::Codex).and_then(|base| {
        Ok((
            base,
            llm::variable_endpoint(auth::codex::AUTH_URL_VARIABLE)?,
        ))
    });
    let ((base, sign_in), status) = match (endpoints, auth::codex::status(Some(state), repo)) {
        (Err(error), _) | (_, Err(error)) => return (CheckStatus::Fail, error, Value::Null),
        (Ok(endpoints), Ok(status)) => (endpoints, status),
    };
    use auth::codex::{FileExpiry, Status, StoredExpiry};
    let (mut level, mut message) = match &status {
        Status::Refused { reason } => (CheckStatus::Warn, reason.clone()),
        Status::Absent => (
            CheckStatus::Warn,
            "No Codex sign-in; run `artifactize login codex` or set ARTIFACTIZE_CODEX_AUTH_FILE."
                .to_owned(),
        ),
        Status::File {
            path,
            expiry: FileExpiry::Usable { .. },
        } => (
            CheckStatus::Pass,
            format!(
                "ARTIFACTIZE_CODEX_AUTH_FILE is read, never refreshed: {}.",
                path.display()
            ),
        ),
        Status::File {
            path,
            expiry: FileExpiry::Expired,
        } => (
            CheckStatus::Warn,
            format!(
                "The Codex access token in {} has expired; sign in with Codex again.",
                path.display()
            ),
        ),
        Status::Stored {
            expiry: StoredExpiry::Usable { .. },
        } => (
            CheckStatus::Pass,
            "Codex sign-in is present; no provider validation or refresh was attempted.".to_owned(),
        ),
        Status::Stored {
            expiry: StoredExpiry::Expired { .. },
        } => (
            CheckStatus::Pass,
            "Codex sign-in is present; its access token has expired and is refreshed on next use."
                .to_owned(),
        ),
    };
    let mut details = json!(status);
    for (variable, endpoint, key) in [
        (llm::base_url_variable(Backend::Codex), base, "testEndpoint"),
        (auth::codex::AUTH_URL_VARIABLE, sign_in, "testAuthEndpoint"),
    ] {
        if let Some(endpoint) = endpoint {
            // A test endpoint is never a production setup, so it always warns.
            level = CheckStatus::Warn;
            message.push_str(&format!(
                " {variable} points at the local test endpoint {endpoint}."
            ));
            details[key] = json!(endpoint);
        }
    }
    (level, message, details)
}

fn probe_writable(state: &Path) -> std::io::Result<()> {
    let mut parent = state;
    while !parent.try_exists()? {
        parent = parent
            .parent()
            .ok_or_else(|| std::io::Error::other("No existing state directory ancestor."))?;
    }
    if !fs::metadata(parent)?.is_dir() {
        return Err(std::io::Error::other(
            "State directory or its ancestor is not a directory.",
        ));
    }
    let probe = tempfile::Builder::new()
        .prefix(".artifactize-doctor-")
        .tempdir_in(parent)?;
    probe.close()
}
