//! How Claude Code is started: its arguments and environment. Claude Code runs with the
//! user's own login and nothing else of theirs: no settings, hooks, CLAUDE.md, memory,
//! skills or tools of its own; the only tools are the ones artifactize offers.

use std::{collections::BTreeMap, ffi::OsString};

use serde_json::{Value, json};

use crate::platform::environment;

/// Claude Code's executable, found on `PATH`.
pub const PROGRAM: &str = "claude";
/// The MCP server artifactize hosts in this process; Claude Code names its tools
/// `mcp__artifactize__<tool>`.
pub const SERVER: &str = "artifactize";

/// What one Claude Code session is started with. A probe sets no model and offers no tools.
#[derive(Debug, Default)]
pub struct Launch<'a> {
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    /// Appended to Claude Code's own system prompt.
    pub system: Option<&'a str>,
    /// The tools artifactize offers, by their own names.
    pub tools: Vec<&'a str>,
    /// The JSON Schema the final answer must match.
    pub schema: Option<&'a Value>,
}

impl Launch<'_> {
    pub fn args(&self) -> Vec<OsString> {
        let mut args: Vec<String> = [
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            // Claude Code's built-in tools are off: only the offered MCP tools are callable.
            "--tools",
            "",
            "--strict-mcp-config",
            "--permission-mode",
            "dontAsk",
            "--setting-sources",
            "",
            "--settings",
            r#"{"disableAllHooks":true}"#,
            "--no-session-persistence",
            "--disable-slash-commands",
            "--no-chrome",
        ]
        .map(str::to_owned)
        .into();
        if self.model.is_some() {
            args.extend([
                "--mcp-config".into(),
                json!({"mcpServers": {SERVER: {"type": "sdk", "name": SERVER}}}).to_string(),
            ]);
        }
        if !self.tools.is_empty() {
            let tools: Vec<String> = self.tools.iter().map(|name| tool_name(name)).collect();
            args.extend(["--allowedTools".into(), tools.join(",")]);
        }
        if let Some(model) = self.model {
            args.extend([
                "--model".into(),
                model.into(),
                "--thinking".into(),
                "adaptive".into(),
                "--thinking-display".into(),
                "summarized".into(),
            ]);
        }
        if let Some(effort) = self.effort {
            args.extend(["--effort".into(), effort.into()]);
        }
        if let Some(system) = self.system {
            args.extend(["--append-system-prompt".into(), system.into()]);
        }
        if let Some(schema) = self.schema {
            args.extend(["--json-schema".into(), schema.to_string()]);
        }
        args.into_iter().map(OsString::from).collect()
    }
}

/// The name Claude Code gives an offered tool.
pub fn tool_name(name: &str) -> String {
    format!("mcp__{SERVER}__{name}")
}

/// Removed from the inherited environment: a minimal mode that skips the subscription login,
/// an effort that would override the declared one, a cap on every tool call, and the marks
/// of a Claude Code session artifactize itself runs in, which this session is not part of.
const CLEARED: [&str; 5] = [
    "CLAUDE_CODE_SIMPLE",
    "CLAUDE_CODE_EFFORT_LEVEL",
    "MCP_TOOL_TIMEOUT",
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
];

/// Set for every session: no CLAUDE.md, auto memory, compaction, away summaries, advisor or
/// tool search (which would hide the MCP tools); tool results up to 200,000 tokens, as large
/// as artifactize's own; and none of Claude Code's silent fallbacks, a non-streaming retry
/// that can leave a tool call unfinished and a switch to another model after a refusal.
const SET: [(&str, &str); 9] = [
    ("CLAUDE_CODE_DISABLE_CLAUDE_MDS", "1"),
    ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1"),
    ("DISABLE_AUTO_COMPACT", "1"),
    ("CLAUDE_CODE_ENABLE_AWAY_SUMMARY", "0"),
    ("ENABLE_TOOL_SEARCH", "false"),
    ("CLAUDE_CODE_DISABLE_ADVISOR_TOOL", "1"),
    ("MAX_MCP_OUTPUT_TOKENS", "200000"),
    ("CLAUDE_CODE_DISABLE_NONSTREAMING_FALLBACK", "1"),
    ("CLAUDE_CODE_DISABLE_REFUSAL_FALLBACK", "1"),
];

/// Claude Code's environment: the user's own, as for any command, so every login Claude
/// Code has (a subscription, an API key, a cloud account) works, with [`CLEARED`] removed
/// and [`SET`] set.
pub fn environment(mut inherited: BTreeMap<OsString, OsString>) -> BTreeMap<OsString, OsString> {
    for name in CLEARED {
        environment::remove(&mut inherited, name);
    }
    for (name, value) in SET {
        environment::set(&mut inherited, name, value);
    }
    inherited
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(args: &[OsString]) -> Vec<&str> {
        args.iter().map(|arg| arg.to_str().unwrap()).collect()
    }

    fn value_of<'a>(args: &'a [&'a str], flag: &str) -> Option<&'a str> {
        let at = args.iter().position(|arg| *arg == flag)?;
        args.get(at + 1).copied()
    }

    #[test]
    fn a_review_offers_only_its_tools_with_the_declared_model_and_effort() {
        let schema = json!({"type": "object"});
        let args = Launch {
            model: Some("claude-opus-5-5"),
            effort: Some("high"),
            system: Some("Review."),
            tools: vec!["read", "grep"],
            schema: Some(&schema),
        }
        .args();
        let args = text(&args);
        assert_eq!(value_of(&args, "--tools"), Some(""));
        assert_eq!(value_of(&args, "--setting-sources"), Some(""));
        assert_eq!(value_of(&args, "--permission-mode"), Some("dontAsk"));
        assert!(args.contains(&"--strict-mcp-config"));
        assert_eq!(
            value_of(&args, "--allowedTools"),
            Some("mcp__artifactize__read,mcp__artifactize__grep")
        );
        let servers: Value =
            serde_json::from_str(value_of(&args, "--mcp-config").unwrap()).unwrap();
        assert_eq!(
            servers,
            json!({"mcpServers": {"artifactize": {"type": "sdk", "name": "artifactize"}}})
        );
        assert_eq!(value_of(&args, "--model"), Some("claude-opus-5-5"));
        assert_eq!(value_of(&args, "--effort"), Some("high"));
        assert_eq!(value_of(&args, "--thinking"), Some("adaptive"));
        assert_eq!(value_of(&args, "--thinking-display"), Some("summarized"));
        assert_eq!(value_of(&args, "--append-system-prompt"), Some("Review."));
        assert_eq!(
            value_of(&args, "--json-schema"),
            Some(r#"{"type":"object"}"#)
        );
    }

    #[test]
    fn a_probe_names_no_model_server_or_tools() {
        let args = Launch::default().args();
        let args = text(&args);
        for flag in ["--model", "--mcp-config", "--allowedTools", "--json-schema"] {
            assert!(!args.contains(&flag), "{flag}");
        }
        assert_eq!(value_of(&args, "--tools"), Some(""));
    }

    #[test]
    fn the_environment_keeps_the_login_and_turns_off_the_users_customizations() {
        let inherited = [
            ("USER_SETTING", "kept"),
            ("CLAUDE_CODE_OAUTH_TOKEN", "token"),
            ("CLAUDE_CODE_SIMPLE", "1"),
            ("CLAUDE_CODE_EFFORT_LEVEL", "low"),
            ("CLAUDECODE", "1"),
            ("ENABLE_TOOL_SEARCH", "true"),
        ]
        .into_iter()
        .map(|(name, value)| (name.into(), value.into()))
        .collect();
        let environment = environment(inherited);
        let get = |name: &str| environment.get(&OsString::from(name)).cloned();
        assert_eq!(get("USER_SETTING"), Some("kept".into()));
        assert_eq!(get("CLAUDE_CODE_OAUTH_TOKEN"), Some("token".into()));
        for cleared in [
            "CLAUDE_CODE_SIMPLE",
            "CLAUDE_CODE_EFFORT_LEVEL",
            "CLAUDECODE",
        ] {
            assert_eq!(get(cleared), None, "{cleared}");
        }
        assert_eq!(get("ENABLE_TOOL_SEARCH"), Some("false".into()));
        assert_eq!(get("CLAUDE_CODE_DISABLE_CLAUDE_MDS"), Some("1".into()));
        assert_eq!(
            get("CLAUDE_CODE_DISABLE_REFUSAL_FALLBACK"),
            Some("1".into())
        );
    }
}
