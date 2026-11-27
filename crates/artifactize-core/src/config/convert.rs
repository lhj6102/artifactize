//! Declarations from before 0.27, TOML tables read as JSON, written in the `.artf` language:
//! the last step of `artifactize config migrate`, and how test fixtures keep their builders.
//!
//! Each old declaration becomes one `artf`; its own tools become a `tool` named after it
//! (shared by every Artifact that declared the same ones), its plugin type a toolset it
//! composes, and every Agent profile a named `profile`. What the language no longer has, such
//! as Human result schemas, is dropped with a warning.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
    path::PathBuf,
};

use serde_json::{Map, Value};

use super::lang::render;

/// One old declaration: the file it was in, relative to `.artifactize/`, its Artifact's name and
/// its table, as `toml` reads it into JSON. `evals` may be a table by id or a list with ids.
#[derive(Debug, Clone)]
pub struct Old {
    pub file: PathBuf,
    pub name: String,
    pub table: Value,
    /// Its comments, each line's text, kept as `//` comments above its `artf`.
    pub comments: Vec<String>,
}

/// The new definition files, relative to `.artifactize/`, and what did not carry over.
#[derive(Debug, Default)]
pub struct Converted {
    pub files: Vec<(PathBuf, String)>,
    pub warnings: Vec<String>,
}

/// The toolsets every project has, whose names a converted toolset may not take.
const BUILTIN_TOOLSETS: [&str; 3] = ["files", "markdown", "artifactize"];
/// Identifiers that mean something where a profile or toolset is named.
const RESERVED: [&str; 7] = ["self", "none", "true", "false", "human", "runtime", "agent"];

/// Convert every old declaration of a project together, so profiles and shared toolsets are
/// declared once. `profiles` are named Agent profiles to declare first, such as the variants
/// of an old `settings.toml`; `plugins` are the installed plugins' namespaces.
pub fn convert(
    declarations: &[Old],
    profiles: &BTreeMap<String, Value>,
    plugins: &BTreeSet<String>,
) -> Result<Converted, String> {
    let mut converter = Converter {
        profiles: Vec::new(),
        toolsets: Vec::new(),
        plugins,
        warnings: Vec::new(),
        artifacts: declarations.iter().map(|old| old.name.clone()).collect(),
    };
    for (name, profile) in profiles {
        converter.profile_name(Some(name), profile)?;
    }
    let artifacts: BTreeSet<&str> = declarations.iter().map(|old| old.name.as_str()).collect();
    let mut bodies = Vec::new();
    for old in declarations {
        let body = converter
            .artifact(old, &artifacts)
            .map_err(|error| format!("{}: {error}", crate::platform::path_text(&old.file)))?;
        bodies.push((old, body));
    }
    let mut converted = Converted::default();
    let taken: BTreeSet<PathBuf> = declarations.iter().map(|old| old.file.clone()).collect();
    let shared_file = |stem: &str, extension: &str| {
        let mut file = PathBuf::from(format!("{stem}.{extension}"));
        let mut index = 2;
        while taken.contains(&file) {
            file = PathBuf::from(format!("{stem}-{index}.{extension}"));
            index += 1;
        }
        file
    };
    for (old, (header, body)) in bodies {
        let mut text = String::new();
        for toolset in &converter.toolsets {
            if toolset.users.len() == 1 && toolset.users[0] == old.name {
                text.push_str(&toolset.text);
                text.push('\n');
            }
        }
        for comment in &old.comments {
            writeln!(
                text,
                "//{}{comment}",
                if comment.is_empty() { "" } else { " " }
            )
            .expect("write to String");
        }
        write!(text, "artf {}{header} = {body}", old.name).expect("write to String");
        converted.files.push((old.file.clone(), text));
    }
    let shared: Vec<_> = converter
        .toolsets
        .iter()
        .filter(|toolset| toolset.users.len() > 1)
        .map(|toolset| toolset.text.as_str())
        .collect();
    if !shared.is_empty() {
        converted.files.push((
            shared_file("tools", "artf"),
            format!("// Tools several Artifacts compose.\n{}", shared.join("\n")),
        ));
    }
    if !converter.profiles.is_empty() {
        let mut text = String::from("// How the Agent evals run.\n");
        for (name, profile) in &converter.profiles {
            writeln!(text, "profile {name} : agent = {}", setting(profile, 0))
                .expect("write to String");
        }
        converted.files.push((
            shared_file("profiles", super::lang::PROFILES_EXTENSION),
            text,
        ));
    }
    converted.warnings = converter.warnings;
    Ok(converted)
}

struct Toolset {
    namespace: String,
    views: Value,
    users: Vec<String>,
    text: String,
}

struct Converter<'a> {
    /// Named Agent profiles, in the order they were first needed.
    profiles: Vec<(String, Map<String, Value>)>,
    toolsets: Vec<Toolset>,
    plugins: &'a BTreeSet<String>,
    warnings: Vec<String>,
    /// The Artifacts' names, which a toolset or profile does not take: names are one
    /// namespace.
    artifacts: BTreeSet<String>,
}

/// The old local Artifacts without a type that an Agent or Human eval reads, its own or one
/// naming it with `{name}`, while they declare no tools for that eval's kind. Before Artifact
/// types, a local file or folder came with the file tools; `files` gives them now.
pub(crate) fn untyped_readers(declarations: &[Old]) -> BTreeSet<String> {
    let mut read: BTreeMap<String, BTreeSet<&'static str>> = BTreeMap::new();
    for old in declarations {
        let Ok(tables) = evals(old.table.get("evals")) else {
            continue; // The conversion itself names the broken table.
        };
        for (_, eval) in tables {
            let audience = match eval
                .get("profile")
                .and_then(|profile| profile.get("kind"))
                .and_then(Value::as_str)
            {
                Some("agent") => "agent_tools",
                Some("human") => "human_tools",
                _ => continue,
            };
            let instruction = eval
                .get("payload")
                .and_then(|payload| payload.get("instruction"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            for name in std::iter::once(old.name.clone()).chain(references(instruction)) {
                read.entry(name).or_default().insert(audience);
            }
        }
    }
    declarations
        .iter()
        .filter(|old| {
            let local = old
                .table
                .get("path")
                .and_then(Value::as_str)
                .is_some_and(|path| !path.contains("://"));
            let lacks = |audience: &str| {
                old.table
                    .get("views")
                    .and_then(|views| views.get(audience))
                    .and_then(Value::as_object)
                    .is_none_or(Map::is_empty)
            };
            local
                && old.table.get("type").is_none()
                && read
                    .get(&old.name)
                    .is_some_and(|audiences| audiences.iter().any(|audience| lacks(audience)))
        })
        .map(|old| old.name.clone())
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Strings {
    Plain,
    /// A tool description: the old `{artifactName}` is `@{artf self}`.
    Described,
    /// A command argument: an old `{name}[/path]`, after an optional `--flag=`, is a path slot.
    Argument,
    /// A Human tool argument, which could also name `{artifactPath}`: `@{path self}`.
    HumanArgument,
}

impl Converter<'_> {
    /// `(header, body)`: the toolsets after `artf NAME`, and the block.
    fn artifact(
        &mut self,
        old: &Old,
        artifacts: &BTreeSet<&str>,
    ) -> Result<(String, String), String> {
        let Value::Object(table) = &old.table else {
            return Err("a declaration is a table".into());
        };
        let mut toolsets = Vec::new();
        let mut out = String::from("{\n");
        for (key, value) in table {
            match key.as_str() {
                "name" => {}
                "parts" => {
                    let parts: Vec<&str> = value
                        .as_array()
                        .ok_or("parts is a list")?
                        .iter()
                        .map(|part| part.as_str().ok_or("parts lists names"))
                        .collect::<Result<_, _>>()?;
                    writeln!(out, "  parts = [{}]", parts.join(", ")).expect("write to String");
                }
                "path" | "select" | "basis" | "tags" => {
                    writeln!(out, "  {key} = {}", plain(value, 1)).expect("write to String");
                }
                "type" => {
                    let namespace = value.as_str().ok_or("type is a plugin's name")?;
                    toolsets.push(namespace.to_owned());
                }
                "fingerprint" => {
                    let rendered = match value {
                        Value::Bool(false) => "false".to_owned(),
                        Value::Object(object) if object.contains_key("script") => {
                            let script = object["script"]
                                .as_object()
                                .ok_or("fingerprint.script is a table")?;
                            call("script", script, 1, Strings::Argument)
                        }
                        other => plain(other, 1),
                    };
                    writeln!(out, "  fingerprint = {rendered}").expect("write to String");
                }
                "views" => {
                    if let Some(namespace) = self.toolset(&old.name, value)? {
                        toolsets.push(namespace);
                    }
                }
                // Layout the removed definition editor kept; nothing reads it now.
                "evals" | "annotations" => {}
                other => {
                    self.warnings.push(format!(
                        "{}: `{other}` is not part of a declaration any more and was dropped.",
                        old.name
                    ));
                }
            }
        }
        for (id, eval) in evals(table.get("evals"))? {
            out.push('\n');
            out.push_str(&self.eval(&old.name, &id, &eval, artifacts)?);
        }
        out.push_str("}\n");
        let header = if toolsets.is_empty() {
            String::new()
        } else {
            format!(" : {}", toolsets.join(" & "))
        };
        Ok((header, out))
    }

    /// The namespace of the toolset holding an Artifact's own tools, declared on first use.
    fn toolset(&mut self, artifact: &str, views: &Value) -> Result<Option<String>, String> {
        let Value::Object(views) = views else {
            return Err("views is a table".into());
        };
        // Viewer tools came from plugins, whose toolsets declare them now.
        let mut own = Map::new();
        for (audience, tools) in views {
            let Value::Object(tools) = tools else {
                return Err(format!("views.{audience} is a table"));
            };
            let tools: Map<String, Value> = tools
                .iter()
                .filter(|(_, tool)| tool.get("kind").and_then(Value::as_str) != Some("viewer"))
                .map(|(name, tool)| (name.clone(), tool.clone()))
                .collect();
            if !tools.is_empty() {
                own.insert(audience.clone(), Value::Object(tools));
            }
        }
        if own.is_empty() {
            return Ok(None);
        }
        let own = Value::Object(own);
        if let Some(toolset) = self
            .toolsets
            .iter_mut()
            .find(|toolset| toolset.views == own)
        {
            toolset.users.push(artifact.to_owned());
            return Ok(Some(toolset.namespace.clone()));
        }
        let mut namespace = artifact.to_owned();
        let mut index = 2;
        while RESERVED.contains(&namespace.as_str())
            || BUILTIN_TOOLSETS.contains(&namespace.as_str())
            || self.plugins.contains(&namespace)
            || self.artifacts.contains(&namespace)
            || self.profiles.iter().any(|(name, _)| *name == namespace)
            || self
                .toolsets
                .iter()
                .any(|toolset| toolset.namespace == namespace)
        {
            namespace = format!(
                "{artifact}-tools{}",
                if index == 2 {
                    String::new()
                } else {
                    index.to_string()
                }
            );
            index += 1;
        }
        let text = toolset_text(&namespace, &own, false)?;
        self.toolsets.push(Toolset {
            namespace: namespace.clone(),
            views: own,
            users: vec![artifact.to_owned()],
            text,
        });
        Ok(Some(namespace))
    }

    fn eval(
        &mut self,
        artifact: &str,
        id: &str,
        eval: &Map<String, Value>,
        artifacts: &BTreeSet<&str>,
    ) -> Result<String, String> {
        let mut new_id = id.to_owned();
        if !new_id.starts_with(|c: char| c.is_ascii_alphabetic()) {
            new_id = format!("e{new_id}");
            self.warnings.push(format!(
                "{artifact}/{id}: an eval id starts with a letter now; it is {artifact}/{new_id}."
            ));
        }
        let profile = eval
            .get("profile")
            .and_then(Value::as_object)
            .ok_or_else(|| format!("eval {id} has no profile"))?;
        let kind = profile
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let instruction = eval
            .get("payload")
            .and_then(|payload| payload.get("instruction"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let references = reference_spans(instruction);
        let header = match kind {
            "human" => "human".to_owned(),
            "runtime" => {
                let mut profile = profile.clone();
                profile.remove("kind");
                call("runtime", &profile, 2, Strings::Argument)
            }
            "agent" => self.profile_name(None, &Value::Object(profile.clone()))?,
            other => return Err(format!("eval {id} has an unknown profile kind `{other}`")),
        };
        let mut out = String::new();
        if kind == "runtime" && !instruction.is_empty() {
            // Its verdict is the command's exit code; the old prose stays as a comment, an old
            // `{name}` read as the name.
            let mut text = instruction.to_owned();
            for (start, end, name) in references.iter().rev() {
                text.replace_range(*start..*end, name);
            }
            for line in text.trim().lines() {
                writeln!(
                    out,
                    "  //{}{}",
                    if line.is_empty() { "" } else { " " },
                    line.trim_end()
                )
                .expect("write to String");
            }
        }
        writeln!(out, "  eval {new_id} : {header} = {{").expect("write to String");
        if let Some(title) = eval.get("title") {
            writeln!(out, "    title = {}", plain(title, 2)).expect("write to String");
        }
        if let Some(red_at) = eval.get("red_at") {
            writeln!(out, "    red_at = {}", plain(red_at, 2)).expect("write to String");
        }
        if let Some(mutex) = eval.get("mutex") {
            writeln!(out, "    mutex = {}", plain(mutex, 2)).expect("write to String");
        }
        if !instruction.is_empty() && kind != "runtime" {
            writeln!(
                out,
                "    instruction = {}",
                instruction_text(instruction, &references)
            )
            .expect("write to String");
        }
        if kind == "runtime" {
            // Its own Artifact is what it checks, and one its arguments name is already read.
            let argued: BTreeSet<String> = profile
                .get("args")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .flat_map(|argument| {
                    reference_spans(argument)
                        .into_iter()
                        .map(|(_, _, name)| name)
                })
                .collect();
            let deps: BTreeSet<&str> = references
                .iter()
                .map(|reference| reference.2.as_str())
                .filter(|name| *name != artifact && !argued.contains(*name))
                .collect();
            if !deps.is_empty() {
                writeln!(
                    out,
                    "    deps = [{}]",
                    deps.into_iter().collect::<Vec<_>>().join(", ")
                )
                .expect("write to String");
            }
        }
        for (_, _, name) in &references {
            if !artifacts.contains(name.as_str()) {
                self.warnings.push(format!("{artifact}/{id}: its instruction names `{name}`, which no declaration declares."));
            }
        }
        if let Some(Value::Object(payload)) = eval.get("payload") {
            for key in payload.keys().filter(|key| *key != "instruction") {
                self.warnings.push(format!(
                    "{artifact}/{id}: payload.{key} was dropped; an eval's instruction says everything it asks."
                ));
            }
        }
        for key in ["pass_schema", "fail_schema"] {
            if eval.get(key).is_some_and(|schema| !schema.is_null()) {
                self.warnings.push(format!(
                    "{artifact}/{id}: {key} was dropped; a person answers GREEN, or RED with issues."
                ));
            }
        }
        if let Some(Value::Object(variants)) = eval.get("profile_variants") {
            for (name, variant) in variants {
                let declared = self.profile_name(Some(name), variant)?;
                if declared != *name {
                    self.warnings.push(format!(
                        "{artifact}/{id}: variant `{name}` differs from another of that name; it is profile `{declared}`."
                    ));
                }
            }
        }
        for key in eval.keys() {
            if !matches!(
                key.as_str(),
                "id" | "title"
                    | "red_at"
                    | "mutex"
                    | "profile"
                    | "payload"
                    | "pass_schema"
                    | "fail_schema"
                    | "profile_variants"
            ) {
                self.warnings
                    .push(format!("{artifact}/{id}: `{key}` was dropped."));
            }
        }
        out.push_str("  }\n");
        Ok(out)
    }

    /// The name of a declared Agent profile equal to `profile`, declaring it under a name
    /// from `wanted` or its model when there is none.
    fn profile_name(&mut self, wanted: Option<&str>, profile: &Value) -> Result<String, String> {
        let Value::Object(profile) = profile else {
            return Err("a profile is a table".into());
        };
        let mut profile = profile.clone();
        if profile.remove("kind").is_some_and(|kind| kind != "agent") {
            return Err("a named profile is an Agent profile".into());
        }
        if let Some((name, _)) = self.profiles.iter().find(|(name, existing)| {
            *existing == profile && wanted.is_none_or(|wanted| wanted == name)
        }) {
            return Ok(name.clone());
        }
        // An unnamed profile is named after its backend, the first thing an author changes.
        let base = identifier(
            wanted
                .or_else(|| profile.get("backend").and_then(Value::as_str))
                .or_else(|| profile.get("model").and_then(Value::as_str))
                .unwrap_or("agent"),
        );
        let mut name = base.clone();
        let mut index = 2;
        while self.profiles.iter().any(|(taken, _)| *taken == name)
            || self.artifacts.contains(&name)
            || self
                .toolsets
                .iter()
                .any(|toolset| toolset.namespace == name)
            || RESERVED.contains(&name.as_str())
        {
            name = format!("{base}-{index}");
            index += 1;
        }
        if let Some(wanted) = wanted
            && wanted != name
        {
            self.warnings.push(format!(
                "Profile `{wanted}` shares its name with another declaration; it is profile `{name}`."
            ));
        }
        self.profiles.push((name.clone(), profile));
        Ok(name)
    }
}

/// `tool NAMESPACE = { … }` for an old `views` table. Viewer tools are a plugin's own, so only a
/// plugin's toolset (`viewers`) writes them.
pub(crate) fn toolset_text(
    namespace: &str,
    views: &Value,
    viewers: bool,
) -> Result<String, String> {
    let Value::Object(views) = views else {
        return Err("views is a table".into());
    };
    for audience in views.keys() {
        if !matches!(
            audience.as_str(),
            "agent_tools" | "human_tools" | "agent_sessions"
        ) {
            return Err(format!("views.{audience} is not a kind of tool"));
        }
    }
    let mut text = format!("tool {namespace} = {{\n");
    for (audience, keyword) in [
        ("agent_tools", "agent"),
        ("human_tools", "human"),
        ("agent_sessions", "session"),
    ] {
        let Some(tools) = views.get(audience) else {
            continue;
        };
        let Value::Object(tools) = tools else {
            return Err(format!("views.{audience} is a table"));
        };
        for (name, tool) in tools {
            let Value::Object(tool) = tool else {
                return Err(format!("views.{audience}.{name} is a table"));
            };
            let viewer = tool.get("kind").and_then(Value::as_str) == Some("viewer");
            if viewer && !viewers {
                continue;
            }
            let rendered = match keyword {
                "session" => {
                    let mut parts = Vec::new();
                    for (side, command) in tool {
                        let command = command
                            .as_object()
                            .ok_or_else(|| format!("views.{audience}.{name}.{side} is a table"))?;
                        parts.push(format!(
                            "{side} = {}",
                            call("command", command, 2, Strings::Argument)
                        ));
                    }
                    format!("session({})", parts.join(", "))
                }
                _ if viewer => {
                    let mut tool = tool.clone();
                    tool.remove("kind");
                    call("viewer", &tool, 2, Strings::Plain)
                }
                _ => {
                    let mut tool = tool.clone();
                    // A plain tool's `{property}` args are its input's templates, not names.
                    let arguments = if keyword == "human" {
                        Strings::HumanArgument
                    } else if tool.get("protocol").and_then(Value::as_str) == Some("plain") {
                        Strings::Plain
                    } else {
                        Strings::Argument
                    };
                    if let Some(op) = tool.remove("builtin") {
                        tool.insert("op".into(), op);
                        call("builtin", &tool, 2, arguments)
                    } else {
                        call("command", &tool, 2, arguments)
                    }
                }
            };
            writeln!(text, "  {keyword} {name} = {rendered}").expect("write to String");
        }
    }
    text.push_str("}\n");
    Ok(text)
}

/// `text` made an identifier: letters, digits, `_` and `-`, starting with a letter.
fn identifier(text: &str) -> String {
    let mut name: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if !name.starts_with(|c: char| c.is_ascii_alphabetic()) || RESERVED.contains(&name.as_str()) {
        name.insert_str(0, "p-");
    }
    name.truncate(64);
    name
}

/// Old eval tables, each with its id, in their written order.
type EvalTables = Vec<(String, Map<String, Value>)>;

/// An old eval table, by id; test builders give a list of evals with ids.
fn evals(value: Option<&Value>) -> Result<EvalTables, String> {
    let entries: Vec<(String, Value)> = match value {
        None => Vec::new(),
        Some(Value::Object(table)) => table
            .iter()
            .map(|(id, eval)| (id.clone(), eval.clone()))
            .collect(),
        Some(Value::Array(list)) => list
            .iter()
            .map(|eval| {
                let id = eval
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or("an eval in a list has an id")?;
                Ok((id.to_owned(), eval.clone()))
            })
            .collect::<Result<_, String>>()?,
        Some(_) => return Err("evals is a table".into()),
    };
    entries
        .into_iter()
        .map(|(id, eval)| match eval {
            Value::Object(eval) => Ok((id, eval)),
            _ => Err(format!("eval {id} is a table")),
        })
        .collect()
}

/// The Artifacts an old instruction names with `{name}`, each once, in order.
pub fn references(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for (_, _, name) in reference_spans(text) {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// Old `{name}` references in an instruction: their byte range and the name. Bare names only:
/// a `${…}`, an escaped `\{`, or JSON, nested or quoted braces stayed text.
fn reference_spans(source: &str) -> Vec<(usize, usize, String)> {
    fn escaped(source: &[u8], index: usize) -> bool {
        source[..index]
            .iter()
            .rev()
            .take_while(|byte| **byte == b'\\')
            .count()
            % 2
            == 1
    }
    fn closing(source: &[u8], start: usize) -> Option<usize> {
        let literal = escaped(source, start);
        let (mut depth, mut quote, mut index) = (0, None, start);
        while index < source.len() {
            let character = source[index];
            if character == b'\\' {
                if !literal
                    || quote.is_some()
                    || !matches!(source.get(index + 1), Some(b'{' | b'}'))
                {
                    index += 1;
                }
            } else if let Some(delimiter) = quote {
                if character == delimiter {
                    quote = None;
                }
            } else if matches!(character, b'"' | b'\'' | b'`') {
                quote = Some(character);
            } else if character == b'{' {
                depth += 1;
            } else if character == b'}' {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            index += 1;
        }
        None
    }
    let bytes = source.as_bytes();
    let mut found = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'{' {
            cursor += 1;
            continue;
        }
        let Some(end) = closing(bytes, cursor) else {
            break;
        };
        let name = &source[cursor + 1..end];
        if (cursor == 0 || bytes[cursor - 1] != b'$')
            && !escaped(bytes, cursor)
            && super::artifact_name(name, "").is_ok()
        {
            found.push((cursor, end + 1, name.to_owned()));
        }
        cursor = end + 1;
    }
    found
}

/// An instruction with its old references as `@{artf …}` slots; a `{name}/sub` keeps its
/// `/sub` as text after the slot.
fn instruction_text(text: &str, references: &[(usize, usize, String)]) -> String {
    let mut pieces = Vec::new();
    let mut cursor = 0;
    for (start, end, name) in references {
        pieces.push(super::lang::Piece::Text(text[cursor..*start].to_owned()));
        pieces.push(super::lang::Piece::Slot(super::lang::SlotRef::Artf {
            target: super::lang::SlotTarget::Named(name.clone(), super::lang::Span::default()),
            span: super::lang::Span::default(),
        }));
        cursor = *end;
    }
    pieces.push(super::lang::Piece::Text(text[cursor..].to_owned()));
    render::template(&pieces, 2)
}

/// One old argument: `{name}[/path]`, after an optional `--flag=`, is a path slot.
fn argument(text: &str, human: bool) -> String {
    let references = reference_spans(text);
    if let [(start, end, name)] = references.as_slice() {
        let prefix = &text[..*start];
        let suffix = &text[*end..];
        let flag = prefix.is_empty()
            || prefix
                .strip_prefix("--")
                .and_then(|flag| flag.strip_suffix('='))
                .is_some_and(|flag| {
                    !flag.is_empty()
                        && flag
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                });
        let path = if suffix.is_empty() {
            Some("")
        } else {
            suffix.strip_prefix('/')
        };
        if flag && let Some(path) = path {
            let target = if human && name == "artifactPath" {
                "self"
            } else {
                name.as_str()
            };
            let slot = if path.is_empty() {
                format!("@{{path {target}}}")
            } else {
                format!("@{{path {target}/{path}}}")
            };
            return format!("\"{}{slot}\"", render::escape(prefix, true));
        }
    }
    render::quote(text)
}

fn string(text: &str, strings: Strings) -> String {
    match strings {
        Strings::Plain => render::quote(text),
        Strings::Described => {
            let parts: Vec<_> = text
                .split("{artifactName}")
                .map(|part| render::escape(part, true))
                .collect();
            format!("\"{}\"", parts.join("@{artf self}"))
        }
        Strings::Argument => argument(text, false),
        Strings::HumanArgument => argument(text, true),
    }
}

/// A JSON value written as an expression, its strings as plain text.
fn plain(value: &Value, depth: usize) -> String {
    expression(value, depth, Strings::Plain)
}

fn expression(value: &Value, depth: usize, strings: Strings) -> String {
    match value {
        // A JSON builder's `null` leaves a field out; callers skip it, so only a list holds one.
        Value::Null => "\"\"".into(),
        Value::Bool(value) => value.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => string(text, strings),
        Value::Array(items) => {
            let items: Vec<_> = items
                .iter()
                .map(|item| expression(item, depth + 1, strings))
                .collect();
            let line = format!("[{}]", items.join(", "));
            if line.len() <= 80 && !line.contains('\n') {
                line
            } else {
                let indent = "  ".repeat(depth + 1);
                format!(
                    "[\n{indent}{},\n{}]",
                    items.join(&format!(",\n{indent}")),
                    "  ".repeat(depth)
                )
            }
        }
        Value::Object(object) => {
            let fields: Vec<_> = object
                .iter()
                .filter(|(_, value)| !value.is_null())
                .map(|(key, value)| {
                    format!(
                        "{} = {}",
                        key_text(key),
                        expression(value, depth + 1, Strings::Plain)
                    )
                })
                .collect();
            let line = format!("{{ {} }}", fields.join(", "));
            if fields.is_empty() {
                "{}".into()
            } else if line.len() <= 80 && !line.contains('\n') {
                line
            } else {
                let indent = "  ".repeat(depth + 1);
                format!(
                    "{{\n{indent}{}\n{}}}",
                    fields.join(&format!("\n{indent}")),
                    "  ".repeat(depth)
                )
            }
        }
    }
}

fn key_text(key: &str) -> String {
    let bare = key.len() <= 64
        && key.starts_with(|c: char| c.is_ascii_alphabetic())
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare {
        key.to_owned()
    } else {
        render::quote(key)
    }
}

/// A profile's complete setting `{ backend = …, model = …, … }`, in the order an author reads it.
fn setting(fields: &Map<String, Value>, depth: usize) -> String {
    let arguments = ordered_arguments(fields, depth, Strings::Plain);
    let line = format!("{{ {} }}", arguments.join(", "));
    if line.len() <= 100 {
        line
    } else {
        let indent = "  ".repeat(depth + 1);
        format!(
            "{{\n{indent}{},\n{}}}",
            arguments.join(&format!(",\n{indent}")),
            "  ".repeat(depth)
        )
    }
}

/// `name(key = value, …)`, `args` and `command` written in `strings`, a description as one.
fn call(name: &str, fields: &Map<String, Value>, depth: usize, strings: Strings) -> String {
    let arguments = ordered_arguments(fields, depth, strings);
    let line = format!("{name}({})", arguments.join(", "));
    if line.len() <= 100 && !line.contains('\n') {
        line
    } else {
        let indent = "  ".repeat(depth + 1);
        format!(
            "{name}(\n{indent}{},\n{})",
            arguments.join(&format!(",\n{indent}")),
            "  ".repeat(depth)
        )
    }
}

/// `key = value` for each field that has a value: what a call is first, then what it runs with,
/// the authored order a JSON map has lost.
fn ordered_arguments(fields: &Map<String, Value>, depth: usize, strings: Strings) -> Vec<String> {
    // What a call is, then what it runs with: authored order, which a JSON map has lost.
    const FIRST: [&str; 9] = [
        "op",
        "backend",
        "model",
        "command",
        "args",
        "protocol",
        "session",
        "description",
        "start",
    ];
    let mut ordered: Vec<(&String, &Value)> = fields
        .iter()
        .filter(|(_, value)| !value.is_null())
        .collect();
    ordered.sort_by_key(|(key, _)| {
        FIRST
            .iter()
            .position(|first| first == key)
            .unwrap_or(FIRST.len())
    });
    ordered
        .into_iter()
        .map(|(key, value)| {
            let mode = match key.as_str() {
                "args" => strings,
                "description" => Strings::Described,
                _ => Strings::Plain,
            };
            format!("{key} = {}", expression(value, depth + 1, mode))
        })
        .collect()
}

#[cfg(test)]
mod tests;
