//! Project-local Artifact types. Built-ins and installed plugins use the same manifest.

use std::{collections::BTreeMap, fs::File, io::Read, path::Path, sync::Arc};

use serde::{Deserialize, Serialize};

use super::{Artifact, ArtifactKind, ConfigError, LogicalPath, Views, discovery::DEFINITION_DIR};
use crate::{platform, scope};

pub(super) const DIRECTORY: &str = "plugins";
const MANIFEST: &str = "plugin.toml";
/// A viewer opens one web document. Bound what a launch copies across IPC.
const DOCUMENT_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BuiltinViewer {
    Files,
    Markdown,
    Artifactize,
}

impl BuiltinViewer {
    const MANIFESTS: [(Self, &str); 3] = [
        (Self::Files, include_str!("plugins/files.toml")),
        (Self::Markdown, include_str!("plugins/markdown.toml")),
        (Self::Artifactize, include_str!("plugins/artifactize.toml")),
    ];
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    #[serde(rename = "type")]
    name: super::ArtifactType,
    title: String,
    #[serde(default)]
    views: Views,
    viewer: Option<WebEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WebEntry {
    entry: LogicalPath,
}

/// A viewer selected by the type's manifest, never by a path supplied by a web page.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ViewerEntry {
    Builtin { viewer: BuiltinViewer },
    Web { html: Arc<str> },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    #[serde(rename = "type")]
    pub name: super::ArtifactType,
    pub title: String,
    pub builtin: bool,
    pub viewer: bool,
}

#[derive(Debug)]
pub(crate) struct Plugin {
    pub info: PluginInfo,
    pub viewer: Option<ViewerEntry>,
    views: Views,
}

impl Plugin {
    fn parse(
        source: &str,
        viewer: Option<BuiltinViewer>,
        directory: Option<&Path>,
    ) -> Result<Self, String> {
        let manifest: Manifest = toml::from_str(source).map_err(|error| error.to_string())?;
        super::validation::text(&manifest.title, "Plugin title")?;
        let table = toml::de::DeTable::parse(source).map_err(|error| error.to_string())?;
        let document =
            toml::Spanned::new(table.span(), toml::de::DeValue::Table(table.into_inner()));
        manifest.views.validate(
            &super::location::Location::root(source, &document).child("views"),
            &mut super::schemas::Schemas::default(),
            None,
        )?;
        let entry = match (viewer, &manifest.viewer, directory) {
            (Some(viewer), None, None) => Some(ViewerEntry::Builtin { viewer }),
            (None, Some(web), Some(directory)) => {
                if Path::new(web.entry.as_str())
                    .extension()
                    .is_none_or(|extension| extension != "html")
                {
                    return Err(
                        "viewer.entry must name an .html file in the plugin directory.".into(),
                    );
                }
                let file = scope::open_scoped(directory, web.entry.as_str())
                    .map_err(|error| format!("viewer.entry {}: {error}", web.entry))?;
                Some(ViewerEntry::Web {
                    html: read_document(file)?.into(),
                })
            }
            (None, None, Some(_)) => None,
            _ => return Err("Invalid plugin viewer entry.".into()),
        };
        let declares_viewer = manifest
            .views
            .human_tools
            .values()
            .any(|tool| matches!(tool, super::HumanTool::Viewer(_)));
        if declares_viewer != entry.is_some() {
            return Err(
                "A viewer entry and a Human tool with kind = \"viewer\" must be declared together."
                    .into(),
            );
        }
        Ok(Self {
            info: PluginInfo {
                name: manifest.name,
                title: manifest.title,
                builtin: viewer.is_some(),
                viewer: entry.is_some(),
            },
            viewer: entry,
            views: manifest.views,
        })
    }

    pub(super) fn apply(self: &Arc<Self>, artifact: &mut Artifact) -> Result<(), String> {
        if artifact.kind() == ArtifactKind::Live && (self.info.viewer || self.views.agent_tools.values().any(|tool| {
            matches!(tool, super::AgentTool::Builtin(tool) if !matches!(tool.builtin, super::Builtin::Graph | super::Builtin::Help | super::Builtin::Open))
        }) || self.views.human_tools.values().any(|tool| {
            matches!(tool, super::HumanTool::Builtin(tool) if !matches!(tool.builtin, super::Builtin::Graph | super::Builtin::Help | super::Builtin::Open))
        })) {
            return Err("This plugin needs a local Artifact target, not a Live URI.".into());
        }
        if matches!(
            self.viewer,
            Some(ViewerEntry::Builtin {
                viewer: BuiltinViewer::Artifactize
            })
        ) && !artifact
            .path
            .local()
            .is_some_and(|path| path.starts_with(DEFINITION_DIR))
        {
            return Err(
                "type = \"artifactize\" must target definitions under .artifactize/.".into(),
            );
        }
        inherit(
            &mut artifact.views.agent_tools,
            &self.views.agent_tools,
            "Agent tool",
        )?;
        inherit(
            &mut artifact.views.agent_sessions,
            &self.views.agent_sessions,
            "Agent tool session",
        )?;
        inherit(
            &mut artifact.views.human_tools,
            &self.views.human_tools,
            "Human tool",
        )?;
        artifact.plugin = Some(Arc::clone(self));
        Ok(())
    }
}

fn inherit<T: Clone>(
    own: &mut BTreeMap<super::ToolOperationName, T>,
    inherited: &BTreeMap<super::ToolOperationName, T>,
    kind: &str,
) -> Result<(), String> {
    for (name, tool) in inherited {
        if own.insert(name.clone(), tool.clone()).is_some() {
            return Err(format!(
                "{kind} {name} is already provided by the plugin; remove or rename your declaration."
            ));
        }
    }
    Ok(())
}

pub(crate) fn read_document(file: File) -> Result<String, String> {
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Expected a regular text file.".into());
    }
    let mut buffer = String::new();
    file.take(DOCUMENT_BYTES + 1)
        .read_to_string(&mut buffer)
        .map_err(|error| error.to_string())?;
    if buffer.len() as u64 > DOCUMENT_BYTES {
        return Err("Viewer document exceeds 8 MiB.".into());
    }
    Ok(buffer)
}

fn builtins() -> impl Iterator<Item = Plugin> {
    BuiltinViewer::MANIFESTS
        .into_iter()
        .map(|(viewer, source)| {
            Plugin::parse(source, Some(viewer), None).expect("embedded plugin manifest")
        })
}

fn definitions(root: &Path) -> Result<std::path::PathBuf, ConfigError> {
    let root = platform::canonicalize(root).map_err(|error| ConfigError::new(root, error))?;
    let definitions = root.join(DEFINITION_DIR);
    if !platform::paths::reusable_directory(&root, DEFINITION_DIR)
        .map_err(|error| ConfigError::new(&definitions, error))?
    {
        return Err(ConfigError::new(
            &definitions,
            "Not an Artifactize project.",
        ));
    }
    Ok(definitions)
}

fn read(directory: &Path) -> Result<Plugin, ConfigError> {
    let manifest = directory.join(MANIFEST);
    let source = scope::open_scoped(directory, MANIFEST)
        .map_err(|error| ConfigError::new(&manifest, error))?;
    let source = read_document(source).map_err(|error| ConfigError::new(&manifest, error))?;
    Plugin::parse(&source, None, Some(directory))
        .map_err(|error| ConfigError::new(&manifest, error))
}

pub(super) fn catalog(
    root: &Path,
) -> Result<BTreeMap<super::ArtifactType, Arc<Plugin>>, ConfigError> {
    let mut plugins: BTreeMap<_, _> = builtins()
        .map(|plugin| (plugin.info.name.clone(), Arc::new(plugin)))
        .collect();
    let parent = root.join(DEFINITION_DIR);
    let directory = parent.join(DIRECTORY);
    if !platform::paths::reusable_directory(root, DEFINITION_DIR)
        .map_err(|error| ConfigError::new(&parent, error))?
    {
        return Ok(plugins);
    }
    if !platform::paths::reusable_directory(&parent, DIRECTORY)
        .map_err(|error| ConfigError::new(&directory, error))?
    {
        return Ok(plugins);
    }
    let opened = platform::open_directory(&directory)
        .map_err(|error| ConfigError::new(&directory, error))?;
    for entry in platform::read_dir(&opened).map_err(|error| ConfigError::new(&directory, error))? {
        let entry = entry.map_err(|error| ConfigError::new(&directory, error))?;
        let name = entry.file_name();
        if name.as_encoded_bytes().starts_with(b".") {
            continue; // Installer staging, not a discoverable plugin name.
        }
        let path = directory.join(&name);
        let kind = entry
            .file_type()
            .map_err(|error| ConfigError::new(&path, error))?;
        if kind == platform::FileKind::File {
            continue;
        }
        if kind != platform::FileKind::Directory {
            return Err(ConfigError::new(
                &path,
                "A plugin must be a real directory, not a link or special file.",
            ));
        }
        let plugin = read(&path)?;
        if name != plugin.info.name.as_str() {
            return Err(ConfigError::new(
                &path,
                "The directory name must match the plugin's type.",
            ));
        }
        if plugins.contains_key(&plugin.info.name) {
            return Err(ConfigError::new(
                &path,
                "Built-in plugin types are reserved.",
            ));
        }
        plugins.insert(plugin.info.name.clone(), Arc::new(plugin));
    }
    Ok(plugins)
}

pub fn list(root: &Path) -> Result<Vec<PluginInfo>, ConfigError> {
    let directory = definitions(root)?;
    catalog(directory.parent().expect("workspace root"))
        .map(|plugins| plugins.values().map(|plugin| plugin.info.clone()).collect())
}

/// Install a complete folder, so readers can never observe a half-written plugin.
pub fn add(root: &Path, source: &Path) -> Result<PluginInfo, ConfigError> {
    let definitions = definitions(root)?;
    let directory = definitions.join(DIRECTORY);
    let source = platform::canonicalize(source).map_err(|error| ConfigError::new(source, error))?;
    let plugin = read(&source)?;
    let installed = directory.join(plugin.info.name.as_str());
    if builtins().any(|builtin| builtin.info.name == plugin.info.name) {
        return Err(ConfigError::new(
            &installed,
            "Built-in plugin types are reserved.",
        ));
    }
    ensure_directory(&definitions, DIRECTORY)
        .map_err(|error| ConfigError::new(&directory, error))?;
    if platform::paths::reusable_directory(&directory, plugin.info.name.as_str())
        .map_err(|error| ConfigError::new(&installed, error))?
    {
        return Err(ConfigError::new(
            &installed,
            "Plugin already installed; remove it before adding a replacement.",
        ));
    }
    let staged = tempfile::Builder::new()
        .prefix(".install-")
        .tempdir_in(&directory)
        .map_err(|error| ConfigError::new(&directory, error))?;
    let payload = staged.path().join("plugin");
    if source.starts_with(&directory) || directory.starts_with(&source) {
        return Err(ConfigError::new(
            &source,
            "The source must be outside the installed plugins directory and cannot contain it.",
        ));
    }
    let source =
        platform::open_directory(&source).map_err(|error| ConfigError::new(&source, error))?;
    copy(&source, &payload).map_err(|error| ConfigError::new(&installed, error))?;
    let validated = read(&payload)?;
    if validated.info.name != plugin.info.name {
        return Err(ConfigError::new(
            &installed,
            "Plugin type changed during installation.",
        ));
    }
    std::fs::rename(&payload, &installed).map_err(|error| ConfigError::new(&installed, error))?;
    Ok(validated.info)
}

fn ensure_directory(parent: &Path, name: &str) -> std::io::Result<()> {
    if !platform::paths::reusable_directory(parent, name)? {
        match std::fs::create_dir(parent.join(name)) {
            Err(error)
                if error.kind() == std::io::ErrorKind::AlreadyExists
                    && platform::paths::reusable_directory(parent, name)? =>
            {
                Ok(())
            }
            result => result,
        }
    } else {
        Ok(())
    }
}

fn copy(source: &File, target: &Path) -> std::io::Result<()> {
    std::fs::create_dir(target)?;
    for entry in platform::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        let path = target.join(&name);
        match entry.file_type()? {
            platform::FileKind::Directory => {
                copy(&platform::paths::open_named(source, &name)?, &path)?
            }
            platform::FileKind::File => {
                let mut input = platform::paths::open_regular_entry(source, &name)?;
                std::io::copy(&mut input, &mut File::create_new(path)?)?;
            }
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Plugin folders cannot contain links or special files.",
                ));
            }
        }
    }
    Ok(())
}

/// Remove an installed package, even if its manifest is no longer valid.
pub fn remove(root: &Path, name: &super::ArtifactType) -> Result<(), ConfigError> {
    let definitions = definitions(root)?;
    let directory = definitions.join(DIRECTORY);
    let path = directory.join(name.as_str());
    if builtins().any(|builtin| builtin.info.name == *name) {
        return Err(ConfigError::new(
            &path,
            "A built-in plugin cannot be removed.",
        ));
    }
    if !platform::paths::reusable_directory(&definitions, DIRECTORY)
        .and_then(|exists| {
            if exists {
                platform::paths::reusable_directory(&directory, name.as_str())
            } else {
                Ok(false)
            }
        })
        .map_err(|error| ConfigError::new(&path, error))?
    {
        return Err(ConfigError::new(&path, "Plugin is not installed."));
    }
    let staged = tempfile::Builder::new()
        .prefix(".remove-")
        .tempdir_in(&directory)
        .map_err(|error| ConfigError::new(&path, error))?;
    std::fs::rename(&path, staged.path().join("plugin"))
        .map_err(|error| ConfigError::new(&path, error))?;
    staged
        .close()
        .map_err(|error| ConfigError::new(&path, error))
}
