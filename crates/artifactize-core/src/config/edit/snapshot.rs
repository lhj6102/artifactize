//! Project snapshots and pure projections of their layout and edit footprint.

use super::*;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Layout {
    pub nodes: BTreeMap<PlacementKey, [f64; 2]>,
}
/// Branch-current places. The top level is hand-placed when a plain node key exists;
/// a box is hand-placed when a `whole/part` key exists. No layout file is read.
pub fn read_layout(project: &Path) -> Result<Layout, Refusal> {
    let read = read_project(project)?;
    let mut layout = Layout::default();
    for (name, node) in read.nodes {
        if let Some(at) = node.annotations.at {
            layout.nodes.insert(PlacementKey::Node(name.clone()), at);
        }
        for (part, at) in node.annotations.parts {
            layout.nodes.insert(
                PlacementKey::Part {
                    whole: name.clone(),
                    part,
                },
                at,
            );
        }
    }
    Ok(layout)
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    #[serde(with = "crate::platform::path_serde")]
    pub definition: PathBuf,
    pub identity: crate::types::Sha256Digest,
    /// Authored PATH, including its kind-bearing trailing slash or URI.
    pub target: String,
    #[serde(with = "crate::platform::path_serde")]
    pub path: PathBuf,
    pub kind: ArtifactKind,
    pub target_missing: bool,
    pub annotations: Annotations,
}
#[derive(Debug, Serialize)]
pub struct Project {
    #[serde(skip)]
    root: PathBuf,
    pub files: BasedOn,
    pub nodes: BTreeMap<ArtifactName, Node>,
}
/// Definition files a proposed operation changes, with their snapshot bases. A new
/// destination has no current text. Paths are project-relative, exactly as in `BasedOn`.
#[derive(Debug, Clone, Serialize)]
pub struct Footprint {
    #[serde(with = "crate::platform::path_serde::map")]
    pub files: BTreeMap<PathBuf, Option<String>>,
}

/// Compute the exact write set from a `read_project` snapshot without writing files,
/// acquiring edit locks or creating state. This is the same candidate edit as `apply`.
/// Use its files as `BasedOn` and to guard unsaved Source drafts; destinations and
/// reference rewrites belong to the library. Disk edits after the snapshot are still
/// detected by `apply`'s compare-and-swap, never silently adopted as fresh bases.
pub fn footprint(project: &Project, operation: &Operation) -> Result<Footprint, Refusal> {
    let sources = project
        .files
        .iter()
        .filter_map(|(file, source)| {
            source
                .as_ref()
                .map(|source| (project.root.join(file), source.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let proposed = proposal::propose(&project.root, &sources, operation.clone())?;
    discovery::read_sources(project.root.clone(), &proposed)?;
    let files = proposal::changes(&sources, &proposed)
        .into_keys()
        .map(|file| {
            let base = sources.get(&file).cloned();
            (
                file.strip_prefix(&project.root)
                    .expect("project definition")
                    .to_owned(),
                base,
            )
        })
        .collect();
    Ok(Footprint { files })
}

/// A file's exact content is its optimistic base; `identity` is SHA-256 for view clients.
/// Invalid annotation values are ignored for display and described in their error fields.
pub fn read_project(project: &Path) -> Result<Project, Refusal> {
    let root = crate::platform::canonicalize(project).map_err(io)?;
    let sources = discovery::sources(&root)?;
    let mut config = discovery::read_sources(root.clone(), &sources)?;
    discovery::inspect_targets(&mut config);
    let files = sources
        .into_iter()
        .map(|(file, source)| {
            (
                file.strip_prefix(&root)
                    .expect("definition below project")
                    .to_owned(),
                Some(source),
            )
        })
        .collect::<BasedOn>();
    let nodes = config
        .artifacts
        .iter()
        .map(|(name, artifact)| {
            let definition = artifact.declaration_path();
            let source = files[&definition].as_deref().expect("definition text");
            let identity = crate::types::Sha256Digest::of(source.as_bytes());
            (
                name.clone(),
                Node {
                    definition,
                    identity,
                    target: artifact
                        .authored_path
                        .clone()
                        .expect("parsed definition PATH"),
                    path: artifact.path.local().unwrap_or(Path::new("")).to_owned(),
                    kind: artifact.kind(),
                    target_missing: artifact.target_error.is_some(),
                    annotations: annotations::read(source, &config, name),
                },
            )
        })
        .collect();
    Ok(Project { root, files, nodes })
}
