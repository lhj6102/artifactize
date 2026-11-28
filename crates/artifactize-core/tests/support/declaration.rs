//! Keep JSON declaration builders convenient in tests while writing real `.artf` definitions.
//!
//! A builder is the table a declaration had before 0.27 (`path`, `evals`, `views`, …), with a
//! `name`; the helper converts every builder of a project together, as `config migrate` does,
//! so their Agent profiles are declared once. Each written file keeps its builder in a
//! `// fixture` comment, which [`read`] returns for small mutations.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use artifactize_core::{config::convert, platform::path_text};
use serde_json::Value;

const MARKER: &str = "// fixture ";

/// A builder written through [`write`]: its table, and the text last written for it.
struct Fixture {
    name: String,
    table: Value,
    written: Option<String>,
}

#[derive(Default)]
struct Registry {
    /// Each fixture target's definition file.
    locations: BTreeMap<PathBuf, PathBuf>,
    /// Each project's fixtures, by definition file.
    projects: BTreeMap<PathBuf, BTreeMap<PathBuf, Fixture>>,
    /// Shared files written for a project (profiles, shared tools).
    shared: BTreeMap<PathBuf, BTreeSet<PathBuf>>,
}

fn registry() -> std::sync::MutexGuard<'static, Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// One builder as `.artf` text on its own: its Artifact with its tools. An Agent eval's profile
/// is in the `.artfprofiles` file [`to_files`] adds. Written through [`write`], it joins its
/// project's other builders instead.
pub fn to_artf(value: Value) -> Result<String, String> {
    Ok(to_files(value)?.remove(0).1)
}

/// One builder as the files of a project of its own, relative to `.artifactize/`: its `.artf`
/// file first, then the `.artfprofiles` file of its Agent evals' profiles, if any.
pub fn to_files(value: Value) -> Result<Vec<(PathBuf, String)>, String> {
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .ok_or("A builder names its Artifact.")?
        .to_owned();
    let mut table = value;
    let object = table.as_object_mut().ok_or("A builder is an object.")?;
    object.remove("name");
    // A builder without a target reviews the project folder.
    object
        .entry("path")
        .or_insert_with(|| Value::String("./".into()));
    let kinds: BTreeSet<&'static str> = evals(&table)
        .iter()
        .filter_map(|eval| review_kind(eval))
        .collect();
    let fallback = fallback_tools(&table, &kinds);
    let converted = convert::convert(
        &[convert::Old {
            file: PathBuf::from(format!("{name}.artf")),
            name: name.clone(),
            table: table.clone(),
            comments: Vec::new(),
        }],
        &BTreeMap::new(),
        &BTreeSet::new(),
    )?;
    let mut text = marker(&name, &table);
    let mut profiles = Vec::new();
    // Its own file first, then its toolsets; profiles go in a file of their own.
    for (index, (file, contents)) in converted.files.into_iter().enumerate() {
        if index == 0 {
            text.push_str(&composing(&contents, &name, &fallback));
        } else if file
            .extension()
            .is_some_and(|extension| extension == "artfprofiles")
        {
            profiles.push((file, contents));
        } else {
            text.push_str(&contents);
        }
    }
    if !fallback.is_empty() {
        text.push_str(FIXTURE_TOOLS);
    }
    let mut files = vec![(PathBuf::from(format!("{name}.artf")), text)];
    files.extend(profiles);
    Ok(files)
}

fn marker(name: &str, table: &Value) -> String {
    let mut builder = table.clone();
    if let Some(object) = builder.as_object_mut() {
        object.insert("name".into(), Value::String(name.to_owned()));
    }
    format!(
        "{MARKER}{}\n",
        serde_json::to_string(&builder).expect("a builder is JSON")
    )
}

/// The builder a fixture file was written from, without its `name`.
pub fn read(contents: impl AsRef<[u8]>) -> Result<Value, String> {
    let source = std::str::from_utf8(contents.as_ref()).map_err(|error| error.to_string())?;
    let line = source
        .lines()
        .find_map(|line| line.strip_prefix(MARKER))
        .ok_or("This definition was not written from a builder.")?;
    let mut value: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
    if let Some(object) = value.as_object_mut() {
        object.remove("name");
        // Builders list their evals; a table by id reads back as that list.
        if let Some(Value::Object(table)) = object.get("evals").cloned() {
            let list = table
                .into_iter()
                .map(|(id, mut eval)| {
                    if let Some(eval) = eval.as_object_mut() {
                        eval.insert("id".into(), Value::String(id));
                    }
                    eval
                })
                .collect();
            object.insert("evals".into(), Value::Array(list));
        }
    }
    Ok(value)
}

/// Write one named fixture target. `target` uses a folder's `index.artf` spelling or a
/// file's `<file>.artf` spelling only as a builder shorthand, never as an on-disk definition.
/// Project roots are explicit. The folder tree of a project's targets becomes explicit
/// `parts`; tests of independent overlapping targets write definitions directly.
pub fn write(
    root: impl AsRef<Path>,
    target: impl AsRef<Path>,
    contents: impl AsRef<[u8]>,
) -> io::Result<()> {
    let root = root.as_ref();
    let target = target.as_ref();
    if !target
        .file_name()
        .is_some_and(|name| name.as_encoded_bytes().ends_with(b".artf"))
    {
        return fs::write(target, contents);
    }
    let bytes = contents.as_ref();
    let source = std::str::from_utf8(bytes).map_err(io::Error::other)?;
    let relative = target.strip_prefix(root).map_err(io::Error::other)?;
    let stem = target
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_owned);
    let mut registry = registry();
    let previous = registry.locations.get(target).cloned();
    let marked = source
        .lines()
        .find_map(|line| line.strip_prefix(MARKER))
        .and_then(|line| serde_json::from_str::<Value>(line).ok());
    let unchanged = previous.as_ref().is_some_and(|file| {
        registry
            .projects
            .get(root)
            .and_then(|project| project.get(file))
            .is_some_and(|fixture| fixture.written.as_deref() == Some(source))
    });
    let builder = match (serde_json::from_slice::<Value>(bytes).ok(), marked) {
        (Some(json), _) => Some(json),
        // Text this helper wrote, unchanged, is still its builder.
        (None, Some(marked))
            if unchanged || to_artf(marked.clone()).ok().as_deref() == Some(source) =>
        {
            Some(marked)
        }
        // A test edited the text: it is written as it is, its builder kept for the project's
        // shared profiles and parts.
        (None, Some(mut marked)) => {
            let name = marked["name"].as_str().map(str::to_owned).or(stem.clone());
            let name = name.ok_or_else(|| io::Error::other("Fixture node requires a name."))?;
            let file = previous
                .clone()
                .unwrap_or_else(|| root.join(".artifactize").join(format!("{name}.artf")));
            fs::create_dir_all(file.parent().unwrap())?;
            fs::write(&file, source)?;
            if let Some(object) = marked.as_object_mut() {
                object.remove("name");
            }
            registry.locations.insert(target.to_owned(), file.clone());
            registry
                .projects
                .entry(root.to_owned())
                .or_default()
                .insert(
                    file,
                    Fixture {
                        name,
                        table: marked,
                        written: None,
                    },
                );
            return regenerate(&mut registry, root);
        }
        (None, None) => toml::from_str::<Value>(source)
            .ok()
            .filter(Value::is_object),
    };
    let Some(mut table) = builder else {
        // Not a builder: written as it is, under its target's name.
        let file = previous.unwrap_or_else(|| {
            root.join(".artifactize")
                .join(format!("{}.artf", stem.clone().unwrap_or_default()))
        });
        fs::create_dir_all(file.parent().unwrap())?;
        if let Some(project) = registry.projects.get_mut(root) {
            project.remove(&file);
        }
        registry.locations.insert(target.to_owned(), file.clone());
        fs::write(&file, source)?;
        return regenerate(&mut registry, root);
    };
    let object = table
        .as_object_mut()
        .ok_or_else(|| io::Error::other("Fixture definition must be an object."))?;
    let name = object
        .remove("name")
        .and_then(|name| name.as_str().map(str::to_owned))
        .or_else(|| {
            previous
                .as_ref()
                .and_then(|file| file.file_stem()?.to_str().map(str::to_owned))
        })
        .or(stem)
        .ok_or_else(|| io::Error::other("Fixture node requires a name."))?;
    let folder_target = object.contains_key("select");
    // A builder written into `.artifactize/` keeps the target it names; elsewhere its place is
    // its target, as an old `.artf` beside it was.
    let named = relative.starts_with(".artifactize") && object.contains_key("path");
    if !named {
        object.insert(
            "path".into(),
            Value::String(fixture_path(relative, folder_target)),
        );
    }
    let file = root.join(".artifactize").join(format!("{name}.artf"));
    if let Some(previous) = previous.filter(|previous| *previous != file) {
        if let Some(project) = registry.projects.get_mut(root) {
            project.remove(&previous);
        }
        if previous.exists() {
            fs::remove_file(previous)?;
        }
    }
    registry.locations.insert(target.to_owned(), file.clone());
    let project = registry.projects.entry(root.to_owned()).or_default();
    let written = project.remove(&file).and_then(|fixture| fixture.written);
    project.insert(
        file,
        Fixture {
            name,
            table,
            written,
        },
    );
    regenerate(&mut registry, root)
}

fn fixture_path(relative: &Path, folder_target: bool) -> String {
    if relative == Path::new(".artf") {
        ".artifactize/".to_owned()
    } else if folder_target
        || relative
            .file_name()
            .is_some_and(|file| file == "index.artf")
    {
        let folder = relative.parent().unwrap_or(Path::new(""));
        if folder.as_os_str().is_empty() {
            "./".to_owned()
        } else {
            format!("{}/", path_text(folder))
        }
    } else {
        path_text(relative)
            .strip_suffix(".artf")
            .expect("fixture definition has an .artf suffix")
            .to_owned()
    }
}

/// Locate a fixture definition by its target, for tests that change a declared target.
pub fn source_path(target: impl AsRef<Path>) -> PathBuf {
    let target = target.as_ref();
    registry()
        .locations
        .get(target)
        .cloned()
        .unwrap_or_else(|| target.to_owned())
}
pub fn read_file(target: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    fs::read(source_path(target))
}
pub fn read_text(target: impl AsRef<Path>) -> io::Result<String> {
    fs::read_to_string(source_path(target))
}
pub fn remove_file(target: impl AsRef<Path>) -> io::Result<()> {
    let target = target.as_ref();
    let mut registry = registry();
    let file = registry
        .locations
        .remove(target)
        .unwrap_or_else(|| target.to_owned());
    let root = registry
        .projects
        .iter_mut()
        .find_map(|(root, project)| project.remove(&file).map(|_| root.clone()));
    fs::remove_file(&file)?;
    match root {
        Some(root) => regenerate(&mut registry, &root),
        None => Ok(()),
    }
}

/// Write every builder of `root` again: its folder tree as `parts`, fallback tools for the
/// Artifacts its reviews read, and one declaration of each profile. A file a test changed by
/// hand since it was written stays as the test left it.
fn regenerate(registry: &mut Registry, root: &Path) -> io::Result<()> {
    let Some(project) = registry.projects.get_mut(root) else {
        return Ok(());
    };
    let nodes: Vec<(PathBuf, String, String, bool)> = project
        .iter()
        .map(|(file, fixture)| {
            let path = fixture.table["path"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let folder = path.ends_with('/')
                && fixture.table.get("select").is_none()
                && path != ".artifactize/";
            (file.clone(), fixture.name.clone(), path, folder)
        })
        .collect();
    let mut added = BTreeMap::<String, Vec<String>>::new();
    for (_, name, path, _) in &nodes {
        if path == ".artifactize/" {
            continue;
        }
        let owner = nodes
            .iter()
            .filter(|(_, whole, prefix, folder)| {
                *folder && whole != name && (prefix == "./" || path.starts_with(prefix.as_str()))
            })
            .max_by_key(|(_, _, prefix, _)| if prefix == "./" { 0 } else { prefix.len() });
        if let Some((_, owner, _, _)) = owner {
            added.entry(owner.clone()).or_default().push(name.clone());
        }
    }
    // Reviews need tools: an Artifact an Agent or Human eval reads composes the File
    // Explorer unless it declares a plugin type or tools of its own.
    let mut needs_tools = BTreeMap::<String, BTreeSet<&'static str>>::new();
    for fixture in project.values() {
        for eval in evals(&fixture.table) {
            let Some(kind) = review_kind(eval) else {
                continue;
            };
            let instruction = eval["payload"]["instruction"].as_str().unwrap_or_default();
            for name in
                std::iter::once(fixture.name.clone()).chain(convert::references(instruction))
            {
                needs_tools.entry(name).or_default().insert(kind);
            }
        }
    }
    let mut olds = Vec::new();
    let mut compositions: BTreeMap<String, Vec<&'static str>> = BTreeMap::new();
    for (file, fixture) in project.iter() {
        let mut table = fixture.table.clone();
        if let Some(parts) = added.get(&fixture.name) {
            let list = table
                .as_object_mut()
                .unwrap()
                .entry("parts")
                .or_insert_with(|| Value::Array(Vec::new()));
            if let Some(list) = list.as_array_mut() {
                for part in parts {
                    if !list.iter().any(|value| value.as_str() == Some(part)) {
                        list.push(Value::String(part.clone()));
                    }
                }
            }
        }
        let fallback = fallback_tools(
            &table,
            &needs_tools.get(&fixture.name).cloned().unwrap_or_default(),
        );
        if !fallback.is_empty() {
            compositions.insert(fixture.name.clone(), fallback);
        }
        olds.push(convert::Old {
            file: file
                .strip_prefix(root.join(".artifactize"))
                .unwrap_or(file)
                .to_owned(),
            name: fixture.name.clone(),
            table,
            comments: Vec::new(),
        });
    }
    let converted =
        convert::convert(&olds, &BTreeMap::new(), &BTreeSet::new()).map_err(io::Error::other)?;
    let directory = root.join(".artifactize");
    fs::create_dir_all(&directory)?;
    let mut shared = BTreeSet::new();
    let mut files = converted.files;
    if !compositions.is_empty() {
        let mut name = PathBuf::from("fixture-tools.artf");
        while files.iter().any(|(file, _)| *file == name) {
            name = PathBuf::from(format!("_{}", name.display()));
        }
        files.push((name, FIXTURE_TOOLS.to_owned()));
    }
    for (relative, text) in files {
        let file = directory.join(&relative);
        match project.get_mut(&file) {
            Some(fixture) => {
                let current = fs::read_to_string(&file).ok();
                if current.is_some() && current != fixture.written {
                    continue;
                }
                let text = composing(
                    &text,
                    &fixture.name,
                    compositions
                        .get(&fixture.name)
                        .map_or(&[][..], Vec::as_slice),
                );
                let text = marker(&fixture.name, &fixture.table) + &text;
                fs::write(&file, &text)?;
                fixture.written = Some(text);
            }
            None => {
                fs::write(&file, text)?;
                shared.insert(file);
            }
        }
    }
    let stale = registry
        .shared
        .insert(root.to_owned(), shared.clone())
        .unwrap_or_default();
    for file in stale.difference(&shared) {
        if file.exists() {
            fs::remove_file(file)?;
        }
    }
    Ok(())
}

/// Simple tools for reviews that test another seam, so a provider sees
/// `fixture_read_<artifact>` and a person `fixture-human.help`. Admission tests write their exact
/// production declarations directly, not through here.
const FIXTURE_TOOLS: &str = r#"tool fixture = { agent read = builtin(op = "read") }
tool fixture-human = { human help = builtin(op = "help", args = ["artifactize"]) }
"#;

/// The fixture toolsets an Artifact a review reads composes: one for each kind of review it
/// has no tools for, unless it declares a plugin type.
fn fallback_tools(table: &Value, kinds: &BTreeSet<&'static str>) -> Vec<&'static str> {
    if table.get("type").is_some() {
        return Vec::new();
    }
    kinds
        .iter()
        .filter(|kind| {
            table["views"][format!("{kind}_tools")]
                .as_object()
                .is_none_or(serde_json::Map::is_empty)
        })
        .map(|kind| {
            if *kind == "agent" {
                "fixture"
            } else {
                "fixture-human"
            }
        })
        .collect()
}

/// `text` with `toolsets` added to the composition of Artifact `name`.
fn composing(text: &str, name: &str, toolsets: &[&str]) -> String {
    if toolsets.is_empty() {
        return text.to_owned();
    }
    let start = text
        .find(&format!("artf {name} "))
        .expect("the converted file binds its Artifact");
    let equals = start + text[start..].find(" = ").expect("a binding has `=`");
    let joined = toolsets.join(" & ");
    let added = if text[start..equals].contains(" : ") {
        format!(" & {joined}")
    } else {
        format!(" : {joined}")
    };
    format!("{}{added}{}", &text[..equals], &text[equals..])
}

/// `agent` or `human` for an eval that reviews with tools.
fn review_kind(eval: &Value) -> Option<&'static str> {
    match eval["profile"]["kind"].as_str() {
        Some("agent") => Some("agent"),
        Some("human") => Some("human"),
        _ => None,
    }
}

fn evals(table: &Value) -> Vec<&Value> {
    match table.get("evals") {
        Some(Value::Array(list)) => list.iter().collect(),
        Some(Value::Object(table)) => table.values().collect(),
        _ => Vec::new(),
    }
}
