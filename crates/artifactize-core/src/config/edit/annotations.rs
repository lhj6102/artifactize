//! Lossless writes to annotations. Only the known keys of the section being written
//! are validated; unrelated and future data is never reserialized or discarded.
use super::*;
use toml_edit::{DocumentMut, Item, Table, TableLike};

pub type Point = [f64; 2];
#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotations {
    pub at: Option<Point>,
    pub parts: BTreeMap<ArtifactName, Point>,
    pub comment: Option<String>,
    pub style_error: Option<String>,
    pub comment_error: Option<String>,
}
fn section<'a>(doc: &'a DocumentMut, key: &str) -> Result<Option<&'a dyn TableLike>, String> {
    let Some(annotations) = doc.get("annotations") else {
        return Ok(None);
    };
    let annotations = annotations
        .as_table_like()
        .ok_or("annotations must be a table")?;
    annotations
        .get(key)
        .map(|item| {
            item.as_table_like()
                .ok_or_else(|| format!("annotations.{key} must be a table"))
        })
        .transpose()
}
fn point(item: &Item) -> Result<Point, String> {
    let array = item.as_array().ok_or("a place must be [x, y]")?;
    if array.len() != 2 {
        return Err("a place must have two coordinates".into());
    }
    let number = |index| {
        array
            .get(index)
            .and_then(|value| {
                value
                    .as_float()
                    .or_else(|| value.as_integer().map(|number| number as f64))
            })
            .filter(|number| number.is_finite())
            .ok_or_else(|| "coordinates must be finite numbers".to_owned())
    };
    Ok([number(0)?, number(1)?])
}
fn top_level(config: &RepoConfig, name: &ArtifactName) -> bool {
    !config
        .artifacts
        .values()
        .any(|artifact| artifact.parts.contains(name))
}
fn style(
    doc: &DocumentMut,
    config: &RepoConfig,
    name: &ArtifactName,
) -> Result<(Option<Point>, BTreeMap<ArtifactName, Point>), String> {
    let Some(style) = section(doc, "style")? else {
        return Ok((None, BTreeMap::new()));
    };
    let at = if top_level(config, name) {
        style.get("at").map(point).transpose()?
    } else {
        None
    };
    let parts = match style.get("parts") {
        None => BTreeMap::new(),
        Some(parts) => parts
            .as_table_like()
            .ok_or("annotations.style.parts must be a table")?
            .iter()
            .filter(|(part, _)| {
                config.artifacts[name]
                    .parts
                    .iter()
                    .any(|name| name.as_str() == *part)
            })
            .map(|(part, value)| Ok((part.parse().expect("configured part"), point(value)?)))
            .collect::<Result<_, String>>()?,
    };
    Ok((at, parts))
}
fn comment(doc: &DocumentMut) -> Result<Option<String>, String> {
    section(doc, "comment")?
        .and_then(|section| section.get("text"))
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| "annotations.comment.text must be a string".to_owned())
        })
        .transpose()
        .map(|text| text.filter(|text| !text.is_empty()))
}
pub(super) fn read(source: &str, config: &RepoConfig, name: &ArtifactName) -> Annotations {
    let doc = source.parse::<DocumentMut>().expect("validated TOML");
    let mut annotations = Annotations::default();
    match style(&doc, config, name) {
        Ok((at, parts)) => {
            annotations.at = at;
            annotations.parts = parts;
        }
        Err(error) => annotations.style_error = Some(error),
    }
    match comment(&doc) {
        Ok(text) => annotations.comment = text,
        Err(error) => annotations.comment_error = Some(error),
    }
    annotations
}
fn invalid(doc: &DocumentMut, file: &Path, section: &str, message: String) -> Refusal {
    let source = doc.to_string();
    ConfigError::new(
        file,
        location::error_at(&source, &["annotations", section], &message),
    )
    .into()
}
fn section_mut<'a>(doc: &'a mut DocumentMut, key: &str) -> &'a mut dyn TableLike {
    if doc.get("annotations").is_none() {
        let mut table = Table::new();
        table.set_implicit(true);
        doc["annotations"] = Item::Table(table);
    }
    let annotations = doc["annotations"]
        .as_table_like_mut()
        .expect("validated annotation table");
    if annotations.get(key).is_none() {
        annotations.insert(key, Item::Table(Table::new()));
    }
    annotations
        .get_mut(key)
        .expect("section inserted")
        .as_table_like_mut()
        .expect("validated section")
}
fn replace(table: &mut dyn TableLike, key: &str, mut item: Item) {
    if let Some(old) = table.get(key).and_then(Item::as_value)
        && let Some(value) = item.as_value_mut()
    {
        *value.decor_mut() = old.decor().clone();
    }
    table.insert(key, item);
}
fn point_value(point: Point) -> Item {
    toml_edit::value(point.into_iter().collect::<toml_edit::Array>())
}
fn cleanup(doc: &mut DocumentMut, section: &str) {
    let Some(annotations) = doc.get_mut("annotations").and_then(Item::as_table_like_mut) else {
        return;
    };
    if annotations
        .get(section)
        .and_then(Item::as_table_like)
        .is_some_and(|table| table.is_empty())
    {
        annotations.remove(section);
    }
    if annotations.is_empty() {
        doc.remove("annotations");
    }
}
fn prune_style(doc: &mut DocumentMut, config: &RepoConfig, name: &ArtifactName) {
    let Some(style) = doc
        .get_mut("annotations")
        .and_then(|annotations| annotations.get_mut("style"))
        .and_then(Item::as_table_like_mut)
    else {
        return;
    };
    if !top_level(config, name) {
        style.remove("at");
    }
    if let Some(parts) = style.get_mut("parts").and_then(Item::as_table_like_mut) {
        let stale = parts
            .iter()
            .filter(|(part, _)| {
                !config.artifacts[name]
                    .parts
                    .iter()
                    .any(|name| name.as_str() == *part)
            })
            .map(|(part, _)| part.to_owned())
            .collect::<Vec<_>>();
        for part in stale {
            parts.remove(&part);
        }
        parts.sort_values();
        if parts.is_empty() {
            style.remove("parts");
        }
    }
    cleanup(doc, "style");
}
pub(super) fn validate_style(
    doc: &DocumentMut,
    file: &Path,
    config: &RepoConfig,
    name: &ArtifactName,
) -> Result<(), Refusal> {
    style(doc, config, name)
        .map(|_| ())
        .map_err(|error| invalid(doc, file, "style", error))
}
pub(super) fn set_at(
    doc: &mut DocumentMut,
    file: &Path,
    config: &RepoConfig,
    name: &ArtifactName,
    at: Option<Point>,
) -> Result<(), Refusal> {
    style(doc, config, name).map_err(|error| invalid(doc, file, "style", error))?;
    if let Some(at) = at {
        if at.iter().any(|number| !number.is_finite()) {
            return Err(forbidden("Places must be finite coordinates."));
        }
        if top_level(config, name) {
            replace(section_mut(doc, "style"), "at", point_value(at));
        }
    } else if let Some(style) = doc
        .get_mut("annotations")
        .and_then(|annotations| annotations.get_mut("style"))
        .and_then(Item::as_table_like_mut)
    {
        style.remove("at");
    }
    prune_style(doc, config, name);
    Ok(())
}
pub(super) fn set_parts(
    doc: &mut DocumentMut,
    file: &Path,
    config: &RepoConfig,
    name: &ArtifactName,
    places: &BTreeMap<ArtifactName, Point>,
    reset: bool,
) -> Result<(), Refusal> {
    style(doc, config, name).map_err(|error| invalid(doc, file, "style", error))?;
    if places.values().flatten().any(|number| !number.is_finite()) {
        return Err(forbidden("Places must be finite coordinates."));
    }
    if reset {
        if let Some(style) = doc
            .get_mut("annotations")
            .and_then(|annotations| annotations.get_mut("style"))
            .and_then(Item::as_table_like_mut)
        {
            style.remove("parts");
        }
    } else if !places.is_empty() {
        let style = section_mut(doc, "style");
        if style.get("parts").is_none() {
            style.insert("parts", Item::Table(Table::new()));
        }
        let parts = style
            .get_mut("parts")
            .expect("inserted parts")
            .as_table_like_mut()
            .expect("validated parts");
        for (part, place) in places {
            if config.artifacts[name].parts.contains(part) {
                replace(parts, part, point_value(*place));
            }
        }
    }
    prune_style(doc, config, name);
    Ok(())
}
pub(super) fn set_comment(doc: &mut DocumentMut, file: &Path, text: &str) -> Result<(), Refusal> {
    comment(doc).map_err(|error| invalid(doc, file, "comment", error))?;
    if text.is_empty() {
        if let Some(comment) = doc
            .get_mut("annotations")
            .and_then(|annotations| annotations.get_mut("comment"))
            .and_then(Item::as_table_like_mut)
        {
            comment.remove("text");
        }
    } else {
        replace(section_mut(doc, "comment"), "text", toml_edit::value(text));
    }
    cleanup(doc, "comment");
    Ok(())
}
pub(super) fn take_comment(doc: &DocumentMut, file: &Path) -> Result<Option<String>, Refusal> {
    comment(doc).map_err(|error| invalid(doc, file, "comment", error))
}
pub(super) fn rename_part(
    doc: &mut DocumentMut,
    file: &Path,
    before: &RepoConfig,
    owner: &ArtifactName,
    old: &ArtifactName,
    new: &ArtifactName,
) -> Result<(), Refusal> {
    let (_, places) =
        style(doc, before, owner).map_err(|error| invalid(doc, file, "style", error))?;
    if !places.contains_key(old) {
        return Ok(());
    }
    let parts = section_mut(doc, "style")
        .get_mut("parts")
        .expect("placed part")
        .as_table_like_mut()
        .expect("validated parts");
    let decoration = parts.key(old).map(|key| key.leaf_decor().clone());
    let item = parts.remove(old).expect("placed part");
    parts.insert(new, item);
    if let Some(decoration) = decoration {
        *parts.key_mut(new).expect("new key").leaf_decor_mut() = decoration;
    }
    parts.sort_values();
    Ok(())
}
