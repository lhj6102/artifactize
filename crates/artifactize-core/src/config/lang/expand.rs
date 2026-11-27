//! Pass two: expand every `artf` binding into an Artifact with its composed tools and evals.
//! Expansion reads no file and runs nothing; a macro is a template filled in by name.

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

use serde_json::{Map, Value, json};

use super::{
    LangError, Origin, PluginSource, Sources, Symbol, SymbolParam,
    ast::{Block, Entry, Expr, FileId, Item, ItemKind, Name, ProfileBody, ProfileItem, Span},
    scope::{Decl, Env, Globals, ProfileDecl, bind, names},
    values::{MAX_DEPTH, Strings},
};
use crate::config::{
    AgentTool, HumanTool, MutexTag, Priority, Profile, ProfileKind, ToolFqn, ToolSession, Views,
    declaration::Fingerprint,
};

/// What a slot names: the Artifact being declared, or one by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SlotTarget {
    SelfRef,
    Named(String, Span),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SlotRef {
    Artf {
        target: SlotTarget,
        span: Span,
    },
    Path {
        target: SlotTarget,
        path: String,
        span: Span,
    },
}

impl SlotRef {
    pub fn span(&self) -> Span {
        match self {
            Self::Artf { span, .. } | Self::Path { span, .. } => *span,
        }
    }

    pub fn target(&self) -> &SlotTarget {
        match self {
            Self::Artf { target, .. } | Self::Path { target, .. } => target,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Piece {
    Text(String),
    Slot(SlotRef),
}

/// An instruction as written after its parameters are filled in; slots stay typed until the
/// Artifacts they name are known.
pub(crate) type Instruction = Vec<Piece>;

#[derive(Debug, Clone)]
pub(crate) enum LoweredFingerprint {
    /// Not declared: the target kind's default.
    Default,
    /// `fingerprint = false`: never reused.
    Disabled,
    /// Declared, and whether it writes an `ignore` list, which a File Artifact may not even
    /// empty.
    Declared(Fingerprint, bool),
}

#[derive(Debug, Clone)]
pub(crate) struct LoweredEval {
    pub id: String,
    pub id_span: Span,
    pub title: String,
    pub mutex: Vec<MutexTag>,
    pub red_at: Option<Priority>,
    pub profile: Profile,
    /// The profile the eval names; none for `human` and an inline `runtime(…)`.
    pub profile_name: Option<String>,
    pub profile_span: Span,
    /// What an Agent or Human evaluator is asked; a runtime eval has none.
    pub instruction: Option<(Instruction, Span)>,
    /// What a runtime eval's command reads, besides what its arguments name.
    pub deps: Vec<(String, Span)>,
}

#[derive(Debug, Clone)]
pub(crate) struct LoweredArtifact {
    pub name: String,
    pub name_span: Span,
    /// The whole `artf` statement.
    pub binding: Span,
    pub macro_name: Option<String>,
    pub path: (String, Span),
    pub basis: Option<bool>,
    pub parts: Vec<(String, Span)>,
    pub select: Option<(Vec<String>, Span)>,
    pub fingerprint: (LoweredFingerprint, Option<Span>),
    pub tags: Vec<String>,
    /// The tool expression as written, and the namespaces it composes in order.
    pub tools: Option<(String, Span)>,
    pub toolsets: Vec<String>,
    pub views: Views,
    /// Where each composed tool is declared, by `audience.fqn`.
    pub tool_spans: BTreeMap<String, Span>,
    pub evals: Vec<LoweredEval>,
}

pub(super) struct Expanded {
    pub artifacts: Vec<LoweredArtifact>,
    /// Every profile in the branch the environment selects.
    pub profiles: BTreeMap<String, (Profile, Span)>,
    /// The environment variables the profiles read, each with its value; `None` when unset.
    pub environment: BTreeMap<String, Option<String>>,
    pub symbols: Vec<Symbol>,
    pub names: Vec<(Span, String)>,
}

/// A name `self` never takes, used to check declarations no Artifact composes.
const UNBOUND_SELF: &str = "self";

pub(super) struct Expander<'a> {
    pub globals: Globals<'a>,
    pub sources: &'a Sources,
    pub nodes: Cell<usize>,
    frames: RefCell<Vec<(Span, String)>>,
    visiting: RefCell<Vec<(ItemKind, String)>>,
    current: RefCell<Option<String>>,
    /// Every span that named an Artifact, for `config rename`.
    pub names: RefCell<std::collections::BTreeSet<(Span, String)>>,
    /// The profiles, each in the branch the environment selects.
    profiles: BTreeMap<String, (Profile, Span)>,
}

/// Leaves a macro frame or a resolving name when dropped.
struct Frame<'e, 'a> {
    expander: &'e Expander<'a>,
    visiting: bool,
}

impl Drop for Frame<'_, '_> {
    fn drop(&mut self) {
        if self.visiting {
            self.expander.visiting.borrow_mut().pop();
        } else {
            self.expander.frames.borrow_mut().pop();
        }
    }
}

/// The value of one environment variable; `None` when it is unset.
pub(crate) type Environment<'e> = &'e dyn Fn(&str) -> Option<String>;

pub(super) fn expand(
    sources: &Sources,
    parsed: &[(FileId, Vec<Item>)],
    profiles: &[(FileId, Vec<ProfileItem>)],
    environment: Environment<'_>,
) -> Result<Expanded, LangError> {
    let globals = Globals::collect(sources, parsed, profiles)?;
    let mut expander = Expander {
        globals,
        sources,
        nodes: Cell::new(0),
        frames: RefCell::new(Vec::new()),
        visiting: RefCell::new(Vec::new()),
        current: RefCell::new(None),
        names: RefCell::default(),
        profiles: BTreeMap::new(),
    };
    let (profiles, environment) = expander.resolve_profiles(environment)?;
    expander.profiles = profiles;
    let mut artifacts = Vec::new();
    let mut lowercase = BTreeMap::new();
    for (name, decl) in &expander.globals.artf.concrete {
        if let Some(previous) = lowercase.insert(name.to_ascii_lowercase(), decl.item.name.span) {
            return Err(LangError::new(
                decl.item.name.span,
                format!("Artifact names must differ ignoring case: `{name}`."),
            )
            .note(previous, "the other name"));
        }
        *expander.current.borrow_mut() = Some((*name).to_owned());
        artifacts.push(expander.artifact(*decl)?);
    }
    *expander.current.borrow_mut() = Some(UNBOUND_SELF.into());
    for (name, decl) in &expander.globals.tool.concrete {
        let toolsets = expander.named_tool(name, *decl)?;
        expander.compose(&toolsets)?;
    }
    let symbols = expander.symbols();
    for artifact in &artifacts {
        expander
            .names
            .borrow_mut()
            .insert((artifact.name_span, artifact.name.clone()));
    }
    let names = expander.names.take().into_iter().collect();
    Ok(Expanded {
        names,
        artifacts,
        profiles: expander.profiles,
        environment,
        symbols,
    })
}

impl<'a> Expander<'a> {
    /// A diagnostic, with the macro calls it was reached through as notes.
    pub(super) fn error(&self, span: Span, message: impl Into<String>) -> LangError {
        let mut error = LangError::new(span, message);
        for (call, name) in self.frames.borrow().iter().rev() {
            error = error.note(*call, format!("in the expansion of `{name}!`"));
        }
        error
    }

    pub(super) fn current(&self) -> Option<String> {
        self.current.borrow().clone()
    }

    fn enter(&self, call: Span, name: &str) -> Result<Frame<'_, 'a>, LangError> {
        if self.frames.borrow().len() >= MAX_DEPTH {
            return Err(self.error(
                call,
                format!("Macros nest more than {MAX_DEPTH} calls deep."),
            ));
        }
        self.frames.borrow_mut().push((call, name.to_owned()));
        Ok(Frame {
            expander: self,
            visiting: false,
        })
    }

    /// Resolve a concrete name once at a time: `tool a = b` and `tool b = a` is a cycle.
    fn visit(&self, kind: ItemKind, name: &str, span: Span) -> Result<Frame<'_, 'a>, LangError> {
        let key = (kind, name.to_owned());
        if self.visiting.borrow().contains(&key) {
            return Err(self.error(
                span,
                format!(
                    "`{} {name}` refers to itself through its own definition.",
                    kind.keyword()
                ),
            ));
        }
        self.visiting.borrow_mut().push(key);
        Ok(Frame {
            expander: self,
            visiting: true,
        })
    }

    fn origin(&self, file: FileId) -> &Origin {
        &self.sources.file(file).origin
    }

    fn artifact(&self, decl: Decl<'a>) -> Result<LoweredArtifact, LangError> {
        let item = decl.item;
        if item.name.text.contains('.') {
            return Err(self.error(item.name.span, "An Artifact's name is one identifier."));
        }
        let root = Env::root();
        let (tools, block, env, macro_name, _frame) = match &item.body {
            Expr::Macro { name, args, span } => {
                if let Some(header) = &item.header {
                    return Err(self.error(
                        header.span(),
                        "An Artifact made by a macro takes its tools from the macro; remove `: …` here.",
                    ));
                }
                let definition = self.macro_decl(ItemKind::Artf, name)?;
                let frame = self.enter(*span, &name.text)?;
                let env = bind(definition.item, args, &root, *span)?;
                let Expr::Block(block) = &definition.item.body else {
                    return Err(self.error(
                        definition.item.body.span(),
                        "An `artf!` macro's body is a block `{ … }`.",
                    ));
                };
                (
                    definition.item.header.as_ref(),
                    block,
                    env,
                    Some(name.text.clone()),
                    Some(frame),
                )
            }
            Expr::Block(block) => (item.header.as_ref(), block, root, None, None),
            other => {
                return Err(self.error(
                    other.span(),
                    "An Artifact is declared with a block `{ … }` or a macro call `name!(…)`.",
                ));
            }
        };
        let mut artifact = LoweredArtifact {
            name: item.name.text.clone(),
            name_span: item.name.span,
            binding: item.span,
            macro_name,
            path: (String::new(), item.name.span),
            basis: None,
            parts: Vec::new(),
            select: None,
            fingerprint: (LoweredFingerprint::Default, None),
            tags: Vec::new(),
            tools: None,
            toolsets: Vec::new(),
            views: Views::default(),
            tool_spans: BTreeMap::new(),
            evals: Vec::new(),
        };
        if let Some(tools) = tools {
            let toolsets = self.toolsets(tools, &env)?;
            let composed = self.compose(&toolsets)?;
            artifact.tools = Some((self.sources.text(tools.span()).to_owned(), tools.span()));
            artifact.toolsets = composed.namespaces;
            artifact.views = composed.views;
            artifact.tool_spans = composed.spans;
        }
        self.artifact_body(block, &env, &mut artifact)?;
        if artifact.path.0.is_empty() {
            return Err(self.error(
                item.name.span,
                format!("Artifact `{}` needs a `path`.", item.name.text),
            ));
        }
        let mut ids: BTreeMap<&str, Span> = BTreeMap::new();
        for eval in &artifact.evals {
            if let Some(previous) = ids.insert(&eval.id, eval.id_span) {
                return Err(self
                    .error(
                        eval.id_span,
                        format!(
                            "Artifact `{}` declares eval `{}` twice.",
                            artifact.name, eval.id
                        ),
                    )
                    .note(previous, "first declared here"));
            }
        }
        Ok(artifact)
    }

    fn macro_decl(&self, kind: ItemKind, name: &Name) -> Result<Decl<'a>, LangError> {
        self.globals
            .kind(kind)
            .macros
            .get(name.text.as_str())
            .copied()
            .ok_or_else(|| {
                let mut error = self.error(
                    name.span,
                    format!("Unknown `{}!` macro `{}`.", kind.keyword(), name.text),
                );
                if let Some(concrete) = self.globals.kind(kind).concrete.get(name.text.as_str()) {
                    error = error.note(
                        concrete.item.name.span,
                        format!(
                            "`{}` is a concrete `{}`; refer to it without `!(…)`",
                            name.text,
                            kind.keyword()
                        ),
                    );
                }
                error
            })
    }

    fn artifact_body(
        &self,
        block: &'a Block,
        env: &Rc<Env<'a>>,
        artifact: &mut LoweredArtifact,
    ) -> Result<(), LangError> {
        let mut seen: BTreeMap<&str, Span> = BTreeMap::new();
        for entry in &block.entries {
            match entry {
                Entry::Field { key, value } => {
                    if let Some(previous) = seen.insert(&key.text, key.span) {
                        return Err(self
                            .error(key.span, format!("Field `{}` is given twice.", key.text))
                            .note(previous, "first given here"));
                    }
                    self.artifact_field(key, value, env, artifact)?;
                }
                Entry::Declare {
                    keyword,
                    name,
                    header,
                    value,
                    ..
                } => {
                    if keyword.text != "eval" {
                        return Err(self.error(
                            keyword.span,
                            format!("An Artifact holds fields and `eval` declarations; `{}` belongs in a `tool`.", keyword.text),
                        ));
                    }
                    artifact
                        .evals
                        .push(self.eval(name, header.as_ref(), value, env)?);
                }
                Entry::Spread(value, _) => artifact.evals.extend(self.spread_evals(value, env)?),
            }
        }
        Ok(())
    }

    fn artifact_field(
        &self,
        key: &Name,
        value: &'a Expr,
        env: &Rc<Env<'a>>,
        artifact: &mut LoweredArtifact,
    ) -> Result<(), LangError> {
        let (value, env) = self.chase(value, env.clone())?;
        match key.text.as_str() {
            "path" => artifact.path = (self.text(value, &env, "`path`")?, value.span()),
            "basis" => artifact.basis = Some(self.boolean(value, &env, "`basis`")?),
            "parts" => artifact.parts = self.artifacts(value, &env, "`parts`")?,
            "select" => {
                artifact.select = Some((self.texts(value, &env, "`select`")?, value.span()));
            }
            "tags" => artifact.tags = self.texts(value, &env, "`tags`")?,
            "fingerprint" => {
                artifact.fingerprint = (self.fingerprint(value, &env)?, Some(value.span()));
            }
            other => {
                return Err(self.error(
                    key.span,
                    format!("Unknown field `{other}`; an Artifact has `path`, `basis`, `parts`, `select`, `fingerprint`, `tags` and `eval` declarations."),
                ));
            }
        }
        Ok(())
    }

    fn fingerprint(
        &self,
        value: &'a Expr,
        env: &Rc<Env<'a>>,
    ) -> Result<LoweredFingerprint, LangError> {
        let object = match value {
            Expr::Bool(false, _) => return Ok(LoweredFingerprint::Disabled),
            Expr::Block(_) => match self.json(value, env, Strings::Plain)? {
                Value::Object(object) => object,
                _ => unreachable!("a block reads as an object"),
            },
            Expr::Call { name, args, .. } if name.text == "script" => {
                let arguments = self.arguments(args, env, |field| {
                    if field == "args" {
                        Strings::Argument
                    } else {
                        Strings::Plain
                    }
                })?;
                Map::from_iter([("script".to_owned(), Value::Object(arguments))])
            }
            other => {
                return Err(self.error(
                    other.span(),
                    "`fingerprint` is `false`, `{ files = […], ignore = […] }` or `script(command = …, args = […])`.",
                ));
            }
        };
        let ignore = object.contains_key("ignore");
        Fingerprint::try_from(object)
            .map(|fingerprint| LoweredFingerprint::Declared(fingerprint, ignore))
            .map_err(|error| self.error(value.span(), format!("`fingerprint`: {error}")))
    }

    fn spread_evals(
        &self,
        value: &'a Expr,
        env: &Rc<Env<'a>>,
    ) -> Result<Vec<LoweredEval>, LangError> {
        let (value, env) = self.chase(value, env.clone())?;
        let Expr::Block(block) = value else {
            return Err(self.error(
                value.span(),
                "`...` in an Artifact spreads a block of `eval` declarations.",
            ));
        };
        block
            .entries
            .iter()
            .map(|entry| match entry {
                Entry::Declare {
                    keyword,
                    name,
                    header,
                    value,
                    ..
                } if keyword.text == "eval" => self.eval(name, header.as_ref(), value, &env),
                Entry::Spread(value, _) => Err(self.error(
                    value.span(),
                    "Spread the eval blocks one by one into the Artifact.",
                )),
                _ => Err(self.error(
                    block.span,
                    "A spread eval block holds only `eval` declarations.",
                )),
            })
            .collect()
    }

    fn eval(
        &self,
        id: &Name,
        header: Option<&'a Expr>,
        value: &'a Expr,
        env: &Rc<Env<'a>>,
    ) -> Result<LoweredEval, LangError> {
        crate::config::identifier(&id.text, "An eval id")
            .map_err(|message| self.error(id.span, message))?;
        if id.text.contains('.') {
            return Err(self.error(id.span, "An eval id is one identifier."));
        }
        let (resolved, resolved_env) = self.chase(value, env.clone())?;
        match (header, resolved) {
            (Some(header), Expr::Block(block)) => {
                let (profile, profile_name) = self.profile(header, env)?;
                self.eval_fields(
                    id,
                    block,
                    &resolved_env,
                    profile,
                    profile_name,
                    header.span(),
                )
            }
            (Some(_), other) => Err(self.error(
                other.span(),
                "`eval id : PROFILE = …` takes a block `{ … }`.",
            )),
            (None, _) => Err(self.error(
                id.span,
                format!(
                    "Eval `{}` needs a profile: `eval {} : PROFILE = {{ … }}`, with `human`, `runtime(…)` or a profile of an `.artfprofiles` file.",
                    id.text, id.text
                ),
            )),
        }
    }

    fn eval_fields(
        &self,
        id: &Name,
        block: &'a Block,
        env: &Rc<Env<'a>>,
        profile: Profile,
        profile_name: Option<String>,
        profile_span: Span,
    ) -> Result<LoweredEval, LangError> {
        let kind = profile.kind();
        let mut eval = LoweredEval {
            id: id.text.clone(),
            id_span: id.span,
            title: String::new(),
            mutex: Vec::new(),
            red_at: None,
            profile,
            profile_name,
            profile_span,
            instruction: None,
            deps: Vec::new(),
        };
        let mut seen: BTreeMap<&str, Span> = BTreeMap::new();
        for entry in &block.entries {
            let Entry::Field { key, value } = entry else {
                return Err(self.error(block.span, "An eval holds `key = value` fields."));
            };
            if let Some(previous) = seen.insert(&key.text, key.span) {
                return Err(self
                    .error(key.span, format!("Field `{}` is given twice.", key.text))
                    .note(previous, "first given here"));
            }
            if matches!(key.text.as_str(), "title" | "instruction") {
                self.written_out(value, env, &format!("`{}`", key.text))?;
            }
            let (value, env) = self.chase(value, env.clone())?;
            match key.text.as_str() {
                "title" => eval.title = self.text(value, &env, "`title`")?,
                "red_at" => {
                    if kind != ProfileKind::Agent {
                        return Err(self.error(key.span, "`red_at` is for Agent evals."));
                    }
                    let text = self.text(value, &env, "`red_at`")?;
                    eval.red_at =
                        Some(serde_json::from_value(Value::String(text)).map_err(|_| {
                            self.error(
                                value.span(),
                                "`red_at` is \"P0\", \"P1\", \"P2\" or \"P3\".",
                            )
                        })?);
                }
                "mutex" => {
                    if kind == ProfileKind::Human {
                        return Err(self.error(key.span, "`mutex` is for runtime and Agent evals."));
                    }
                    eval.mutex = self
                        .texts(value, &env, "`mutex`")?
                        .into_iter()
                        .map(|tag| {
                            tag.parse()
                                .map_err(|message: String| self.error(value.span(), message))
                        })
                        .collect::<Result<_, _>>()?;
                    crate::config::mutex::validate(&eval.mutex)
                        .map_err(|message| self.error(value.span(), message))?;
                }
                "instruction" => {
                    if kind == ProfileKind::Runtime {
                        return Err(self.error(
                            key.span,
                            "A runtime eval's verdict is its command's exit code, so it takes no `instruction`: explain it in a `//` comment and list what it reads in `deps`.",
                        ));
                    }
                    let pieces = self.template(value, &env, "`instruction`")?;
                    if let Some(slot) = pieces.iter().find_map(|piece| match piece {
                        Piece::Slot(slot @ SlotRef::Path { .. }) => Some(slot),
                        _ => None,
                    }) {
                        return Err(self.error(
                            slot.span(),
                            "An instruction names Artifacts with `@{artf …}`; a `@{path …}` belongs in a command's arguments.",
                        ));
                    }
                    eval.instruction = Some((pieces, value.span()));
                }
                "deps" => {
                    if kind != ProfileKind::Runtime {
                        return Err(self.error(key.span, "`deps` is for runtime evals; an Agent or Human eval names what it reads with `@{artf …}` in its instruction."));
                    }
                    eval.deps = self.artifacts(value, &env, "`deps`")?;
                }
                other => {
                    return Err(self.error(
                        key.span,
                        format!("Unknown field `{other}`; an Agent or Human eval has `title`, `instruction` and `mutex`, an Agent eval also `red_at`, and a runtime eval `title`, `mutex` and `deps`."),
                    ));
                }
            }
        }
        if eval.title.trim().is_empty() {
            return Err(self.error(
                id.span,
                format!("Eval `{}` needs a nonblank `title`.", id.text),
            ));
        }
        if kind != ProfileKind::Runtime && eval.instruction.is_none() {
            return Err(self.error(
                id.span,
                format!("Eval `{}` needs an `instruction`.", id.text),
            ));
        }
        Ok(eval)
    }

    /// The evaluator in an eval's header: `human`, an inline `runtime(…)`, or a profile's name.
    fn profile(
        &self,
        expr: &'a Expr,
        env: &Rc<Env<'a>>,
    ) -> Result<(Profile, Option<String>), LangError> {
        let (expr, env) = self.chase(expr, env.clone())?;
        let json = match expr {
            Expr::Ident(name) if name.text == "human" => json!({"kind": "human"}),
            Expr::Ident(name) => {
                let (profile, _) = self.profiles.get(name.text.as_str()).ok_or_else(|| {
                    self.error(
                        name.span,
                        format!(
                            "Unknown profile `{}`; the profiles of the `.artfprofiles` files are {}.",
                            name.text,
                            names(self.profiles.keys().map(String::as_str))
                        ),
                    )
                })?;
                return Ok((profile.clone(), Some(name.text.clone())));
            }
            Expr::Call { name, args, .. } if name.text == "runtime" => {
                let mut object = self.arguments(args, &env, |field| {
                    if field == "args" {
                        Strings::Argument
                    } else {
                        Strings::Plain
                    }
                })?;
                object.insert("kind".into(), json!("runtime"));
                Value::Object(object)
            }
            Expr::Call { name, .. } if name.text == "agent" => {
                return Err(self.error(
                    name.span,
                    "An Agent eval names a profile of an `.artfprofiles` file: `profile NAME : agent = { … }`, then `eval ID : NAME`.",
                ));
            }
            other => {
                return Err(self.error(
                    other.span(),
                    "Expected an evaluator: `human`, `runtime(…)` or a profile's name.",
                ));
            }
        };
        let profile: Profile = serde_json::from_value(json)
            .map_err(|error| self.error(expr.span(), format!("Profile: {error}")))?;
        profile
            .check()
            .map_err(|message| self.error(expr.span(), message))?;
        Ok((profile, None))
    }

    /// Every profile in the branch the environment selects. Every branch is read, so a mistake
    /// in one the environment does not select fails too. A variable set to a value no profile's
    /// `match` names is refused rather than read as `default`.
    #[expect(
        clippy::type_complexity,
        reason = "the profiles and the environment values they read are returned together"
    )]
    fn resolve_profiles(
        &self,
        environment: Environment<'_>,
    ) -> Result<
        (
            BTreeMap<String, (Profile, Span)>,
            BTreeMap<String, Option<String>>,
        ),
        LangError,
    > {
        let mut labels: BTreeMap<&str, (Span, std::collections::BTreeSet<&str>)> = BTreeMap::new();
        for decl in self.globals.profiles.values() {
            if let ProfileBody::Match { variable, arms } = &decl.item.body {
                let entry = labels
                    .entry(variable.text.as_str())
                    .or_insert((variable.span, std::collections::BTreeSet::new()));
                entry.1.extend(
                    arms.iter()
                        .map(|(label, _)| label.text.as_str())
                        .filter(|label| *label != "default"),
                );
            }
        }
        let mut values = BTreeMap::new();
        for (variable, (span, known)) in &labels {
            let value = environment(variable).filter(|value| !value.trim().is_empty());
            if let Some(value) = &value
                && !known.contains(value.as_str())
            {
                return Err(self.error(
                    *span,
                    format!(
                        "{variable}={value} names no branch of any profile; the branches are {}. Unset {variable} for each profile's `default`.",
                        names(known.iter().copied())
                    ),
                ));
            }
            values.insert((*variable).to_owned(), value);
        }
        let mut profiles = BTreeMap::new();
        for (name, decl) in &self.globals.profiles {
            let profile = self.profile_branch(*decl, &values)?;
            profiles.insert((*name).to_owned(), (profile, decl.item.name.span));
        }
        Ok((profiles, values))
    }

    /// One profile: every branch read, the one `values` selects returned.
    fn profile_branch(
        &self,
        decl: ProfileDecl<'a>,
        values: &BTreeMap<String, Option<String>>,
    ) -> Result<Profile, LangError> {
        let item = decl.item;
        if item.kind.text != "agent" {
            return Err(self.error(
                item.kind.span,
                "A profile is `: agent`; a runtime or Human eval writes its evaluator in its own header.",
            ));
        }
        match &item.body {
            ProfileBody::Setting(block) => self.agent_setting(block),
            ProfileBody::Match { variable, arms } => {
                let mut seen: BTreeMap<&str, Span> = BTreeMap::new();
                let mut chosen = None;
                let mut default = None;
                let wanted = values.get(&variable.text).cloned().flatten();
                for (label, block) in arms {
                    if let Some(previous) = seen.insert(&label.text, label.span) {
                        return Err(self
                            .error(
                                label.span,
                                format!("Branch `{}` is given twice.", label.text),
                            )
                            .note(previous, "first given here"));
                    }
                    let setting = self.agent_setting(block)?;
                    if label.text == "default" {
                        default = Some(setting);
                    } else if wanted.as_deref() == Some(label.text.as_str()) {
                        chosen = Some(setting);
                    }
                }
                let default = default.ok_or_else(|| {
                    self.error(
                        item.name.span,
                        format!(
                            "Profile `{}` matches {} without a `default` branch, which runs while {} is unset or names another branch.",
                            item.name.text, variable.text, variable.text
                        ),
                    )
                })?;
                Ok(chosen.unwrap_or(default))
            }
        }
    }

    /// One complete Agent setting: `{ backend = …, model = …, … }`.
    fn agent_setting(&self, block: &'a Block) -> Result<Profile, LangError> {
        let root = Env::root();
        let mut object = Map::new();
        for entry in &block.entries {
            let Entry::Field { key, value } = entry else {
                return Err(self.error(
                    block.span,
                    "A profile's setting holds `key = value` fields.",
                ));
            };
            if object
                .insert(key.text.clone(), self.json(value, &root, Strings::Plain)?)
                .is_some()
            {
                return Err(self.error(key.span, format!("Field `{}` is given twice.", key.text)));
            }
        }
        object.insert("kind".into(), json!("agent"));
        let profile: Profile = serde_json::from_value(Value::Object(object))
            .map_err(|error| self.error(block.span, format!("Profile: {error}")))?;
        profile
            .check()
            .map_err(|message| self.error(block.span, message))?;
        Ok(profile)
    }

    /// The toolsets a tool expression composes, in order.
    fn toolsets(&self, expr: &'a Expr, env: &Rc<Env<'a>>) -> Result<Vec<Toolset<'a>>, LangError> {
        let (expr, env) = self.chase(expr, env.clone())?;
        match expr {
            Expr::And(terms, _) => {
                let mut all = Vec::new();
                for term in terms {
                    all.extend(self.toolsets(term, &env)?);
                }
                Ok(all)
            }
            Expr::Ident(name) => {
                let decl = self
                    .globals
                    .tool
                    .concrete
                    .get(name.text.as_str())
                    .copied()
                    .ok_or_else(|| {
                        let known = self.globals.tool.concrete.keys().copied();
                        self.error(
                            name.span,
                            format!(
                                "Unknown toolset `{}`; the toolsets are {}.",
                                name.text,
                                names(known)
                            ),
                        )
                    })?;
                self.named_tool(&name.text, decl)
            }
            other => Err(self.error(
                other.span(),
                "Expected a toolset: a `tool`'s name, or `a & b`.",
            )),
        }
    }

    fn named_tool(&self, name: &str, decl: Decl<'a>) -> Result<Vec<Toolset<'a>>, LangError> {
        let _visit = self.visit(ItemKind::Tool, name, decl.item.name.span)?;
        if decl.item.header.is_some() {
            return Err(self.error(decl.item.name.span, "A tool has no `: …` header."));
        }
        match &decl.item.body {
            Expr::Block(block) => Ok(vec![self.toolset(
                name,
                decl.item.name.span,
                block,
                Env::root(),
                decl.file,
            )?]),
            alias => self.toolsets(alias, &Env::root()),
        }
    }

    fn toolset(
        &self,
        namespace: &str,
        span: Span,
        block: &'a Block,
        env: Rc<Env<'a>>,
        file: FileId,
    ) -> Result<Toolset<'a>, LangError> {
        ToolFqn::namespace(namespace).map_err(|message| self.error(span, message))?;
        let plugin = match self.origin(file) {
            Origin::Plugin(plugin) => Some(plugin.clone()),
            Origin::Project => None,
        };
        let mut members = Vec::new();
        for entry in &block.entries {
            match entry {
                Entry::Declare {
                    keyword,
                    name,
                    header: None,
                    value,
                    span,
                } if matches!(keyword.text.as_str(), "agent" | "human" | "session") => {
                    if name.text.contains('.') {
                        return Err(self.error(name.span, "A tool's own name is one identifier; its namespace comes from the `tool`."));
                    }
                    members.push(Member {
                        audience: keyword.text.clone(),
                        name: name.clone(),
                        value,
                        env: env.clone(),
                        span: *span,
                    });
                }
                Entry::Declare { keyword, .. } => {
                    return Err(self.error(
                        keyword.span,
                        format!("A toolset declares `agent NAME = …`, `human NAME = …` and `session NAME = …`, not `{}`.", keyword.text),
                    ));
                }
                Entry::Field { key, .. } => {
                    return Err(self.error(
                        key.span,
                        format!("Unknown field `{}` in a toolset.", key.text),
                    ));
                }
                Entry::Spread(value, _) => {
                    return Err(self.error(
                        value.span(),
                        "Compose toolsets with `&` instead of spreading them.",
                    ));
                }
            }
        }
        Ok(Toolset {
            namespace: namespace.to_owned(),
            members,
            plugin,
        })
    }

    /// Lower toolsets for the Artifact being declared: identical definitions merge, and two
    /// different ones may not claim the same name for one audience.
    fn compose(&self, toolsets: &[Toolset<'a>]) -> Result<Composed, LangError> {
        let mut composed = Composed::default();
        let mut lowered: BTreeMap<String, (Value, Span)> = BTreeMap::new();
        for toolset in toolsets {
            if !composed.namespaces.contains(&toolset.namespace) {
                composed.namespaces.push(toolset.namespace.clone());
            }
            for member in &toolset.members {
                let fqn = format!("{}.{}", toolset.namespace, member.name.text);
                let key = format!("{}.{fqn}", member.audience);
                let value = self.member(toolset, member)?;
                match lowered.get(&key) {
                    Some((existing, _)) if *existing == value => continue,
                    Some((_, previous)) => {
                        return Err(self
                            .error(
                                member.span,
                                format!(
                                    "Two different {} tools are both named `{fqn}`.",
                                    member.audience
                                ),
                            )
                            .note(*previous, "the other definition"));
                    }
                    None => {}
                }
                let id: ToolFqn = fqn
                    .parse()
                    .map_err(|message: String| self.error(member.name.span, message))?;
                let invalid = |error: serde_json::Error| {
                    self.error(member.span, format!("Tool `{fqn}`: {error}"))
                };
                match member.audience.as_str() {
                    "agent" => {
                        let tool: AgentTool =
                            serde_json::from_value(value.clone()).map_err(invalid)?;
                        composed.views.agent_tools.insert(id, tool);
                    }
                    "human" => {
                        let tool: HumanTool =
                            serde_json::from_value(value.clone()).map_err(invalid)?;
                        composed.views.human_tools.insert(id, tool);
                    }
                    _ => {
                        let session: ToolSession =
                            serde_json::from_value(value.clone()).map_err(invalid)?;
                        composed.views.agent_sessions.insert(id, session);
                    }
                }
                composed.spans.insert(key.clone(), member.span);
                lowered.insert(key, (value, member.span));
            }
        }
        Ok(composed)
    }

    /// One tool as the declaration types read it.
    fn member(&self, toolset: &Toolset<'a>, member: &Member<'a>) -> Result<Value, LangError> {
        let (value, env) = self.chase(member.value, member.env.clone())?;
        let Expr::Call { name, args, span } = value else {
            return Err(self.error(value.span(), "A tool is `builtin(…)`, `command(…)`, `viewer(…)` or, for a session, `session(…)`."));
        };
        let strings = |field: &str| match field {
            "args" if member.audience == "human" || name.text == "command" => Strings::Argument,
            "description" => Strings::Described,
            _ => Strings::Plain,
        };
        let audience = member.audience.as_str();
        let mut object = match (audience, name.text.as_str()) {
            ("session", "session") => {
                let mut object = Map::new();
                for arg in args {
                    let Expr::Call {
                        name: command,
                        args: command_args,
                        ..
                    } = self.chase(&arg.value, env.clone())?.0
                    else {
                        return Err(self.error(arg.value.span(), "A session's `start` and `stop` are `command(command = …, args = […])`."));
                    };
                    if command.text != "command" {
                        return Err(self.error(
                            command.span,
                            "A session's `start` and `stop` are `command(…)`.",
                        ));
                    }
                    let command = self.arguments(command_args, &env, |field| {
                        if field == "args" {
                            Strings::Argument
                        } else {
                            Strings::Plain
                        }
                    })?;
                    object.insert(arg.name.text.clone(), Value::Object(command));
                }
                object
            }
            ("agent" | "human", "builtin") => {
                let mut object = self.arguments(args, &env, |field| match field {
                    "args" if audience == "human" => Strings::Argument,
                    "description" => Strings::Described,
                    _ => Strings::Plain,
                })?;
                let op = object.remove("op").ok_or_else(|| {
                    self.error(*span, "`builtin(…)` names its operation: `op = \"read\"`.")
                })?;
                object.insert("builtin".into(), op);
                object
            }
            ("agent" | "human", "command") => {
                let mut object = self.arguments(args, &env, strings)?;
                if let Some(Value::String(session)) = object.get("session").cloned() {
                    object.insert(
                        "session".into(),
                        Value::String(format!("{}.{session}", toolset.namespace)),
                    );
                }
                object
            }
            ("human", "viewer") => {
                let Some(PluginSource {
                    namespace,
                    viewer: true,
                }) = &toolset.plugin
                else {
                    return Err(self.error(
                        *span,
                        "Only a plugin with a viewer entry declares a `viewer(…)` tool.",
                    ));
                };
                let mut object = self.arguments(args, &env, strings)?;
                object.insert("kind".into(), json!("viewer"));
                object.insert("plugin".into(), json!(namespace));
                object
            }
            (audience, constructor) => {
                return Err(self.error(
                    name.span,
                    format!("A `{audience}` tool cannot be `{constructor}(…)`: agent and human tools are `builtin(…)` or `command(…)`, a human viewer `viewer(…)`, a session `session(start = …, stop = …)`."),
                ));
            }
        };
        if audience == "human" && name.text == "builtin" && !object.contains_key("args") {
            object.insert("args".into(), json!([]));
        }
        Ok(Value::Object(object))
    }

    fn symbols(&self) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        for kind in [ItemKind::Artf, ItemKind::Tool] {
            let namespace = self.globals.kind(kind);
            for (is_macro, table) in [(false, &namespace.concrete), (true, &namespace.macros)] {
                for decl in table.values() {
                    let file = self.sources.file(decl.file);
                    symbols.push(Symbol {
                        kind: kind.keyword(),
                        name: decl.item.name.text.clone(),
                        is_macro,
                        params: decl.item.params.as_ref().map(|params| {
                            params
                                .iter()
                                .map(|param| SymbolParam {
                                    name: param.name.text.clone(),
                                    default: param.default.as_ref().map(|default| {
                                        self.sources.text(default.span()).to_owned()
                                    }),
                                })
                                .collect()
                        }),
                        file: file.path.clone(),
                        line: self.sources.line_column(decl.item.name.span).0,
                        plugin: match &file.origin {
                            Origin::Plugin(plugin) => Some(plugin.namespace.clone()),
                            Origin::Project => None,
                        },
                    });
                }
            }
        }
        for decl in self.globals.profiles.values() {
            symbols.push(Symbol {
                kind: "profile",
                name: decl.item.name.text.clone(),
                is_macro: false,
                params: None,
                file: self.sources.file(decl.file).path.clone(),
                line: self.sources.line_column(decl.item.name.span).0,
                plugin: None,
            });
        }
        symbols
    }
}

struct Toolset<'a> {
    namespace: String,
    members: Vec<Member<'a>>,
    plugin: Option<PluginSource>,
}

struct Member<'a> {
    audience: String,
    name: Name,
    value: &'a Expr,
    env: Rc<Env<'a>>,
    span: Span,
}

#[derive(Default)]
struct Composed {
    namespaces: Vec<String>,
    views: Views,
    spans: BTreeMap<String, Span>,
}
