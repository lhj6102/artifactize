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

#[derive(Debug, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub status: &'static str,
    pub message: String,
    pub details: Value,
}

impl DoctorReport {
    fn add(&mut self, name: &'static str, status: &'static str, message: &str, details: Value) {
        self.ok &= status != "FAIL";
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
                "PASS",
                "Folder configuration is valid.",
                json!({"artifacts": config.artifacts.len(), "evals": config.evals.len()}),
            ),
            Err(error) => report.add("config", "FAIL", &error.to_string(), Value::Null),
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
            "PASS",
            "State directory is writable or can be created.",
            json!({"writable":true}),
        ),
        Err(error) => report.add(
            "state",
            "FAIL",
            &error.to_string(),
            json!({"writable":false}),
        ),
    }
    // Read-only: an older database is upgraded by the next command that opens it, never here.
    let current = store::STATE_SCHEMA_VERSION;
    match store::state_schema(&state) {
        Ok(None) => report.add(
            "schema",
            "PASS",
            "No state database yet.",
            json!({"schema":null}),
        ),
        Ok(Some(0)) => report.add(
            "schema",
            "PASS",
            "The state database is not initialized yet.",
            json!({"schema":0}),
        ),
        Ok(Some(found)) if found > current => report.add(
            "schema",
            "FAIL",
            &format!("Unsupported state schema version: {found}; a newer artifactize wrote it."),
            json!({"schema":found}),
        ),
        Ok(Some(found)) if found < current => report.add(
            "schema",
            "PASS",
            &format!(
                "State database schema {found}; the next artifactize command upgrades it to {current}."
            ),
            json!({"schema":found,"upgradeTo":current}),
        ),
        Ok(Some(_)) => report.add(
            "schema",
            "PASS",
            &format!("State database schema {current}."),
            json!({"schema":current}),
        ),
        Err(error) => report.add("schema", "FAIL", &error, Value::Null),
    }
    match crate::limits::Limits::read(&state) {
        Ok(limits) if limits.backends().is_empty() => report.add(
            "limits",
            "PASS",
            "No backend capacity limits (limits.json).",
            json!({"backends":{}}),
        ),
        Ok(limits) => report.add(
            "limits",
            "PASS",
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
        Err(error) => report.add("limits", "FAIL", &error, Value::Null),
    }
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
                if present { "PASS" } else { "WARN" },
                &format!("{key}."),
                json!({"present":present}),
            ),
            // A test endpoint is never a production setup, so it always warns.
            Ok(Some(endpoint)) => report.add(
                name,
                "WARN",
                &format!(
                    "{key}; {} sends this backend's requests to the local test endpoint {endpoint}.",
                    llm::base_url_variable(backend)
                ),
                json!({"present":present,"testEndpoint":endpoint}),
            ),
            Err(error) => report.add(name, "FAIL", &error, json!({"present":present})),
        }
    }
    let (status, message, details) = codex(&state, repo);
    report.add("codex", status, &message, details);
    // Offline: configuration and token storage only; `remote status` checks reachability.
    match auth::remote::remote(Some(&state), repo) {
        Ok(None) => report.add(
            "remote",
            "PASS",
            "No remote review store is configured.",
            json!({"configured":false}),
        ),
        Ok(Some(remote)) => {
            let details = json!({"configured":true,"url":remote.url,"share":remote.share,"tokenSource":remote.token_source});
            if remote.token_source == auth::remote::TokenSource::None {
                report.add(
                    "remote",
                    "WARN",
                    "No remote token; run `artifactize remote login URL` or set ARTIFACTIZE_REMOTE_TOKEN.",
                    details,
                );
            } else {
                report.add(
                    "remote",
                    "PASS",
                    "Remote review store is configured; `remote status` checks reachability.",
                    details,
                );
            }
        }
        Err(error) => report.add("remote", "FAIL", &error, Value::Null),
    }
    Ok(report)
}

/// The Codex sign-in, read offline: no lock, refresh or network, and an auth file is
/// only read.
fn codex(state: &Path, repo: Option<&Path>) -> (&'static str, String, Value) {
    let endpoints = llm::test_endpoint(Backend::Codex).and_then(|base| {
        Ok((
            base,
            llm::variable_endpoint(auth::codex::AUTH_URL_VARIABLE)?,
        ))
    });
    let ((base, sign_in), status) = match (endpoints, auth::codex::status(Some(state), repo)) {
        (Err(error), _) | (_, Err(error)) => return ("FAIL", error, Value::Null),
        (Ok(endpoints), Ok(status)) => (endpoints, status),
    };
    let (mut level, mut message) = match (status.source, status.expired) {
        ("none", _) if status.refused.is_some() => ("WARN", status.refused.clone().unwrap()),
        ("none", _) => (
            "WARN",
            "No Codex sign-in; run `artifactize login codex` or set ARTIFACTIZE_CODEX_AUTH_FILE."
                .to_owned(),
        ),
        ("file", false) => (
            "PASS",
            format!(
                "ARTIFACTIZE_CODEX_AUTH_FILE is read, never refreshed: {}.",
                status.auth_file.as_ref().unwrap().display()
            ),
        ),
        ("file", true) => (
            "WARN",
            format!(
                "The Codex access token in {} has expired; sign in with Codex again.",
                status.auth_file.as_ref().unwrap().display()
            ),
        ),
        (_, false) => (
            "PASS",
            "Codex sign-in is present; no provider validation or refresh was attempted.".to_owned(),
        ),
        (_, true) => (
            "PASS",
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
            level = "WARN";
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
