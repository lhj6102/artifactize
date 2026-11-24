//! Write real TOML declarations while keeping JSON builders convenient in tests.

use std::{fs, io, path::Path};

use artifactize_core::platform::path_text;
use serde_json::Value;

fn locations()
-> &'static std::sync::Mutex<std::collections::BTreeMap<std::path::PathBuf, std::path::PathBuf>> {
    static FILES: std::sync::OnceLock<
        std::sync::Mutex<std::collections::BTreeMap<std::path::PathBuf, std::path::PathBuf>>,
    > = std::sync::OnceLock::new();
    FILES.get_or_init(Default::default)
}
fn remember(target: &Path, file: &Path) -> io::Result<()> {
    let previous = locations()
        .lock()
        .unwrap()
        .insert(target.to_owned(), file.to_owned());
    if let Some(previous) = previous
        && previous != file
        && previous.exists()
    {
        fs::remove_file(previous)?;
    }
    Ok(())
}

/// Test builders may use an eval array; only the writer turns ids into table keys.
pub fn to_toml(mut value: Value) -> Result<String, String> {
    if let Some(evals) = value.get_mut("evals")
        && let Some(items) = evals.as_array_mut()
    {
        let mut table = serde_json::Map::new();
        for mut eval in std::mem::take(items) {
            let id = eval
                .as_object_mut()
                .and_then(|eval| eval.remove("id"))
                .and_then(|id| id.as_str().map(str::to_owned))
                .ok_or("Test eval requires a string id.")?;
            if table.insert(id.clone(), eval).is_some() {
                return Err(format!(
                    "Duplicate local Eval in {}: {id}",
                    value["name"].as_str().unwrap_or("?")
                ));
            }
        }
        *evals = Value::Object(table);
    }
    toml::to_string_pretty(&value).map_err(|error| error.to_string())
}

/// Read a declaration back into the array-shaped test builder for small mutations.
pub fn read(contents: impl AsRef<[u8]>) -> Result<Value, String> {
    let source = std::str::from_utf8(contents.as_ref()).map_err(|error| error.to_string())?;
    let mut value: Value = toml::from_str(source).map_err(|error| error.to_string())?;
    if let Some(evals) = value.get_mut("evals")
        && let Some(table) = evals.as_object_mut()
    {
        let mut items = Vec::new();
        for (id, mut eval) in std::mem::take(table) {
            eval.as_object_mut()
                .ok_or("Eval must be a table.")?
                .insert("id".into(), Value::String(id));
            items.push(eval);
        }
        *evals = Value::Array(items);
    }
    Ok(value)
}

/// Write one named fixture target. `target` uses a folder's `index.artf` spelling or
/// a file's `<file>.artf` spelling only as a builder shorthand, never as an on-disk
/// declaration. Project roots are explicit. This fixture DSL makes its folder tree
/// into explicit `parts`; tests of independent overlapping targets write definitions directly.
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
    let json = serde_json::from_slice::<Value>(bytes).ok();
    let raw_name = source.lines().enumerate().find_map(|(index, line)| {
        let value = toml::from_str::<Value>(line).ok()?;
        Some((index, value.get("name")?.as_str()?.to_owned()))
    });
    let mut value = json
        .clone()
        .or_else(|| toml::from_str::<Value>(source).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let relative = target.strip_prefix(root).map_err(io::Error::other)?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| raw_name.as_ref().map(|(_, name)| name.clone()))
        .or_else(|| {
            source_path(target)
                .file_stem()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        })
        .ok_or_else(|| io::Error::other("Fixture node requires a name."))?;
    let path = fixture_path(relative, value.get("select").is_some());
    if json.is_none() {
        let file = root.join(".artifactize").join(format!("{name}.artf"));
        fs::create_dir_all(file.parent().unwrap())?;
        remember(target, &file)?;
        if let Some((line, _)) = raw_name {
            let next = source
                .split_inclusive('\n')
                .enumerate()
                .map(|(index, text)| {
                    if index == line {
                        format!(
                            "path = {}{}",
                            serde_json::to_string(&path).unwrap(),
                            if text.ends_with('\n') { "\n" } else { "" }
                        )
                    } else {
                        text.to_owned()
                    }
                })
                .collect::<String>();
            fs::write(file, next)?;
        } else {
            fs::write(file, source)?;
        }
        return compose(root);
    }
    let object = value
        .as_object_mut()
        .ok_or_else(|| io::Error::other("Fixture definition must be an object."))?;
    object.remove("name");
    object.insert("path".into(), Value::String(path));
    let file = root.join(".artifactize").join(format!("{name}.artf"));
    fs::create_dir_all(file.parent().unwrap())?;
    remember(target, &file)?;
    fs::write(file, to_toml(value).map_err(io::Error::other)?)?;
    compose(root)
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

/// Locate a fixture definition by its target, for tests that mutate a previously
/// declared target. No marker file is created or consumed by production discovery.
pub fn source_path(target: impl AsRef<Path>) -> std::path::PathBuf {
    let target = target.as_ref();
    if let Some(file) = locations().lock().unwrap().get(target).cloned() {
        return file;
    }
    if target.exists()
        || !target
            .file_name()
            .is_some_and(|name| name.as_encoded_bytes().ends_with(b".artf"))
    {
        return target.to_owned();
    }
    for root in target.ancestors().skip(1) {
        let directory = root.join(".artifactize");
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        let relative = target.strip_prefix(root).unwrap();
        let expected = fixture_path(relative, false);
        for entry in entries.flatten() {
            let file = entry.path();
            let Ok(source) = fs::read_to_string(&file) else {
                continue;
            };
            let Ok(value) = toml::from_str::<Value>(&source) else {
                continue;
            };
            if value["path"].as_str() == Some(&expected) {
                return file;
            }
        }
    }
    target.to_owned()
}
pub fn read_file(target: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    fs::read(source_path(target))
}
pub fn read_text(target: impl AsRef<Path>) -> io::Result<String> {
    fs::read_to_string(source_path(target))
}
pub fn remove_file(target: impl AsRef<Path>) -> io::Result<()> {
    fs::remove_file(source_path(target))
}

/// Materialize the fixture builder's tree as explicit composition, never as marker files.
fn compose(root: &Path) -> io::Result<()> {
    let mut nodes = Vec::new();
    for entry in fs::read_dir(root.join(".artifactize"))? {
        let file = entry?.path();
        if !file
            .extension()
            .is_some_and(|extension| extension == "artf")
        {
            continue;
        }
        let Ok(source) = fs::read_to_string(&file) else {
            continue;
        };
        let Ok(value) = toml::from_str::<Value>(&source) else {
            continue;
        };
        let Some(path) = value["path"].as_str().map(str::to_owned) else {
            continue;
        };
        let name = file.file_stem().unwrap().to_string_lossy().into_owned();
        let folder =
            path.ends_with('/') && value.get("select").is_none() && path != ".artifactize/";
        nodes.push((file, name, path, folder, value));
    }
    let mut added = std::collections::BTreeMap::<String, Vec<String>>::new();
    for (_, name, path, _, _) in &nodes {
        if path == ".artifactize/" {
            continue;
        }
        let owner = nodes
            .iter()
            .filter(|(_, whole, prefix, folder, _)| {
                *folder && whole != name && (prefix == "./" || path.starts_with(prefix))
            })
            .max_by_key(|(_, _, prefix, _, _)| if prefix == "./" { 0 } else { prefix.len() });
        if let Some((_, owner, _, _, _)) = owner {
            added.entry(owner.clone()).or_default().push(name.clone());
        }
    }
    // This fixture shorthand supplies simple tools for reviews that test another seam.
    // Admission tests write their exact production declarations directly, not through here.
    let mut profiles = std::collections::BTreeMap::<_, Vec<_>>::new();
    for (_, name, _, _, value) in &nodes {
        for eval in value["evals"]
            .as_object()
            .into_iter()
            .flat_map(|evals| evals.values())
        {
            let tool = match eval["profile"]["kind"].as_str() {
                Some("agent") => "agent_tools",
                Some("human") => "human_tools",
                _ => continue,
            };
            profiles.entry(name.clone()).or_default().push(tool);
            let instruction = eval["payload"]["instruction"].as_str().unwrap_or_default();
            for other in artifactize_core::scope::instruction_references(instruction) {
                profiles.entry(other.to_string()).or_default().push(tool);
            }
        }
    }
    for (file, name, _, _, mut value) in nodes {
        let fallback = profiles
            .get(&name)
            .into_iter()
            .flatten()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let mut tools = String::new();
        if value.get("type").is_none() {
            for kind in fallback {
                if value["views"][kind]
                    .as_object()
                    .is_none_or(serde_json::Map::is_empty)
                {
                    tools.push_str(if kind == "agent_tools" {
                        "\n[views.agent_tools.fixture_read]\nbuiltin = 'read'\n"
                    } else {
                        "\n[views.human_tools.fixture_help]\nbuiltin = 'help'\nargs = ['artifactize']\n"
                    });
                }
            }
        }
        let Some(added) = added.get(&name) else {
            if !tools.is_empty() {
                let mut source = fs::read_to_string(&file)?;
                source.push_str(&tools);
                fs::write(file, source)?;
            }
            continue;
        };
        let object = value.as_object_mut().unwrap();
        let parts = object
            .entry("parts")
            .or_insert_with(|| Value::Array(Vec::new()));
        let Some(parts) = parts.as_array_mut() else {
            continue;
        };
        for part in added {
            if !parts.iter().any(|value| value.as_str() == Some(part)) {
                parts.push(Value::String(part.clone()));
            }
        }
        let source = to_toml(value).map_err(io::Error::other)?;
        fs::write(file, source + &tools)?;
    }
    Ok(())
}
