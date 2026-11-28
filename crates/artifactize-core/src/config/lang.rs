//! The `.artf` language: `artf` bindings with their `eval`s, `tool` sets composed with `&`,
//! `artf!` macros and typed `@{…}` slots; and `.artfprofiles` files, whose profiles say how
//! Agent evals run, each branch chosen by the environment. Every file of a project shares one
//! scope; reading resolves names in two passes, so declaration order is free, and expands
//! macros into plain Artifacts without running anything.

pub(crate) mod ast;
mod expand;
mod parse;
pub(crate) mod render;
mod scope;
mod values;

use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

pub(crate) use ast::{FileId, Span};
pub(crate) use expand::{
    Environment, LoweredArtifact, LoweredEval, LoweredFingerprint, Piece, SlotRef, SlotTarget,
};

/// Where a definition file comes from: the project's `.artifactize/`, or a plugin, whose
/// files may declare only its toolset and macros.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Origin {
    Project,
    Plugin(PluginSource),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PluginSource {
    pub namespace: String,
    pub viewer: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct SourceFile {
    /// Workspace-relative for project files; `plugins/<namespace>/…` for plugin files.
    pub path: PathBuf,
    pub text: String,
    pub origin: Origin,
}

/// The extension of a file of profiles.
pub(crate) const PROFILES_EXTENSION: &str = "artfprofiles";

impl SourceFile {
    /// Whether it holds profiles rather than declarations.
    pub(crate) fn holds_profiles(&self) -> bool {
        self.path
            .extension()
            .is_some_and(|extension| extension == PROFILES_EXTENSION)
    }
}

/// Every file read together, so spans can be shown as `file: line, column`.
#[derive(Debug, Clone, Default)]
pub struct Sources {
    files: Vec<SourceFile>,
}

impl Sources {
    pub(crate) fn new(files: Vec<SourceFile>) -> Self {
        Self { files }
    }

    pub(crate) fn file(&self, id: FileId) -> &SourceFile {
        &self.files[id.0 as usize]
    }

    pub(crate) fn ids(&self) -> impl Iterator<Item = (FileId, &SourceFile)> {
        self.files.iter().enumerate().map(|(index, file)| {
            (
                FileId(u32::try_from(index).expect("fewer than 2^32 files")),
                file,
            )
        })
    }

    /// `line L, column C` of a span's start.
    pub(crate) fn line_column(&self, span: Span) -> (usize, usize) {
        let text = &self.file(span.file).text;
        let prefix = &text[..(span.start as usize).min(text.len())];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
        (line, column)
    }

    pub(crate) fn text(&self, span: Span) -> &str {
        &self.file(span.file).text[span.range()]
    }

    /// The whole lines a span covers, for showing a declaration as written.
    pub(crate) fn lines(&self, span: Span) -> (usize, &str) {
        let text = &self.file(span.file).text;
        let start = text[..span.start as usize]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        let end = text[span.end as usize..]
            .find('\n')
            .map_or(text.len(), |index| span.end as usize + index);
        (self.line_column(span).0, &text[start..end])
    }

    pub(crate) fn error(&self, error: LangError) -> super::ConfigError {
        let mut message = self.positioned(error.span, &error.message);
        for (span, note) in &error.notes {
            message.push_str("\n  note: ");
            message.push_str(&crate::platform::path_text(&self.file(span.file).path));
            message.push_str(": ");
            message.push_str(&self.positioned(*span, note));
        }
        super::ConfigError::new(self.file(error.span.file).path.clone(), message)
    }

    fn positioned(&self, span: Span, message: &str) -> String {
        let (line, column) = self.line_column(span);
        format!("line {line}, column {column}: {message}")
    }
}

/// A diagnostic at one span, with notes at others: a macro's call site, or the other side of
/// a duplicate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LangError {
    pub span: Span,
    pub message: String,
    pub notes: Vec<(Span, String)>,
}

impl LangError {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            notes: Vec::new(),
        }
    }

    pub fn note(mut self, span: Span, note: impl Into<String>) -> Self {
        self.notes.push((span, note.into()));
        self
    }
}

/// What reading a project produces: its Artifacts, expanded, and its profiles, each in the
/// branch the environment selects.
#[derive(Debug, Clone)]
pub(crate) struct Program {
    pub sources: Arc<Sources>,
    pub artifacts: Vec<LoweredArtifact>,
    pub profiles: BTreeMap<String, (super::Profile, Span)>,
    /// The environment variables the profiles read, each with its value.
    pub environment: BTreeMap<String, Option<String>>,
    pub symbols: Vec<Symbol>,
    /// Every span that names an Artifact: its bindings, the names it is listed by and the
    /// slots that name it, each once.
    pub names: Vec<(Span, String)>,
}

/// A global binding, for `config symbols`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Symbol {
    pub kind: &'static str,
    pub name: String,
    #[serde(rename = "macro")]
    pub is_macro: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Vec<SymbolParam>>,
    #[serde(serialize_with = "crate::platform::path_serde::serialize")]
    pub file: PathBuf,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SymbolParam {
    pub name: String,
    pub default: Option<String>,
}

/// Parse and expand every file, choosing each profile's branch by `environment`. A file that
/// fails to parse names its own position.
pub(crate) fn read(
    files: Vec<SourceFile>,
    environment: Environment<'_>,
) -> Result<Program, (Arc<Sources>, LangError)> {
    let sources = Arc::new(Sources::new(files));
    let mut parsed = Vec::new();
    let mut profiles = Vec::new();
    for (id, file) in sources.ids() {
        let failed =
            |error: parse::ParseError| (sources.clone(), LangError::new(error.span, error.message));
        if file.holds_profiles() {
            profiles.push((id, parse::parse_profiles(id, &file.text).map_err(failed)?));
        } else {
            parsed.push((id, parse::parse(id, &file.text).map_err(failed)?));
        }
    }
    let expanded = expand::expand(&sources, &parsed, &profiles, environment)
        .map_err(|error| (sources.clone(), error))?;
    Ok(Program {
        sources,
        artifacts: expanded.artifacts,
        profiles: expanded.profiles,
        environment: expanded.environment,
        symbols: expanded.symbols,
        names: expanded.names,
    })
}

/// The process environment, as a project's profiles read it.
pub(crate) fn process_environment(name: &str) -> Option<String> {
    crate::platform::environment::var_text(name)
}

/// Whether a project file is written in the language at all, or is an older TOML definition
/// that `config migrate` converts.
pub(crate) fn looks_like_toml(text: &str) -> bool {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("//"))
        .is_some_and(|line| {
            line.starts_with('[')
                || line
                    .split_once('=')
                    .is_some_and(|(key, _)| !key.trim().contains(' ') && !key.trim().is_empty())
                    && !["artf", "tool", "profile"]
                        .iter()
                        .any(|keyword| line.starts_with(keyword))
        })
}

/// The span of the part of an Artifact's declaration that `keys` names, or its binding.
pub(crate) fn span_of(artifact: &LoweredArtifact, keys: &[&str]) -> Span {
    let eval = |id: &str| artifact.evals.iter().find(|eval| eval.id == id);
    match keys {
        ["path", ..] => artifact.path.1,
        ["name", ..] => artifact.name_span,
        ["parts", ..] => artifact
            .parts
            .first()
            .map_or(artifact.binding, |part| part.1),
        ["select", ..] => artifact
            .select
            .as_ref()
            .map_or(artifact.binding, |select| select.1),
        ["fingerprint", ..] => artifact.fingerprint.1.unwrap_or(artifact.binding),
        ["tools"] | ["views"] => artifact
            .tools
            .as_ref()
            .map_or(artifact.binding, |tools| tools.1),
        ["views", table, tool, ..] => {
            let audience = match *table {
                "human_tools" => "human",
                "agent_sessions" => "session",
                _ => "agent",
            };
            artifact
                .tool_spans
                .get(&format!("{audience}.{tool}"))
                .copied()
                .or(artifact.tools.as_ref().map(|tools| tools.1))
                .unwrap_or(artifact.binding)
        }
        ["evals", id, rest @ ..] => eval(id).map_or(artifact.binding, |eval| match rest {
            ["instruction" | "payload", ..] => eval
                .instruction
                .as_ref()
                .map_or(eval.id_span, |instruction| instruction.1),
            ["profile", ..] => eval.profile_span,
            ["inputs" | "deps", ..] => eval.deps.first().map_or(eval.id_span, |dep| dep.1),
            _ => eval.id_span,
        }),
        _ => artifact.name_span,
    }
}
