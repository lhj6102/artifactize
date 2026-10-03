use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::Serialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{auth, config::read_workspace_config, process, store, workspace};

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
    for (backend, variable) in [
        ("openai", "OPENAI_API_KEY"),
        ("anthropic", "ANTHROPIC_API_KEY"),
    ] {
        let present = env::var(variable).is_ok_and(|key| !key.trim().is_empty());
        report.add(
            backend,
            if present { "PASS" } else { "WARN" },
            &format!(
                "{variable} is {} (not validated with the provider).",
                if present { "present" } else { "absent" }
            ),
            json!({"present":present}),
        );
    }
    match auth::chatgpt_status(Some(&state), repo) {
        Ok(status) => report.add(
            "chatgpt",
            if status.present && !status.expired {
                "PASS"
            } else {
                "WARN"
            },
            if !status.present {
                "No ChatGPT login; run `artifactize login chatgpt`."
            } else if status.expired {
                "ChatGPT access token has expired; refresh was not attempted."
            } else {
                "ChatGPT login is present; no provider validation or refresh was attempted."
            },
            json!(status),
        ),
        Err(error) => report.add("chatgpt", "FAIL", &error, Value::Null),
    }
    let binary = env::var_os("PATH").and_then(|path| {
        env::split_paths(&path)
            .map(|path| path.join("claude"))
            .find(|path| {
                path.metadata()
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            })
    });
    if let Some(binary) = binary {
        let command = process::Command {
            program: binary.clone().into(),
            args: vec!["--version".into()],
            cwd: env::current_dir().map_err(|e| e.to_string())?,
            env: ["PATH", "HOME", "LANG", "TMPDIR"]
                .into_iter()
                .filter_map(|name| env::var_os(name).map(|value| (name.into(), value)))
                .collect(),
            timeout: Duration::from_secs(5),
        };
        match process::run(command, CancellationToken::new(), |_| async { Ok(()) }).await {
            Ok(output) if output.status.success() && !output.truncated => {
                let version = String::from_utf8(crate::runtime::clean_output(&output.stdout))
                    .expect("clean output is UTF-8");
                report.add(
                    "claude",
                    "PASS",
                    version.trim(),
                    json!({"present":true,"path":binary,"version":version.trim()}),
                );
            }
            Ok(_) => report.add(
                "claude",
                "FAIL",
                "claude --version failed or exceeded the output limit.",
                json!({"present":true,"path":binary}),
            ),
            Err(error) => report.add(
                "claude",
                "FAIL",
                &format!("claude --version: {error}"),
                json!({"present":true,"path":binary}),
            ),
        }
    } else {
        report.add(
            "claude",
            "WARN",
            "The claude executable was not found on PATH.",
            json!({"present":false}),
        );
    }
    Ok(report)
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
