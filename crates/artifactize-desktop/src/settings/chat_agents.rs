//! Settings → Chat Agents: 3-tier ACP catalog (built-in presets, official ACP Registry pinned
//! installs, and machine-local custom ACP agents). Secret env values are write-only and never
//! echoed back over IPC.

use std::collections::BTreeMap;

use artifactize_core::acp::{
    AgentId,
    catalog::{self, AgentAvailability, AgentTier, CustomAgent, DistributionKind, SecretEnvValue},
};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{App, Failure, browse::PathKey, chat::ChatConfigOption};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ChatAgentTier {
    Preset,
    Registry,
    Custom,
}

impl From<AgentTier> for ChatAgentTier {
    fn from(tier: AgentTier) -> Self {
        match tier {
            AgentTier::Preset => Self::Preset,
            AgentTier::Registry => Self::Registry,
            AgentTier::Custom => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ChatDistributionKind {
    Binary,
    Archive,
    Npx,
    Uvx,
}

impl From<DistributionKind> for ChatDistributionKind {
    fn from(kind: DistributionKind) -> Self {
        match kind {
            DistributionKind::Binary => Self::Binary,
            DistributionKind::Archive => Self::Archive,
            DistributionKind::Npx => Self::Npx,
            DistributionKind::Uvx => Self::Uvx,
        }
    }
}

impl From<ChatDistributionKind> for DistributionKind {
    fn from(kind: ChatDistributionKind) -> Self {
        match kind {
            ChatDistributionKind::Binary => Self::Binary,
            ChatDistributionKind::Archive => Self::Archive,
            ChatDistributionKind::Npx => Self::Npx,
            ChatDistributionKind::Uvx => Self::Uvx,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatRegistryInstallSpec {
    pub registry_id: String,
    pub display_name: String,
    pub exact_version: String,
    pub platform: String,
    pub distribution_kind: ChatDistributionKind,
    pub package_or_url: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env_pairs: BTreeMap<String, String>,
    #[serde(default)]
    pub build_scripts_allowed: bool,
    #[serde(default)]
    pub artifact_sha256: Option<String>,
    #[serde(default)]
    pub allow_unsigned_digest: bool,
}

impl From<catalog::RegistryInstallSpec> for ChatRegistryInstallSpec {
    fn from(spec: catalog::RegistryInstallSpec) -> Self {
        Self {
            registry_id: spec.registry_id.to_string(),
            display_name: spec.display_name,
            exact_version: spec.exact_version,
            platform: spec.platform,
            distribution_kind: spec.distribution_kind.into(),
            package_or_url: spec.package_or_url,
            args: spec.args,
            env_pairs: spec.env_pairs,
            build_scripts_allowed: spec.build_scripts_allowed,
            artifact_sha256: spec.artifact_sha256,
            allow_unsigned_digest: spec.allow_unsigned_digest,
        }
    }
}

impl TryFrom<ChatRegistryInstallSpec> for catalog::RegistryInstallSpec {
    type Error = Failure;

    fn try_from(spec: ChatRegistryInstallSpec) -> Result<Self, Failure> {
        Ok(Self {
            registry_id: spec.registry_id.parse().map_err(Failure::Invalid)?,
            display_name: spec.display_name,
            exact_version: spec.exact_version,
            platform: spec.platform,
            distribution_kind: spec.distribution_kind.into(),
            package_or_url: spec.package_or_url,
            args: spec.args,
            env_pairs: spec.env_pairs,
            build_scripts_allowed: spec.build_scripts_allowed,
            artifact_sha256: spec.artifact_sha256,
            allow_unsigned_digest: spec.allow_unsigned_digest,
        })
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatRegistryEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub spec: ChatRegistryInstallSpec,
}

impl From<catalog::RegistryEntry> for ChatRegistryEntry {
    fn from(entry: catalog::RegistryEntry) -> Self {
        Self {
            id: entry.id.to_string(),
            name: entry.name,
            version: entry.version,
            description: entry.description,
            spec: entry.spec.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatAgentCatalogEntry {
    pub id: String,
    pub name: String,
    pub tier: ChatAgentTier,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    /// Secret env variable names only — values are never returned over IPC.
    pub secret_env_keys: Vec<String>,
    pub mcp_stdio: bool,
    pub can_disable_project_config: bool,
    pub native_sandbox_note: String,
    pub login_action: String,
    pub install_hint: String,
    pub registry: Option<ChatRegistryInstallSpec>,
}

impl From<catalog::CatalogEntry> for ChatAgentCatalogEntry {
    fn from(entry: catalog::CatalogEntry) -> Self {
        Self {
            id: entry.id.to_string(),
            name: entry.name,
            tier: entry.tier.into(),
            command: entry.command,
            args: entry.args,
            env: entry.env,
            secret_env_keys: entry.secret_env_keys,
            mcp_stdio: entry.mcp_stdio,
            can_disable_project_config: entry.can_disable_project_config,
            native_sandbox_note: entry.native_sandbox_note,
            login_action: entry.login_action,
            install_hint: entry.install_hint,
            registry: entry.registry.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatAgentModel {
    pub slug: String,
    pub display_name: String,
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ChatAgentAvailability {
    Ready {
        executable: PathKey,
        version: Option<String>,
        models: Vec<ChatAgentModel>,
        config_options: Vec<ChatConfigOption>,
    },
    NotSetUp {
        executable: Option<PathKey>,
        version: Option<String>,
        reason: String,
        login_action: String,
    },
    Missing {
        reason: String,
    },
    TooOld {
        executable: PathKey,
        version: String,
        required_version: String,
        reason: String,
    },
    Failed {
        executable: Option<PathKey>,
        version: Option<String>,
        error: String,
    },
}

impl From<AgentAvailability> for ChatAgentAvailability {
    fn from(availability: AgentAvailability) -> Self {
        match availability {
            AgentAvailability::Ready {
                executable,
                version,
                models,
                config_options,
            } => Self::Ready {
                executable: PathKey::new(&executable),
                version,
                models: models
                    .into_iter()
                    .map(|m| ChatAgentModel {
                        slug: m.slug.to_string(),
                        display_name: m.display_name,
                        efforts: m.efforts,
                        default_effort: m.default_effort,
                    })
                    .collect(),
                config_options: config_options.into_iter().map(Into::into).collect(),
            },
            AgentAvailability::NotSetUp {
                executable,
                version,
                reason,
                login_action,
            } => Self::NotSetUp {
                executable: executable.as_deref().map(PathKey::new),
                version,
                reason,
                login_action,
            },
            AgentAvailability::Missing { reason } => Self::Missing { reason },
            AgentAvailability::TooOld {
                executable,
                version,
                required_version,
                reason,
            } => Self::TooOld {
                executable: PathKey::new(&executable),
                version,
                required_version,
                reason,
            },
            AgentAvailability::Failed {
                executable,
                version,
                error,
            } => Self::Failed {
                executable: executable.as_deref().map(PathKey::new),
                version,
                error,
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatCustomAgentInput {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Write-only secret env values; never echoed back.
    #[serde(default)]
    pub set_secrets: BTreeMap<String, String>,
    #[serde(default)]
    pub remove_secret_keys: Vec<String>,
    #[serde(default = "default_true")]
    pub mcp_stdio: bool,
}

fn default_true() -> bool {
    true
}

fn location(app: &App) -> (Option<&std::path::Path>, Option<&std::path::Path>) {
    (Some(&app.launch.state), app.launch.settings_repo())
}

/// List all built-in ACP presets, Registry-installed, and custom ACP Chat agents.
#[tauri::command]
#[specta::specta]
pub fn chat_agents_list(app: tauri::State<'_, App>) -> Result<Vec<ChatAgentCatalogEntry>, Failure> {
    let (state, repo) = location(&app);
    catalog::list_catalog(state, repo)
        .map(|entries| entries.into_iter().map(Into::into).collect())
        .map_err(Failure::Setting)
}

/// Probe one ACP Chat agent strictly inside an empty private tempdir.
#[tauri::command]
#[specta::specta]
pub async fn chat_agent_probe(
    app: tauri::State<'_, App>,
    agent: String,
    refresh: bool,
) -> Result<ChatAgentAvailability, Failure> {
    let id: AgentId = agent.parse().map_err(Failure::Invalid)?;
    let (state, repo) = location(&app);
    Ok(catalog::probe_agent(&id, state, repo, refresh).await.into())
}

/// Save or update a machine-local custom ACP Chat agent and its write-only secrets.
#[tauri::command]
#[specta::specta]
pub async fn chat_agent_custom_save(
    app: tauri::State<'_, App>,
    input: ChatCustomAgentInput,
) -> Result<ChatAgentCatalogEntry, Failure> {
    let id: AgentId = input.id.parse().map_err(Failure::Invalid)?;
    let mut secrets = BTreeMap::new();
    for (key, value) in input.set_secrets {
        let key = key.trim().to_owned();
        if key.is_empty() {
            return Err(Failure::Invalid(
                "Secret environment variable name cannot be empty.".into(),
            ));
        }
        let parsed = SecretEnvValue::parse(&value).map_err(Failure::Invalid)?;
        secrets.insert(key, parsed);
    }
    let agent = CustomAgent {
        id: id.clone(),
        name: input.name,
        command: input.command,
        args: input.args,
        env: input.env,
        secret_env_keys: Vec::new(),
        mcp_stdio: input.mcp_stdio,
        registry: None,
        entrypoint_sha256: None,
    };
    let (state, repo) = location(&app);
    catalog::save_custom_agent(state, repo, agent, secrets, &input.remove_secret_keys)
        .await
        .map_err(Failure::Setting)?;
    let entries = catalog::list_catalog(state, repo).map_err(Failure::Setting)?;
    entries
        .into_iter()
        .find(|entry| entry.id == id)
        .map(Into::into)
        .ok_or_else(|| Failure::Setting(format!("Saved custom ACP agent {id} not found.")))
}

/// Remove a custom or Registry-installed ACP Chat agent and its stored secrets.
#[tauri::command]
#[specta::specta]
pub async fn chat_agent_custom_remove(
    app: tauri::State<'_, App>,
    agent: String,
) -> Result<bool, Failure> {
    let id: AgentId = agent.parse().map_err(Failure::Invalid)?;
    let (state, repo) = location(&app);
    catalog::remove_custom_agent(state, repo, &id)
        .await
        .map_err(Failure::Setting)
}

/// Discover official ACP Registry entries (discovery-only; never executes anything).
#[tauri::command]
#[specta::specta]
pub async fn chat_agent_registry_list() -> Result<Vec<ChatRegistryEntry>, Failure> {
    catalog::discover_registry()
        .await
        .map(|entries| entries.into_iter().map(Into::into).collect())
        .map_err(Failure::Account)
}

/// Install a pinned official ACP Registry entry after explicit user consent.
#[tauri::command]
#[specta::specta]
pub async fn chat_agent_registry_install(
    app: tauri::State<'_, App>,
    spec: ChatRegistryInstallSpec,
) -> Result<ChatAgentCatalogEntry, Failure> {
    let spec: catalog::RegistryInstallSpec = spec.try_into()?;
    let id = spec.registry_id.clone();
    let (state, repo) = location(&app);
    catalog::install_registry_agent(state, repo, spec)
        .await
        .map_err(Failure::Setting)?;
    let entries = catalog::list_catalog(state, repo).map_err(Failure::Setting)?;
    entries
        .into_iter()
        .find(|entry| entry.id == id)
        .map(Into::into)
        .ok_or_else(|| Failure::Setting(format!("Installed Registry ACP agent {id} not found.")))
}
