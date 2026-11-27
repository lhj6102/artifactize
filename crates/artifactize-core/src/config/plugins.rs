//! Toolsets that come with artifactize or a plugin. A plugin is a folder with a `plugin.toml`
//! naming its namespace, its `.artf` files declaring that namespace's toolset and any macros,
//! and optionally a viewer page.

use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Serialize};

use super::{ConfigError, LogicalPath, PluginName, discovery::DEFINITION_DIR};
use crate::{platform, scope};

pub(super) const DIRECTORY: &str = "plugins";
const MANIFEST: &str = "plugin.toml";
/// A viewer opens one web document. Bound what a launch copies across IPC.
const DOCUMENT_BYTES: u64 = 8 * 1024 * 1024;
/// A plugin's `.artf` files, bounded like any document a plugin carries.
const MAX_SOURCES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BuiltinViewer {
    Files,
    Markdown,
    Artifactize,
}

impl BuiltinViewer {
    // Kept as text rather than `.artf` files: any `.artf` file marks its folder as a workspace.
    const ALL: [(Self, &'static str, &'static str, &'static str); 3] = [
        (
            Self::Files,
            "files",
            "File Explorer",
            r#"// The built-in File Explorer: an Artifact's files for agents and people.
tool files = {
  agent list = builtin(op = "list")
  agent glob = builtin(op = "glob")
  agent grep = builtin(op = "grep")
  agent read = builtin(op = "read")
  human view = viewer(description = "Open @{artf self} in File Explorer.")
}
"#,
        ),
        (
            Self::Markdown,
            "markdown",
            "Markdown",
            r#"// The built-in Markdown viewer.
tool markdown = {
  agent read = builtin(op = "read")
  human view = viewer(description = "Open @{artf self} in Markdown.")
}
"#,
        ),
        (
            Self::Artifactize,
            "artifactize",
            "Artifactize Viewer",
            r#"// The built-in Artifactize Viewer of the definitions under .artifactize/.
tool artifactize = {
  agent declarations = builtin(op = "declarations", args = [])
  human view = viewer(description = "Open @{artf self} in Artifactize Viewer.")
}
"#,
        ),
    ];
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    namespace: PluginName,
    title: String,
    viewer: Option<WebEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WebEntry {
    entry: LogicalPath,
}

/// A viewer selected by its plugin, never by a path supplied by a web page.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ViewerEntry {
    Builtin { viewer: BuiltinViewer },
    Web { html: Arc<str> },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    #[serde(rename = "namespace")]
    pub name: PluginName,
    pub title: String,
    pub builtin: bool,
    pub viewer: bool,
}

#[derive(Debug)]
pub(crate) struct Plugin {
    pub info: PluginInfo,
    pub viewer: Option<ViewerEntry>,
    /// Its `.artf` files, by their path under `plugins/<namespace>/`.
    pub sources: Vec<(PathBuf, String)>,
}

impl Plugin {
    pub(super) fn source_files(&self) -> impl Iterator<Item = super::lang::SourceFile> + '_ {
        self.sources.iter().map(|(path, text)| super::lang::SourceFile {
            path: PathBuf::from(DIRECTORY).join(self.info.name.as_str()).join(path),
            text: text.clone(),
            origin: super::lang::Origin::Plugin(super::lang::PluginSource {
                namespace: self.info.name.to_string(),
                viewer: self.viewer.is_some(),
            }),
        })
    }
}

pub(crate) type Catalog = BTreeMap<PluginName, Arc<Plugin>>;

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
    BuiltinViewer::ALL.into_iter().map(|(viewer, name, title, source)| Plugin {
        info: PluginInfo {
            name: name.parse().expect("a built-in plugin name"),
            title: title.into(),
            builtin: true,
            viewer: true,
        },
        viewer: Some(ViewerEntry::Builtin { viewer }),
        sources: vec![(PathBuf::from(format!("{name}.artf")), source.into())],
    })
}

fn definitions(root: &Path) -> Result<PathBuf, ConfigError> {
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

/// Read one plugin folder: its manifest, its viewer page and its `.artf` files.
pub(super) fn read(directory: &Path) -> Result<Plugin, ConfigError> {
    let manifest_path = directory.join(MANIFEST);
    let source = scope::open_scoped(directory, MANIFEST)
        .map_err(|error| ConfigError::new(&manifest_path, error))?;
    let source =
        read_document(source).map_err(|error| ConfigError::new(&manifest_path, error))?;
    let table: toml::Table =
        toml::from_str(&source).map_err(|error| ConfigError::new(&manifest_path, error))?;
    if table.contains_key("type") || table.contains_key("views") {
        return Err(ConfigError::new(
            &manifest_path,
            "This plugin.toml is from before 0.27: it names its `namespace` now and declares its tools in an .artf file. Run artifactize config migrate in a project that has it installed, or update the plugin.",
        ));
    }
    let manifest: Manifest =
        toml::from_str(&source).map_err(|error| ConfigError::new(&manifest_path, error))?;
    super::validation::text(&manifest.title, "Plugin title")
        .map_err(|error| ConfigError::new(&manifest_path, error))?;
    let viewer = manifest
        .viewer
        .map(|web| {
            if Path::new(web.entry.as_str())
                .extension()
                .is_none_or(|extension| extension != "html")
            {
                return Err("viewer.entry must name an .html file in the plugin directory.".to_owned());
            }
            let file = scope::open_scoped(directory, web.entry.as_str())
                .map_err(|error| format!("viewer.entry {}: {error}", web.entry))?;
            Ok(ViewerEntry::Web {
                html: read_document(file)?.into(),
            })
        })
        .transpose()
        .map_err(|error| ConfigError::new(&manifest_path, error))?;
    let opened = platform::open_directory(directory)
        .map_err(|error| ConfigError::new(directory, error))?;
    let mut sources = Vec::new();
    for entry in platform::read_dir(&opened).map_err(|error| ConfigError::new(directory, error))? {
        let entry = entry.map_err(|error| ConfigError::new(directory, error))?;
        let name = entry.file_name();
        if !name.as_encoded_bytes().ends_with(b".artf") {
            continue;
        }
        let path = directory.join(&name);
        if entry.file_type().map_err(|error| ConfigError::new(&path, error))?
            != platform::FileKind::File
        {
            return Err(ConfigError::new(&path, "A plugin's definition must be a regular file."));
        }
        let text = super::discovery::read_regular_text(&path)
            .map_err(|error| ConfigError::new(&path, error))?;
        sources.push((PathBuf::from(name), text));
    }
    if sources.len() > MAX_SOURCES {
        return Err(ConfigError::new(directory, format!("A plugin holds at most {MAX_SOURCES} .artf files.")));
    }
    sources.sort();
    if sources.is_empty() {
        return Err(ConfigError::new(
            directory,
            format!("Plugin {} declares no tools: add an .artf file with `tool {} = {{ … }}`.", manifest.namespace, manifest.namespace),
        ));
    }
    Ok(Plugin {
        info: PluginInfo {
            name: manifest.namespace,
            title: manifest.title,
            builtin: false,
            viewer: viewer.is_some(),
        },
        viewer,
        sources,
    })
}

/// The built-in plugins and those installed under `.artifactize/plugins/`.
pub(crate) fn catalog(root: &Path) -> Result<Catalog, ConfigError> {
    let mut plugins: Catalog = builtins()
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
                "The directory name must match the plugin's namespace.",
            ));
        }
        if plugins.contains_key(&plugin.info.name) {
            return Err(ConfigError::new(
                &path,
                "Built-in plugin namespaces are reserved.",
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

/// A plugin's files must read on their own: its toolset and macros, with the built-ins.
fn validate(plugin: &Plugin) -> Result<(), ConfigError> {
    let files = builtins()
        .chain(std::iter::once(Plugin {
            info: plugin.info.clone(),
            viewer: plugin.viewer.clone(),
            sources: plugin.sources.clone(),
        }))
        .flat_map(|plugin| plugin.source_files().collect::<Vec<_>>())
        .collect();
    let program = super::lang::read(files).map_err(|(sources, error)| sources.error(error))?;
    if !program
        .symbols
        .iter()
        .any(|symbol| symbol.kind == "tool" && !symbol.is_macro && symbol.name == plugin.info.name.as_str())
    {
        return Err(ConfigError::new(
            PathBuf::from(plugin.info.name.as_str()),
            format!("Plugin {0} declares no `tool {0} = {{ … }}`.", plugin.info.name),
        ));
    }
    Ok(())
}

/// Install a complete folder, so readers can never observe a half-written plugin.
pub fn add(root: &Path, source: &Path) -> Result<PluginInfo, ConfigError> {
    let definitions = definitions(root)?;
    let directory = definitions.join(DIRECTORY);
    let source = platform::canonicalize(source).map_err(|error| ConfigError::new(source, error))?;
    let plugin = read(&source)?;
    validate(&plugin)?;
    let installed = directory.join(plugin.info.name.as_str());
    if builtins().any(|builtin| builtin.info.name == plugin.info.name) {
        return Err(ConfigError::new(
            &installed,
            "Built-in plugin namespaces are reserved.",
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
    platform::paths::copy_portable_tree(&source, &payload)
        .map_err(|error| ConfigError::new(&installed, error))?;
    let validated = read(&payload)?;
    if validated.info.name != plugin.info.name {
        return Err(ConfigError::new(
            &installed,
            "Plugin namespace changed during installation.",
        ));
    }
    platform::paths::rename_open_file(&payload, &installed)
        .map_err(|error| ConfigError::new(&installed, error))?;
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

/// Remove an installed package, even if its manifest is no longer valid.
pub fn remove(root: &Path, name: &PluginName) -> Result<(), ConfigError> {
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
    let staged = platform::private_tempdir_in(".remove-", &directory)
        .map_err(|error| ConfigError::new(&path, error))?;
    platform::paths::rename_open_file(&path, &staged.path().join("plugin"))
        .map_err(|error| ConfigError::new(&path, error))?;
    staged
        .close()
        .map_err(|error| ConfigError::new(&path, error))
}

/// The plugins a migration reads with: the built-ins, the current ones as installed, and each
/// one installed before 0.27 converted in memory. Returns the files a converted plugin is
/// written as: its new manifest and its toolset.
pub(super) fn migrate(root: &Path) -> Result<(Catalog, Vec<(PathBuf, String)>), ConfigError> {
    let mut plugins: Catalog = builtins()
        .map(|plugin| (plugin.info.name.clone(), Arc::new(plugin)))
        .collect();
    let mut writes = Vec::new();
    let directory = root.join(DEFINITION_DIR).join(DIRECTORY);
    if !platform::paths::reusable_directory(root, DEFINITION_DIR)
        .map_err(|error| ConfigError::new(root.join(DEFINITION_DIR), error))?
        || !platform::paths::reusable_directory(&root.join(DEFINITION_DIR), DIRECTORY)
            .map_err(|error| ConfigError::new(&directory, error))?
    {
        return Ok((plugins, writes));
    }
    let opened = platform::open_directory(&directory)
        .map_err(|error| ConfigError::new(&directory, error))?;
    for entry in platform::read_dir(&opened).map_err(|error| ConfigError::new(&directory, error))? {
        let entry = entry.map_err(|error| ConfigError::new(&directory, error))?;
        let name = entry.file_name();
        let path = directory.join(&name);
        if name.as_encoded_bytes().starts_with(b".")
            || entry.file_type().map_err(|error| ConfigError::new(&path, error))?
                != platform::FileKind::Directory
        {
            continue;
        }
        let manifest_path = path.join(MANIFEST);
        let source = super::discovery::read_regular_text(&manifest_path)
            .map_err(|error| ConfigError::new(&manifest_path, error))?;
        let table: toml::Table =
            toml::from_str(&source).map_err(|error| ConfigError::new(&manifest_path, error))?;
        if !table.contains_key("type") && !table.contains_key("views") {
            let plugin = read(&path)?;
            plugins.insert(plugin.info.name.clone(), Arc::new(plugin));
            continue;
        }
        let invalid = |message: &str| ConfigError::new(&manifest_path, message);
        let namespace: PluginName = table
            .get("type")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| invalid("An old plugin.toml names its `type`."))?
            .parse()
            .map_err(|error: String| invalid(&error))?;
        let title = table
            .get("title")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| invalid("A plugin.toml has a `title`."))?
            .to_owned();
        let entry = table
            .get("viewer")
            .and_then(|viewer| viewer.get("entry"))
            .and_then(toml::Value::as_str)
            .map(str::to_owned);
        let views = serde_json::to_value(table.get("views").cloned().unwrap_or_else(|| toml::Table::new().into()))
            .expect("TOML is JSON");
        let tools = super::convert::toolset_text(namespace.as_str(), &views, entry.is_some())
            .map_err(|error| invalid(&error))?;
        let mut manifest = toml::Table::new();
        manifest.insert("namespace".into(), namespace.to_string().into());
        manifest.insert("title".into(), title.clone().into());
        if let Some(entry) = &entry {
            let mut viewer = toml::Table::new();
            viewer.insert("entry".into(), entry.clone().into());
            manifest.insert("viewer".into(), viewer.into());
        }
        let viewer = entry
            .map(|entry| {
                let file = scope::open_scoped(&path, &entry)
                    .map_err(|error| invalid(&format!("viewer.entry {entry}: {error}")))?;
                Ok::<_, ConfigError>(ViewerEntry::Web {
                    html: read_document(file).map_err(|error| invalid(&error))?.into(),
                })
            })
            .transpose()?;
        let file = PathBuf::from(format!("{namespace}.artf"));
        let plugin = Plugin {
            info: PluginInfo {
                name: namespace.clone(),
                title,
                builtin: false,
                viewer: viewer.is_some(),
            },
            viewer,
            sources: vec![(file.clone(), tools.clone())],
        };
        validate(&plugin)?;
        writes.push((
            manifest_path.clone(),
            toml::to_string(&manifest).expect("a manifest is TOML"),
        ));
        writes.push((path.join(file), tools));
        plugins.insert(namespace, Arc::new(plugin));
    }
    Ok((plugins, writes))
}
