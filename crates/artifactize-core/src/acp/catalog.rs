//! 3-tier ACP Chat agent catalog: built-in presets, official ACP Registry pinned installs,
//! and machine-local custom ACP agents (`$STATE/chat/agents.json` + `$STATE/chat/secrets.json`).
//!
//! Never loads ACP commands, arguments or environment from repository-committed files.
//! All status, version and model probes run exclusively in empty private tempdirs.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::{
    auth::storage::{Storage, Tokens},
    llm::models::ListedModel,
    platform, process,
};

use super::{
    AgentId, ConfigOption, Problem,
    trust::{self, LauncherFingerprint},
    wire::{Connection, INITIALIZE_TIMEOUT},
};

const AGENTS_FILE: &str = "agents.json";
const AGENTS_LOCK: &str = "agents";
const SECRETS_FILE: &str = "secrets.json";
const SECRETS_LOCK: &str = "secrets";
const AGENTS_SCHEMA_VERSION: u32 = 1;
/// Official ACP Registry discovery URL (latest is discovery-only; installs pin exact versions).
pub const DEFAULT_REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/agentclientprotocol/registry/main/registry.json";
/// Environment override for offline Registry discovery tests.
const REGISTRY_URL_ENV: &str = "ARTIFACTIZE_ACP_REGISTRY_URL";
/// Probe results are cached per `(agent_id, resolved_executable, args_hash, mtime_or_version, auth_generation)`.
pub const PROBE_CACHE_TTL: Duration = Duration::from_secs(2 * 60);
/// Interactive probe timeout (matches model listing timeout).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
/// Network fetch timeout for Registry catalog discovery and pinned artifact downloads.
const REGISTRY_FETCH_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximum size of a single custom secret env value.
const MAX_SECRET_VALUE_BYTES: usize = 16 * 1024;
/// Maximum Registry artifact download size (256 MiB) to bound local disk usage.
const MAX_REGISTRY_ARTIFACT_BYTES: usize = 256 * 1024 * 1024;
/// Antigravity minimum CLI version for ACP model support.
pub const ANTIGRAVITY_MINIMUM: semver::Version = semver::Version::new(1, 5, 1);

/// Alternate Google auth env vars removed for Antigravity so its own Google login is used.
pub(crate) const ANTIGRAVITY_ALTERNATE_AUTH: &[&str] = &[
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "GOOGLE_GENAI_USE_VERTEXAI",
];

static AUTH_GENERATION: AtomicU64 = AtomicU64::new(1);

/// Bump after any native CLI login/logout so probe caches and runtime fingerprints refresh.
pub fn bump_auth_generation() -> u64 {
    AUTH_GENERATION.fetch_add(1, Ordering::SeqCst) + 1
}

pub fn auth_generation() -> u64 {
    AUTH_GENERATION.load(Ordering::SeqCst)
}

/// Isolation and transport metadata declared by each built-in ACP preset.
#[derive(Debug, Clone)]
pub struct Preset {
    pub id: &'static str,
    pub display_name: &'static str,
    pub program: &'static str,
    pub args: &'static [&'static str],
    pub version_args: &'static [&'static str],
    pub minimum_version: Option<semver::Version>,
    pub env_denylist: &'static [&'static str],
    pub extra_env: &'static [(&'static str, &'static str)],
    pub mcp_stdio: bool,
    pub supports_same_child_load: bool,
    pub can_disable_project_config: bool,
    pub native_sandbox_note: &'static str,
    pub hide_always_options: bool,
    pub install_hint: &'static str,
    pub login_action: &'static str,
}

/// Built-in 3-OS ACP Chat presets.
pub const PRESETS: &[Preset] = &[
    Preset {
        id: "antigravity",
        display_name: "Antigravity",
        program: "agy",
        args: &["acp"],
        version_args: &["--version"],
        minimum_version: Some(ANTIGRAVITY_MINIMUM),
        env_denylist: ANTIGRAVITY_ALTERNATE_AUTH,
        extra_env: &[("AGY_CLI_DISABLE_AUTO_UPDATE", "1")],
        mcp_stdio: true,
        supports_same_child_load: true,
        can_disable_project_config: true,
        native_sandbox_note: "Runs natively in the worktree via `agy acp`; ACP client fs/terminal callbacks are disabled (not an OS sandbox).",
        hide_always_options: false,
        install_hint: "Install the Antigravity CLI (`agy`), then sign in via `artifactize login antigravity` or Settings → Chat Agents.",
        login_action: "artifactize login antigravity",
    },
    Preset {
        id: "claude-acp",
        display_name: "Claude ACP",
        program: "claude-agent-acp",
        args: &[],
        version_args: &["--version"],
        minimum_version: None,
        env_denylist: &[],
        extra_env: &[],
        mcp_stdio: true,
        supports_same_child_load: true,
        can_disable_project_config: false,
        native_sandbox_note: "Runs Claude Code's native harness via ACP in the worktree; may read repository `.claude/` settings and hooks on startup. ACP client fs/terminal callbacks are disabled (not an OS sandbox).",
        hide_always_options: false,
        install_hint: "Install `claude-agent-acp` (or install from Settings → Chat Agents → Browse Registry) and sign in with `claude auth login`.",
        login_action: "claude auth login",
    },
    Preset {
        id: "codex-acp",
        display_name: "Codex ACP",
        program: "codex-acp",
        args: &[],
        version_args: &["--version"],
        minimum_version: None,
        env_denylist: &[],
        extra_env: &[],
        mcp_stdio: true,
        supports_same_child_load: true,
        can_disable_project_config: false,
        native_sandbox_note: "Runs Codex's native harness via ACP in the worktree; native sandbox/approval modes are exposed via `configOptions`. ACP client fs/terminal callbacks are disabled (not an OS sandbox).",
        hide_always_options: false,
        install_hint: "Install `codex-acp` (or install from Settings → Chat Agents → Browse Registry) and sign in with Codex.",
        login_action: "codex login",
    },
    Preset {
        id: "gemini-cli",
        display_name: "Gemini CLI",
        program: "gemini",
        args: &["--experimental-acp"],
        version_args: &["--version"],
        minimum_version: None,
        env_denylist: &[],
        extra_env: &[],
        mcp_stdio: true,
        supports_same_child_load: true,
        can_disable_project_config: false,
        native_sandbox_note: "Runs Gemini CLI natively via ACP in the worktree; may read `.gemini/settings.json` on startup. ACP client fs/terminal callbacks are disabled (not an OS sandbox).",
        hide_always_options: false,
        install_hint: "Install Gemini CLI (`gemini`) and complete its login in a terminal.",
        login_action: "gemini",
    },
    Preset {
        id: "opencode",
        display_name: "OpenCode",
        program: "opencode",
        args: &["acp"],
        version_args: &["--version"],
        minimum_version: None,
        env_denylist: &[],
        extra_env: &[],
        mcp_stdio: true,
        supports_same_child_load: true,
        can_disable_project_config: false,
        native_sandbox_note: "Runs OpenCode natively via ACP in the worktree; may read `opencode.json` on startup. ACP client fs/terminal callbacks are disabled (not an OS sandbox).",
        hide_always_options: false,
        install_hint: "Install OpenCode (`opencode`) and configure its credentials.",
        login_action: "opencode auth login",
    },
    Preset {
        id: "copilot",
        display_name: "GitHub Copilot",
        program: "copilot",
        args: &["--acp"],
        version_args: &["--version"],
        minimum_version: None,
        env_denylist: &[],
        extra_env: &[],
        mcp_stdio: true,
        supports_same_child_load: true,
        can_disable_project_config: false,
        native_sandbox_note: "Runs GitHub Copilot CLI natively via ACP in the worktree. ACP client fs/terminal callbacks are disabled (not an OS sandbox).",
        hide_always_options: true,
        install_hint: "Install GitHub Copilot CLI (`copilot`) and sign in with GitHub.",
        login_action: "copilot auth login",
    },
    Preset {
        id: "cursor",
        display_name: "Cursor Agent",
        program: "cursor-agent",
        args: &["acp"],
        version_args: &["--version"],
        minimum_version: None,
        env_denylist: &[],
        extra_env: &[],
        mcp_stdio: true,
        supports_same_child_load: true,
        can_disable_project_config: false,
        native_sandbox_note: "Runs Cursor Agent natively via ACP in the worktree. ACP client fs/terminal callbacks are disabled (not an OS sandbox).",
        hide_always_options: true,
        install_hint: "Install Cursor Agent (`cursor-agent`) and sign in.",
        login_action: "cursor-agent login",
    },
    Preset {
        id: "goose",
        display_name: "Goose",
        program: "goose",
        args: &["acp"],
        version_args: &["--version"],
        minimum_version: None,
        env_denylist: &[],
        extra_env: &[],
        mcp_stdio: true,
        supports_same_child_load: true,
        can_disable_project_config: false,
        native_sandbox_note: "Runs Goose natively via ACP in the worktree. ACP client fs/terminal callbacks are disabled (not an OS sandbox).",
        hide_always_options: true,
        install_hint: "Install Goose (`goose`) and run `goose configure`.",
        login_action: "goose configure",
    },
];

pub fn preset_by_id(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|preset| preset.id == id)
}

pub fn is_builtin_preset(id: &str) -> bool {
    preset_by_id(id).is_some()
}

/// Whether `name` matches a built-in ACP preset or a saved machine-local ACP agent.
/// Used by eval routing to give a targeted diagnostic when an unknown backend is a Chat agent.
pub fn is_chat_agent(name: &str, state: Option<&Path>, repo: Option<&Path>) -> bool {
    if is_builtin_preset(name) {
        return true;
    }
    list_custom(state, repo)
        .map(|agents| agents.iter().any(|agent| agent.id.as_str() == name))
        .unwrap_or(false)
}

/// Distribution kind for an official ACP Registry entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DistributionKind {
    Binary,
    Archive,
    Npx,
    Uvx,
}

/// Complete resolved specification bound to user consent before installing from the ACP Registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryInstallSpec {
    pub registry_id: AgentId,
    pub display_name: String,
    pub exact_version: String,
    pub platform: String,
    pub distribution_kind: DistributionKind,
    pub package_or_url: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env_pairs: BTreeMap<String, String>,
    #[serde(default)]
    pub build_scripts_allowed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_sha256: Option<String>,
    /// Explicitly true only when the user confirmed the computed SHA-256 of an unsigned entry.
    #[serde(default)]
    pub allow_unsigned_digest: bool,
}

/// One discovered entry from the official ACP Registry (discovery-only; never auto-executed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryEntry {
    pub id: AgentId,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub spec: RegistryInstallSpec,
}

/// Tier of an ACP Chat agent in the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentTier {
    Preset,
    Registry,
    Custom,
}

/// A machine-local custom or Registry-installed ACP agent saved in `$STATE/chat/agents.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomAgent {
    pub id: AgentId,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Secret env variable names whose values live in `$STATE/chat/secrets.json` (never echoed here).
    #[serde(default)]
    pub secret_env_keys: Vec<String>,
    #[serde(default = "default_true")]
    pub mcp_stdio: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<RegistryInstallSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entrypoint_sha256: Option<String>,
}

fn default_true() -> bool {
    true
}

/// A secret env value redacted in `Debug` and stored separately in `$STATE/chat/secrets.json`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretEnvValue(String);

impl SecretEnvValue {
    pub fn parse(raw: &str) -> Result<Self, String> {
        if raw.is_empty() || raw.len() > MAX_SECRET_VALUE_BYTES {
            return Err(format!(
                "Secret environment value must be 1–{MAX_SECRET_VALUE_BYTES} bytes."
            ));
        }
        if raw.bytes().any(|byte| byte == 0) {
            return Err("Secret environment value cannot contain NUL bytes.".into());
        }
        Ok(Self(raw.to_owned()))
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretEnvValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretEnvValue(…)")
    }
}

/// Unified descriptor of any ACP agent (preset, Registry-installed, or custom) resolved for launch.
#[derive(Debug, Clone)]
pub struct AgentSpec {
    pub id: AgentId,
    pub name: String,
    pub tier: AgentTier,
    pub program: String,
    pub args: Vec<String>,
    pub version_args: Vec<String>,
    pub minimum_version: Option<semver::Version>,
    pub env_denylist: Vec<String>,
    pub extra_env: BTreeMap<String, String>,
    pub secret_env: BTreeMap<String, SecretEnvValue>,
    pub mcp_stdio: bool,
    pub supports_same_child_load: bool,
    pub reject_worktree_executable: bool,
    pub hide_always_options: bool,
    pub native_sandbox_note: String,
    pub can_disable_project_config: bool,
    pub install_hint: String,
    pub login_action: String,
}

impl AgentSpec {
    pub fn from_preset(preset: &Preset) -> Self {
        Self {
            id: preset.id.parse().expect("built-in preset id is valid"),
            name: preset.display_name.to_owned(),
            tier: AgentTier::Preset,
            program: preset.program.to_owned(),
            args: preset.args.iter().map(|arg| (*arg).to_owned()).collect(),
            version_args: preset
                .version_args
                .iter()
                .map(|arg| (*arg).to_owned())
                .collect(),
            minimum_version: preset.minimum_version.clone(),
            env_denylist: preset
                .env_denylist
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
            extra_env: preset
                .extra_env
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            secret_env: BTreeMap::new(),
            mcp_stdio: preset.mcp_stdio,
            supports_same_child_load: preset.supports_same_child_load,
            reject_worktree_executable: true,
            hide_always_options: preset.hide_always_options,
            native_sandbox_note: preset.native_sandbox_note.to_owned(),
            can_disable_project_config: preset.can_disable_project_config,
            install_hint: preset.install_hint.to_owned(),
            login_action: preset.login_action.to_owned(),
        }
    }

    /// Build the child environment following the #145 env contract uniformly across probe,
    /// login, Chat, and resume. Relative `PATH` entries and denylisted auth keys are stripped
    /// after applying custom/secret overrides so custom env cannot re-introduce them
    /// (`C-TRUST-BEFORE-WORKTREE`).
    pub fn build_env(&self, for_worktree: bool) -> BTreeMap<OsString, OsString> {
        let mut env = platform::environment::for_command();
        for (key, value) in &self.extra_env {
            platform::environment::set(&mut env, key, value);
        }
        for (key, secret) in &self.secret_env {
            platform::environment::set(&mut env, key, secret.expose());
        }
        if for_worktree {
            platform::environment::strip_relative_path_entries(&mut env);
        }
        for name in &self.env_denylist {
            platform::environment::remove(&mut env, name);
        }
        env
    }

    /// All secret values injected into this agent's environment (for `StreamRedactor`).
    /// A digest of the variables this agent is launched with, its secrets included, for the
    /// runtime fingerprint; the values themselves are never kept.
    pub fn environment_digest(&self) -> String {
        let mut text = Vec::new();
        for (key, value) in &self.extra_env {
            text.extend_from_slice(key.as_bytes());
            text.push(0);
            text.extend_from_slice(value.as_bytes());
            text.push(0);
        }
        text.push(1);
        for (key, secret) in &self.secret_env {
            text.extend_from_slice(key.as_bytes());
            text.push(0);
            text.extend_from_slice(secret.expose().as_bytes());
            text.push(0);
        }
        hex_sha256(&text)
    }

    pub fn known_secrets(&self) -> Vec<String> {
        self.secret_env
            .values()
            .map(|secret| secret.expose().to_owned())
            .collect()
    }
}

/// Summary of an ACP agent for Settings → Chat Agents and CLI `chat-agent list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: AgentId,
    pub name: String,
    pub tier: AgentTier,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub secret_env_keys: Vec<String>,
    pub mcp_stdio: bool,
    pub can_disable_project_config: bool,
    pub native_sandbox_note: String,
    pub login_action: String,
    pub install_hint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<RegistryInstallSpec>,
}

/// Result of probing an ACP agent in an empty private tempdir.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentAvailability {
    Ready {
        #[serde(with = "crate::platform::path_serde")]
        executable: PathBuf,
        version: Option<String>,
        models: Vec<ListedModel>,
        config_options: Vec<ConfigOption>,
    },
    NotSetUp {
        #[serde(with = "crate::platform::path_serde::option")]
        executable: Option<PathBuf>,
        version: Option<String>,
        reason: String,
        login_action: String,
    },
    Missing {
        reason: String,
    },
    TooOld {
        #[serde(with = "crate::platform::path_serde")]
        executable: PathBuf,
        version: String,
        required_version: String,
        reason: String,
    },
    Failed {
        #[serde(with = "crate::platform::path_serde::option")]
        executable: Option<PathBuf>,
        version: Option<String>,
        error: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedAgents {
    version: u32,
    #[serde(default)]
    agents: Vec<CustomAgent>,
}

impl Default for SavedAgents {
    fn default() -> Self {
        Self {
            version: AGENTS_SCHEMA_VERSION,
            agents: Vec::new(),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedSecrets {
    #[serde(default)]
    by_agent: BTreeMap<String, BTreeMap<String, SecretEnvValue>>,
}

/// List all saved custom and Registry-installed ACP agents in `$STATE/chat/agents.json`.
pub fn list_custom(state: Option<&Path>, repo: Option<&Path>) -> Result<Vec<CustomAgent>, String> {
    let storage = Storage::inspect(state, repo, Tokens::ChatAgents)?;
    Ok(storage
        .read::<SavedAgents>(AGENTS_FILE)?
        .unwrap_or_default()
        .agents)
}

/// List the full 3-tier catalog (built-in presets + Registry-installed + custom agents).
pub fn list_catalog(
    state: Option<&Path>,
    repo: Option<&Path>,
) -> Result<Vec<CatalogEntry>, String> {
    let mut entries: Vec<CatalogEntry> = PRESETS
        .iter()
        .map(|preset| CatalogEntry {
            id: preset.id.parse().expect("preset id is valid"),
            name: preset.display_name.to_owned(),
            tier: AgentTier::Preset,
            command: preset.program.to_owned(),
            args: preset.args.iter().map(|arg| (*arg).to_owned()).collect(),
            env: BTreeMap::new(),
            secret_env_keys: Vec::new(),
            mcp_stdio: preset.mcp_stdio,
            can_disable_project_config: preset.can_disable_project_config,
            native_sandbox_note: preset.native_sandbox_note.to_owned(),
            login_action: preset.login_action.to_owned(),
            install_hint: preset.install_hint.to_owned(),
            registry: None,
        })
        .collect();
    for custom in list_custom(state, repo)? {
        let tier = if custom.registry.is_some() {
            AgentTier::Registry
        } else {
            AgentTier::Custom
        };
        entries.push(CatalogEntry {
            id: custom.id,
            name: custom.name,
            tier,
            command: custom.command,
            args: custom.args,
            env: custom.env,
            secret_env_keys: custom.secret_env_keys,
            mcp_stdio: custom.mcp_stdio,
            can_disable_project_config: false,
            native_sandbox_note: "Custom ACP agent running natively in the worktree. ACP client fs/terminal callbacks are disabled (not an OS sandbox).".into(),
            login_action: String::new(),
            install_hint: String::new(),
            registry: custom.registry,
        });
    }
    Ok(entries)
}

/// Resolve an `AgentId` to its launchable `AgentSpec` (loading secret env values from
/// `$STATE/chat/secrets.json` when applicable).
pub fn resolve_spec(
    agent_id: &AgentId,
    state: Option<&Path>,
    repo: Option<&Path>,
) -> Result<AgentSpec, String> {
    if let Some(preset) = preset_by_id(agent_id.as_str()) {
        return Ok(AgentSpec::from_preset(preset));
    }
    let custom = list_custom(state, repo)?
        .into_iter()
        .find(|agent| &agent.id == agent_id)
        .ok_or_else(|| format!("No ACP Chat agent named {agent_id:?} is configured."))?;
    let secret_storage = Storage::inspect(state, repo, Tokens::ChatSecrets)?;
    let secret_env = secret_storage
        .read::<SavedSecrets>(SECRETS_FILE)?
        .unwrap_or_default()
        .by_agent
        .remove(agent_id.as_str())
        .unwrap_or_default();
    let tier = if custom.registry.is_some() {
        AgentTier::Registry
    } else {
        AgentTier::Custom
    };
    Ok(AgentSpec {
        id: custom.id,
        name: custom.name,
        tier,
        program: custom.command,
        args: custom.args,
        version_args: vec!["--version".into()],
        minimum_version: None,
        env_denylist: Vec::new(),
        extra_env: custom.env,
        secret_env,
        mcp_stdio: custom.mcp_stdio,
        supports_same_child_load: true,
        reject_worktree_executable: false,
        hide_always_options: false,
        native_sandbox_note: "Custom ACP agent running natively in the worktree. ACP client fs/terminal callbacks are disabled (not an OS sandbox).".into(),
        can_disable_project_config: false,
        install_hint: String::new(),
        login_action: String::new(),
    })
}

/// Save or update a machine-local custom ACP agent and its optional secret env values.
pub async fn save_custom_agent(
    state: Option<&Path>,
    repo: Option<&Path>,
    mut agent: CustomAgent,
    set_secrets: BTreeMap<String, SecretEnvValue>,
    remove_secret_keys: &[String],
) -> Result<CustomAgent, String> {
    if is_builtin_preset(agent.id.as_str()) {
        return Err(format!(
            "{} is a built-in ACP preset and cannot be shadowed by a custom agent.",
            agent.id
        ));
    }
    if agent.name.trim().is_empty() {
        return Err("Custom ACP agent display name cannot be empty.".into());
    }
    if agent.command.trim().is_empty() {
        return Err("Custom ACP agent command cannot be empty.".into());
    }
    let secrets_storage = Storage::new(state, repo, Tokens::ChatSecrets)?;
    let _secrets_lock = secrets_storage.lock(SECRETS_LOCK).await?;
    let mut secrets: SavedSecrets = secrets_storage.read(SECRETS_FILE)?.unwrap_or_default();
    let agent_secrets = secrets
        .by_agent
        .entry(agent.id.as_str().to_owned())
        .or_default();
    for key in remove_secret_keys {
        agent_secrets.remove(key);
    }
    for (key, value) in set_secrets {
        agent_secrets.insert(key, value);
    }
    agent.secret_env_keys = agent_secrets.keys().cloned().collect();
    secrets_storage.save(SECRETS_FILE, &secrets)?;

    let agents_storage = Storage::new(state, repo, Tokens::ChatAgents)?;
    let _agents_lock = agents_storage.lock(AGENTS_LOCK).await?;
    let mut saved: SavedAgents = agents_storage.read(AGENTS_FILE)?.unwrap_or_default();
    saved.agents.retain(|existing| existing.id != agent.id);
    saved.agents.push(agent.clone());
    agents_storage.save(AGENTS_FILE, &saved)?;
    invalidate_probe_cache(&agent.id);
    Ok(agent)
}

/// Remove a custom or Registry-installed ACP agent and its stored secrets.
pub async fn remove_custom_agent(
    state: Option<&Path>,
    repo: Option<&Path>,
    id: &AgentId,
) -> Result<bool, String> {
    if is_builtin_preset(id.as_str()) {
        return Err(format!(
            "{id} is a built-in ACP preset and cannot be removed."
        ));
    }
    let agents_storage = Storage::new(state, repo, Tokens::ChatAgents)?;
    let _agents_lock = agents_storage.lock(AGENTS_LOCK).await?;
    let mut saved: SavedAgents = agents_storage.read(AGENTS_FILE)?.unwrap_or_default();
    let before = saved.agents.len();
    saved.agents.retain(|existing| &existing.id != id);
    let removed = saved.agents.len() != before;
    if removed {
        agents_storage.save(AGENTS_FILE, &saved)?;
    }

    let secrets_storage = Storage::new(state, repo, Tokens::ChatSecrets)?;
    let _secrets_lock = secrets_storage.lock(SECRETS_LOCK).await?;
    let mut secrets: SavedSecrets = secrets_storage.read(SECRETS_FILE)?.unwrap_or_default();
    if secrets.by_agent.remove(id.as_str()).is_some() {
        secrets_storage.save(SECRETS_FILE, &secrets)?;
    }
    invalidate_probe_cache(id);
    Ok(removed)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ProbeCacheKey {
    agent_id: AgentId,
    resolved_executable: PathBuf,
    args_hash: String,
    mtime_secs: Option<u64>,
    auth_generation: u64,
}

struct CachedProbe {
    recorded_at: Instant,
    availability: AgentAvailability,
}

fn probe_cache() -> &'static Mutex<BTreeMap<ProbeCacheKey, CachedProbe>> {
    static CACHE: OnceLock<Mutex<BTreeMap<ProbeCacheKey, CachedProbe>>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

pub fn invalidate_probe_cache(agent_id: &AgentId) {
    if let Ok(mut cache) = probe_cache().lock() {
        cache.retain(|key, _| &key.agent_id != agent_id);
    }
}

/// Probe an ACP agent strictly inside an empty private tempdir (never `cwd = worktree`),
/// returning availability, models, and live `configOptions` without triggering interactive auth.
pub async fn probe_agent(
    agent_id: &AgentId,
    state: Option<&Path>,
    repo: Option<&Path>,
    force_refresh: bool,
) -> AgentAvailability {
    let spec = match resolve_spec(agent_id, state, repo) {
        Ok(spec) => spec,
        Err(error) => {
            return AgentAvailability::Failed {
                executable: None,
                version: None,
                error,
            };
        }
    };
    probe_spec(&spec, repo, force_refresh).await
}

/// Probe `spec` as `probe_agent` does. With a `worktree`, a built-in preset whose executable
/// resolves inside it is refused before anything runs (C-TRUST-BEFORE-WORKTREE).
pub async fn probe_spec(
    spec: &AgentSpec,
    worktree: Option<&Path>,
    force_refresh: bool,
) -> AgentAvailability {
    let probe_dir = match platform::private_tempdir_in(
        "artifactize-acp-probe-",
        &platform::temporary_directory(),
    ) {
        Ok(dir) => dir,
        Err(error) => {
            return AgentAvailability::Failed {
                executable: None,
                version: None,
                error: format!("Cannot create ACP probe directory: {error}."),
            };
        }
    };
    let program_path = match platform::program_path(&spec.program) {
        Ok(path) => path,
        Err(error) => {
            return AgentAvailability::Failed {
                executable: None,
                version: None,
                error: format!("Invalid ACP command {:?}: {error}.", spec.program),
            };
        }
    };
    let resolved = match platform::program::resolve(&program_path, probe_dir.path()) {
        Ok(path) => platform::canonicalize(&path).unwrap_or(path),
        Err(_) => {
            return AgentAvailability::Missing {
                reason: if spec.install_hint.is_empty() {
                    format!("Executable {:?} is not on PATH.", spec.program)
                } else {
                    format!(
                        "{} (`{}`) is not on PATH. {}",
                        spec.name, spec.program, spec.install_hint
                    )
                },
            };
        }
    };
    if spec.reject_worktree_executable
        && let Some(worktree) = worktree
        && let Err(error) = trust::outside_worktree(&resolved, worktree)
    {
        return AgentAvailability::Failed {
            executable: Some(resolved.clone()),
            version: None,
            error,
        };
    }
    let mtime_secs = std::fs::metadata(&resolved)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|modified| modified.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs());
    let cache_key = ProbeCacheKey {
        agent_id: spec.id.clone(),
        resolved_executable: resolved.clone(),
        args_hash: trust::hash_args(&spec.args),
        mtime_secs,
        auth_generation: auth_generation(),
    };
    if !force_refresh
        && let Ok(cache) = probe_cache().lock()
        && let Some(entry) = cache.get(&cache_key)
        && entry.recorded_at.elapsed() < PROBE_CACHE_TTL
    {
        return entry.availability.clone();
    }

    let availability = run_live_probe(spec, &resolved, probe_dir.path()).await;
    if let Ok(mut cache) = probe_cache().lock() {
        cache.insert(
            cache_key,
            CachedProbe {
                recorded_at: Instant::now(),
                availability: availability.clone(),
            },
        );
    }
    availability
}

async fn run_live_probe(
    spec: &AgentSpec,
    executable: &Path,
    probe_dir: &Path,
) -> AgentAvailability {
    let cancel = CancellationToken::new();
    let deadline = tokio::time::Instant::now() + PROBE_TIMEOUT;

    let detected_version = probe_version(spec, executable, probe_dir, deadline, &cancel).await;
    if let Some(minimum) = &spec.minimum_version {
        match &detected_version {
            None => {
                return AgentAvailability::Failed {
                    executable: Some(executable.to_path_buf()),
                    version: None,
                    error: format!(
                        "Cannot read the {} CLI version; requires {minimum} or newer.",
                        spec.name
                    ),
                };
            }
            Some(found) if found < minimum => {
                return AgentAvailability::TooOld {
                    executable: executable.to_path_buf(),
                    version: found.to_string(),
                    required_version: minimum.to_string(),
                    reason: format!(
                        "{} {found} is too old; update to {minimum} or newer.",
                        spec.name
                    ),
                };
            }
            Some(_) => {}
        }
    }

    let mut args: Vec<OsString> = spec.args.iter().map(OsString::from).collect();
    // For Antigravity empty-tempdir probes, disable default customizations cleanly.
    if spec.id.as_str() == "antigravity" {
        args.push("--sandbox".into());
        args.push("--include-default-customizations=false".into());
    }
    let command = process::Command {
        program: executable.as_os_str().to_owned(),
        args,
        cwd: probe_dir.to_path_buf(),
        env: spec.build_env(false),
        timeout: PROBE_TIMEOUT,
    };
    let process_cancel = CancellationToken::new();
    let mut child = match process::spawn_supervised(&command, &process_cancel).await {
        Ok(child) => child,
        Err(error) => {
            return AgentAvailability::Failed {
                executable: Some(executable.to_path_buf()),
                version: detected_version.map(|v| v.to_string()),
                error: format!("Cannot start {}: {error}.", spec.name),
            };
        }
    };
    let stdout = child.stdout().take().expect("probe stdout piped");
    let stdin = child.stdin().take().expect("probe stdin piped");
    let mut stderr = child.stderr().take().expect("probe stderr piped");
    let stderr_task = tokio::spawn(async move { read_bounded_stderr(&mut stderr).await });
    let mut conn = Connection::new(
        &spec.name,
        &spec.install_hint,
        stdout,
        stdin,
        spec.hide_always_options,
        spec.known_secrets(),
    );
    let result = tokio::time::timeout(PROBE_TIMEOUT, conn.inspect_models(probe_dir))
        .await
        .unwrap_or_else(|_| {
            Err(Problem::Failed(format!(
                "{} did not answer the ACP probe in time.",
                spec.name
            )))
        });
    drop(conn);
    let _ = process::terminate(&mut child).await;
    let _ = child.cleanup().await;
    let stderr_bytes = crate::task::joined(stderr_task.await).unwrap_or_default();
    let stderr_text = platform::output_text(&stderr_bytes);

    let version_str = detected_version.map(|v| v.to_string());
    match result {
        Ok((models, config_options)) => AgentAvailability::Ready {
            executable: executable.to_path_buf(),
            version: version_str,
            models,
            config_options,
        },
        Err(problem) => {
            let classified = if stderr_text.contains(super::wire::AUTH_LINK_MARKER) {
                Problem::SignedOut {
                    agent: spec.name.clone(),
                    hint: spec.install_hint.clone(),
                }
            } else if matches!(problem, Problem::Failed(_)) {
                Problem::classify_text(&spec.name, &spec.install_hint, &stderr_text)
                    .unwrap_or(problem)
            } else {
                problem
            };
            match classified {
                Problem::SignedOut { .. } => AgentAvailability::NotSetUp {
                    executable: Some(executable.to_path_buf()),
                    version: version_str,
                    reason: format!("Sign in to {}.", spec.name),
                    login_action: spec.login_action.clone(),
                },
                other => AgentAvailability::Failed {
                    executable: Some(executable.to_path_buf()),
                    version: version_str,
                    error: crate::agent::error::Failure::from(other).message,
                },
            }
        }
    }
}

/// Maximum stderr bytes retained from a short-lived ACP probe or logout child (`C-ACP-EDGE-ISOLATION`).
const PROBE_STDERR_LIMIT: usize = 64 * 1024;
const PROBE_STDERR_CHUNK: usize = 4 * 1024;

pub(crate) async fn read_bounded_stderr(stderr: &mut tokio::process::ChildStderr) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    let mut kept = Vec::new();
    let mut buf = [0u8; PROBE_STDERR_CHUNK];
    while let Ok(count) = stderr.read(&mut buf).await {
        if count == 0 {
            break;
        }
        kept.extend_from_slice(&buf[..count]);
        if kept.len() > PROBE_STDERR_LIMIT {
            kept.drain(..kept.len() - PROBE_STDERR_LIMIT);
        }
    }
    kept
}

async fn probe_version(
    spec: &AgentSpec,
    executable: &Path,
    probe_dir: &Path,
    deadline: tokio::time::Instant,
    cancel: &CancellationToken,
) -> Option<semver::Version> {
    if spec.version_args.is_empty() {
        return None;
    }
    let command = process::Command {
        program: executable.as_os_str().to_owned(),
        args: spec.version_args.iter().map(OsString::from).collect(),
        cwd: probe_dir.to_path_buf(),
        env: spec.build_env(false),
        timeout: deadline
            .saturating_duration_since(tokio::time::Instant::now())
            .min(INITIALIZE_TIMEOUT),
    };
    let output = process::run(command, cancel.clone(), |_| async { Ok(()) })
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = platform::output_text(&output.stdout);
    text.split_whitespace()
        .find_map(|word| semver::Version::parse(word.trim_start_matches('v')).ok())
}

/// Discover official ACP Registry entries (`latest` is discovery-only; never auto-executed).
pub async fn discover_registry() -> Result<Vec<RegistryEntry>, String> {
    let url = platform::environment::var_text(REGISTRY_URL_ENV)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_REGISTRY_URL.to_owned());
    let client = reqwest::Client::builder()
        .timeout(REGISTRY_FETCH_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|error| format!("Cannot build HTTP client for ACP Registry: {error}."))?;
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|error| format!("Cannot fetch ACP Registry from {url}: {error}."))?;
    if !response.status().is_success() {
        return Err(format!(
            "ACP Registry returned HTTP {} for {url}.",
            response.status()
        ));
    }
    let raw: RawRegistryFeed = response
        .json()
        .await
        .map_err(|error| format!("Cannot parse ACP Registry feed: {error}."))?;
    let platform_key = current_platform_key();
    let mut entries = Vec::new();
    for item in raw.agents {
        let Ok(id) = item.id.parse::<AgentId>() else {
            continue;
        };
        if let Some(spec) = resolve_registry_spec(&id, &item, platform_key) {
            entries.push(RegistryEntry {
                id,
                name: item.name,
                version: item.version,
                description: item.description.unwrap_or_default(),
                spec,
            });
        }
    }
    Ok(entries)
}

/// Install a pinned ACP Registry entry into `$STATE/chat/runtimes/<agent>/<exact-version>/`
/// after explicit full-spec consent. Checks MSIX constraints, verifies SHA-256, rejects
/// symlinks/reparse points/zip-slip, and defaults to `--ignore-scripts` / `--only-binary=:all:`.
pub async fn install_registry_agent(
    state: Option<&Path>,
    repo: Option<&Path>,
    spec: RegistryInstallSpec,
) -> Result<CustomAgent, String> {
    if is_builtin_preset(spec.registry_id.as_str()) {
        return Err(format!(
            "{} matches a built-in ACP preset and cannot shadow it.",
            spec.registry_id
        ));
    }
    if platform::install::is_msix() {
        return Err(
            "Microsoft Store (MSIX) installations restrict dynamic runtime execution under state directories; install the agent CLI onto PATH instead."
                .into(),
        );
    }
    if spec.exact_version.trim().is_empty()
        || spec.exact_version.eq_ignore_ascii_case("latest")
        || !spec
            .exact_version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+'))
    {
        return Err(format!(
            "Registry installs require an exact pinned version (got {:?}).",
            spec.exact_version
        ));
    }
    let chat_storage = Storage::new(state, repo, Tokens::ChatAgents)?;
    let runtimes_root = chat_storage.directory.join("runtimes");
    platform::create_private_dir_all(&runtimes_root).map_err(|e| e.to_string())?;
    let target_dir = runtimes_root
        .join(spec.registry_id.as_str())
        .join(&spec.exact_version);
    platform::create_private_dir_all(&target_dir).map_err(|e| e.to_string())?;

    let entrypoint = match spec.distribution_kind {
        DistributionKind::Binary => install_pinned_binary(&spec, &target_dir).await?,
        DistributionKind::Archive => {
            return Err(
                "Archive extraction requires a verified binary or npm/uvx package entrypoint; unpack manually or choose a Binary/Npx/Uvx distribution."
                    .into(),
            );
        }
        DistributionKind::Npx => install_pinned_npm(&spec, &target_dir).await?,
        DistributionKind::Uvx => install_pinned_uv(&spec, &target_dir).await?,
    };

    // Verify no symlink/reparse point escape and that final executable stays within target_dir.
    verify_tree_no_links(&target_dir)?;
    let canonical_target = platform::canonicalize(&target_dir).map_err(|e| e.to_string())?;
    let canonical_entry = platform::canonicalize(&entrypoint).map_err(|e| e.to_string())?;
    if !platform::is_within(&canonical_entry, &canonical_target) {
        return Err(
            "Installed ACP runtime entrypoint escaped its pinned runtime directory.".into(),
        );
    }
    let entrypoint_sha256 = trust::hash_executable(&canonical_entry)?;

    let agent = CustomAgent {
        id: spec.registry_id.clone(),
        name: spec.display_name.clone(),
        command: platform::path_text(&canonical_entry),
        args: spec.args.clone(),
        env: spec.env_pairs.clone(),
        secret_env_keys: Vec::new(),
        mcp_stdio: true,
        registry: Some(spec),
        entrypoint_sha256: Some(entrypoint_sha256),
    };
    save_custom_agent(state, repo, agent, BTreeMap::new(), &[]).await
}

async fn install_pinned_binary(
    spec: &RegistryInstallSpec,
    target_dir: &Path,
) -> Result<PathBuf, String> {
    let url = url::Url::parse(&spec.package_or_url)
        .map_err(|_| format!("Invalid binary URL {:?}.", spec.package_or_url))?;
    if !matches!(url.scheme(), "https" | "http") {
        return Err("Registry binary downloads require an http(s) URL.".into());
    }
    let client = reqwest::Client::builder()
        .timeout(REGISTRY_FETCH_TIMEOUT)
        .build()
        .map_err(|error| format!("Cannot build HTTP client: {error}."))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("Cannot download Registry binary: {error}."))?;
    if !response.status().is_success() {
        return Err(format!(
            "Registry binary download failed with HTTP {}.",
            response.status()
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("Cannot read Registry binary bytes: {error}."))?;
    if bytes.len() > MAX_REGISTRY_ARTIFACT_BYTES {
        return Err("Registry binary exceeds the 256 MiB safety limit.".into());
    }
    let computed = hex_sha256(&bytes);
    match &spec.artifact_sha256 {
        Some(expected) if expected.eq_ignore_ascii_case(&computed) => {}
        Some(expected) => {
            return Err(format!(
                "SHA-256 mismatch for {}: expected {expected}, got {computed}.",
                spec.registry_id
            ));
        }
        None if spec.allow_unsigned_digest => {}
        None => {
            return Err(format!(
                "Unsigned Registry binary blocked by default. Re-confirm install with computed SHA-256 {computed}."
            ));
        }
    }
    let final_path = platform::install_executable(target_dir, spec.registry_id.as_str(), &bytes)
        .map_err(|e| format!("Cannot install {}: {e}.", spec.registry_id))?;
    Ok(final_path)
}

async fn install_pinned_npm(
    spec: &RegistryInstallSpec,
    target_dir: &Path,
) -> Result<PathBuf, String> {
    let empty_cwd = platform::private_tempdir_in("acp-npm-", &platform::temporary_directory())
        .map_err(|e| e.to_string())?;
    let mut args = vec![
        OsString::from("install"),
        OsString::from("--prefix"),
        target_dir.as_os_str().to_owned(),
        OsString::from("--no-audit"),
        OsString::from("--no-fund"),
        OsString::from("--save-exact"),
    ];
    if !spec.build_scripts_allowed {
        args.push(OsString::from("--ignore-scripts"));
    }
    args.push(OsString::from(format!(
        "{}@{}",
        spec.package_or_url, spec.exact_version
    )));
    let command = process::Command {
        program: OsString::from("npm"),
        args,
        cwd: empty_cwd.path().to_path_buf(),
        env: platform::environment::for_command(),
        timeout: Duration::from_secs(120),
    };
    let output = process::run(command, CancellationToken::new(), |_| async { Ok(()) })
        .await
        .map_err(|error| format!("Pinned npm install failed: {error}."))?;
    if !output.status.success() {
        return Err(format!(
            "Pinned npm install failed: {}",
            platform::output_text(&output.stderr).trim()
        ));
    }
    let bin_dir = target_dir.join("node_modules").join(".bin");
    if let Ok(resolved) =
        platform::program::resolve(&bin_dir.join(spec.registry_id.as_str()), &bin_dir)
    {
        Ok(resolved)
    } else {
        // Fall back to the first runnable entrypoint in node_modules/.bin via platform lookup.
        let handle = platform::open_directory(&bin_dir).map_err(|e| e.to_string())?;
        let entries = platform::read_dir(&handle).map_err(|e| e.to_string())?;
        let mut found = None;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            if let Ok(resolved) =
                platform::program::resolve(&bin_dir.join(entry.file_name()), &bin_dir)
            {
                found = Some(resolved);
                break;
            }
        }
        found.ok_or_else(|| {
            "Pinned npm install produced no runnable binary in node_modules/.bin.".into()
        })
    }
}

async fn install_pinned_uv(
    spec: &RegistryInstallSpec,
    target_dir: &Path,
) -> Result<PathBuf, String> {
    let empty_cwd = platform::private_tempdir_in("acp-uv-", &platform::temporary_directory())
        .map_err(|e| e.to_string())?;
    let venv_dir = target_dir.join("venv");
    let create = process::Command {
        program: OsString::from("uv"),
        args: vec![OsString::from("venv"), venv_dir.as_os_str().to_owned()],
        cwd: empty_cwd.path().to_path_buf(),
        env: platform::environment::for_command(),
        timeout: Duration::from_secs(60),
    };
    let created = process::run(create, CancellationToken::new(), |_| async { Ok(()) })
        .await
        .map_err(|error| format!("Cannot create pinned uv venv: {error}."))?;
    if !created.status.success() {
        return Err(format!(
            "uv venv failed: {}",
            platform::output_text(&created.stderr).trim()
        ));
    }
    let mut args = vec![
        OsString::from("pip"),
        OsString::from("install"),
        OsString::from("--python"),
        venv_dir.as_os_str().to_owned(),
    ];
    if !spec.build_scripts_allowed {
        args.push(OsString::from("--only-binary=:all:"));
    }
    args.push(OsString::from(format!(
        "{}=={}",
        spec.package_or_url, spec.exact_version
    )));
    let install = process::Command {
        program: OsString::from("uv"),
        args,
        cwd: empty_cwd.path().to_path_buf(),
        env: platform::environment::for_command(),
        timeout: Duration::from_secs(120),
    };
    let installed = process::run(install, CancellationToken::new(), |_| async { Ok(()) })
        .await
        .map_err(|error| format!("Pinned uv install failed: {error}."))?;
    if !installed.status.success() {
        return Err(format!(
            "Pinned uv install failed: {}",
            platform::output_text(&installed.stderr).trim()
        ));
    }
    let bin_dir = platform::venv_bin_dir(&venv_dir);
    platform::program::resolve(&bin_dir.join(spec.registry_id.as_str()), &bin_dir).map_err(|_| {
        "Pinned uv install produced no runnable entrypoint in the isolated venv.".into()
    })
}

/// Reject symlinks and reparse points anywhere inside a pinned runtime directory.
fn verify_tree_no_links(root: &Path) -> Result<(), String> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let handle = platform::open_directory(&dir).map_err(|e| e.to_string())?;
        for entry in platform::read_dir(&handle).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let child = dir.join(entry.file_name());
            match entry.file_type().map_err(|e| e.to_string())? {
                platform::FileKind::Directory => stack.push(child),
                platform::FileKind::File => {}
                platform::FileKind::Symlink | platform::FileKind::Other => {
                    return Err(format!(
                        "Refusing symlink or special file {} inside pinned ACP runtime.",
                        platform::path_text(&child)
                    ));
                }
            }
        }
    }
    Ok(())
}

fn hex_sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for &byte in digest.as_slice() {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn current_platform_key() -> &'static str {
    platform::registry_platform()
}

#[derive(Deserialize)]
struct RawRegistryFeed {
    #[serde(default)]
    agents: Vec<RawRegistryAgent>,
}

#[derive(Deserialize)]
struct RawRegistryAgent {
    id: String,
    name: String,
    version: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    distribution: RawDistribution,
}

#[derive(Default, Deserialize)]
struct RawDistribution {
    #[serde(default)]
    binary: BTreeMap<String, RawBinaryTarget>,
    #[serde(default)]
    npx: Option<RawPackageTarget>,
    #[serde(default)]
    uvx: Option<RawPackageTarget>,
}

#[derive(Deserialize)]
struct RawBinaryTarget {
    archive: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    sha256: Option<String>,
}

#[derive(Deserialize)]
struct RawPackageTarget {
    package: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

fn resolve_registry_spec(
    id: &AgentId,
    item: &RawRegistryAgent,
    platform_key: &str,
) -> Option<RegistryInstallSpec> {
    if let Some(bin) = item.distribution.binary.get(platform_key) {
        return Some(RegistryInstallSpec {
            registry_id: id.clone(),
            display_name: item.name.clone(),
            exact_version: item.version.clone(),
            platform: platform_key.to_owned(),
            distribution_kind: if bin.archive.ends_with(".tar.gz")
                || bin.archive.ends_with(".zip")
                || bin.archive.ends_with(".tgz")
            {
                DistributionKind::Archive
            } else {
                DistributionKind::Binary
            },
            package_or_url: bin.archive.clone(),
            args: bin.args.clone(),
            env_pairs: bin.env.clone(),
            build_scripts_allowed: false,
            artifact_sha256: bin.sha256.clone(),
            allow_unsigned_digest: false,
        });
    }
    if let Some(npx) = &item.distribution.npx {
        return Some(RegistryInstallSpec {
            registry_id: id.clone(),
            display_name: item.name.clone(),
            exact_version: item.version.clone(),
            platform: platform_key.to_owned(),
            distribution_kind: DistributionKind::Npx,
            package_or_url: npx.package.clone(),
            args: npx.args.clone(),
            env_pairs: npx.env.clone(),
            build_scripts_allowed: false,
            artifact_sha256: None,
            allow_unsigned_digest: false,
        });
    }
    if let Some(uvx) = &item.distribution.uvx {
        return Some(RegistryInstallSpec {
            registry_id: id.clone(),
            display_name: item.name.clone(),
            exact_version: item.version.clone(),
            platform: platform_key.to_owned(),
            distribution_kind: DistributionKind::Uvx,
            package_or_url: uvx.package.clone(),
            args: uvx.args.clone(),
            env_pairs: uvx.env.clone(),
            build_scripts_allowed: false,
            artifact_sha256: None,
            allow_unsigned_digest: false,
        });
    }
    None
}

/// Helper to read the executable's launcher fingerprint for an agent spec.
pub fn compute_launcher_fingerprint(
    spec: &AgentSpec,
    worktree: &Path,
) -> Result<LauncherFingerprint, String> {
    let resolved = trust::resolve_executable(
        &spec.program,
        Some(worktree),
        spec.reject_worktree_executable,
    )?;
    LauncherFingerprint::compute(&resolved, &spec.args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn custom_agent_roundtrip_stores_secrets_separately_and_blocks_preset_shadowing() {
        let root = crate::test_os::tempdir();
        let state = root.path().join("state");
        let worktree = root.path().join("work");
        std::fs::create_dir_all(&worktree).unwrap();

        let shadow = CustomAgent {
            id: "antigravity".parse().unwrap(),
            name: "Shadow".into(),
            command: "fake".into(),
            args: Vec::new(),
            env: BTreeMap::new(),
            secret_env_keys: Vec::new(),
            mcp_stdio: true,
            registry: None,
            entrypoint_sha256: None,
        };
        assert!(
            save_custom_agent(Some(&state), Some(&worktree), shadow, BTreeMap::new(), &[])
                .await
                .is_err()
        );

        let custom = CustomAgent {
            id: "my-acp".parse().unwrap(),
            name: "My ACP".into(),
            command: "my-acp-bin".into(),
            args: vec!["--stdio".into()],
            env: BTreeMap::from([("PLAIN_FLAG".into(), "1".into())]),
            secret_env_keys: Vec::new(),
            mcp_stdio: true,
            registry: None,
            entrypoint_sha256: None,
        };
        let mut secrets = BTreeMap::new();
        secrets.insert(
            "MY_SECRET".into(),
            SecretEnvValue::parse("top-secret-value-999").unwrap(),
        );
        let saved = save_custom_agent(Some(&state), Some(&worktree), custom, secrets, &[])
            .await
            .unwrap();
        assert_eq!(saved.secret_env_keys, vec!["MY_SECRET".to_owned()]);

        // agents.json must never contain the secret value.
        let agents_json = std::fs::read_to_string(state.join("chat/agents.json")).unwrap();
        assert!(!agents_json.contains("top-secret-value-999"));
        assert!(agents_json.contains("MY_SECRET"));

        let spec = resolve_spec(&saved.id, Some(&state), Some(&worktree)).unwrap();
        assert_eq!(
            spec.known_secrets(),
            vec!["top-secret-value-999".to_owned()]
        );
        assert!(is_chat_agent("my-acp", Some(&state), Some(&worktree)));

        assert!(
            remove_custom_agent(Some(&state), Some(&worktree), &saved.id)
                .await
                .unwrap()
        );
        assert!(!is_chat_agent("my-acp", Some(&state), Some(&worktree)));
    }
}
