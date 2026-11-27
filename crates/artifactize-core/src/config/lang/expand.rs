//! Pass two: expand every `artf` binding into an Artifact with its composed tools and evals.
//! Expansion reads no file and runs nothing; a macro is a template filled in by name.

use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use serde_json::{Map, Value, json};

use super::{
    LangError, Origin, PluginSource, Sources, Symbol, SymbolParam,
    ast::{Block, Entry, Expr, FileId, Item, ItemKind, Name, Span},
    scope::{Decl, Env, Globals, bind, names},
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
    Tool {
        target: SlotTarget,
        audience: String,
        tool: String,
        span: Span,
    },
}

impl SlotRef {
    pub fn span(&self) -> Span {
        match self {
            Self::Artf { span, .. } | Self::Path { span, .. } | Self::Tool { span, .. } => *span,
        }
    }

    pub fn target(&self) -> &SlotTarget {
        match self {
            Self::Artf { target, .. } | Self::Path { target, .. } | Self::Tool { target, .. } => {
                target
            }
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
    /// Not declared, or `none`: the target kind's default.
    Default,
    /// `fingerprint = false`: never reused.
    Disabled,
    Declared(Fingerprint),
}

#[derive(Debug, Clone)]
pub(crate) struct LoweredEval {
    pub id: String,
    pub id_span: Span,
    pub span: Span,
    pub title: String,
    pub mutex: Vec<MutexTag>,
    pub red_at: Option<Priority>,
    pub profile: Profile,
    /// The named profile or `name!` macro the eval refers to; none for `human` and an inline
    /// `runtime(…)`.
    pub profile_name: Option<String>,
    pub profile_span: Span,
    pub instruction: Option<(Instruction, Span)>,
    pub inputs: Vec<(String, Span)>,
    /// The `eval!` macro or global eval it was made from.
    pub source: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct LoweredArtifact {
    pub name: String,
    pub name_span: Span,
    /// The whole `artf` statement.
    pub binding: Span,
    pub macro_name: Option<String>,
    /// The `artf!` declaration whose body made it.
    pub macro_body: Option<Span>,
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
    pub profiles: BTreeMap<String, (Profile, Span)>,
    pub symbols: Vec<Symbol>,
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

pub(super) fn expand(sources: &Sources, parsed: &[(FileId, Vec<Item>)]) -> Result<Expanded, LangError> {
    let globals = Globals::collect(sources, parsed)?;
    let expander = Expander {
        globals,
        sources,
        nodes: Cell::new(0),
        frames: RefCell::new(Vec::new()),
        visiting: RefCell::new(Vec::new()),
        current: RefCell::new(None),
    };
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
    let mut profiles = BTreeMap::new();
    for (name, decl) in &expander.globals.profile.concrete {
        let (profile, _) = expander.named_profile(name, *decl)?;
        if profile.kind() == ProfileKind::Agent {
            profiles.insert((*name).to_owned(), (profile, decl.item.name.span));
        }
    }
    for (name, decl) in &expander.globals.tool.concrete {
        let toolsets = expander.named_tool(name, *decl)?;
        expander.compose(&toolsets)?;
    }
    for decl in expander.globals.eval.concrete.values() {
        expander.global_eval(&decl.item.name, *decl)?;
    }
    let symbols = expander.symbols();
    Ok(Expanded {
        artifacts,
        profiles,
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
            return Err(self.error(call, format!("Macros nest more than {MAX_DEPTH} calls deep.")));
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
                format!("`{} {name}` refers to itself through its own definition.", kind.keyword()),
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
        let (tools, block, env, macro_name, macro_body, _frame) = match &item.body {
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
                    return Err(self.error(definition.item.body.span(), "An `artf!` macro's body is a block `{ … }`."));
                };
                (
                    definition.item.header.as_ref(),
                    block,
                    env,
                    Some(name.text.clone()),
                    Some(definition.item.span),
                    Some(frame),
                )
            }
            Expr::Block(block) => (item.header.as_ref(), block, root, None, None, None),
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
            macro_body,
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
            return Err(self.error(item.name.span, format!("Artifact `{}` needs a `path`.", item.name.text)));
        }
        let mut ids: BTreeMap<&str, Span> = BTreeMap::new();
        for eval in &artifact.evals {
            if let Some(previous) = ids.insert(&eval.id, eval.id_span) {
                return Err(self
                    .error(eval.id_span, format!("Artifact `{}` declares eval `{}` twice.", artifact.name, eval.id))
                    .note(previous, "first declared here"));
            }
        }
        Ok(artifact)
    }

    fn macro_decl(&self, kind: ItemKind, name: &Name) -> Result<Decl<'a>, LangError> {
        self.globals.kind(kind).macros.get(name.text.as_str()).copied().ok_or_else(|| {
            let mut error = self.error(
                name.span,
                format!("Unknown `{}!` macro `{}`.", kind.keyword(), name.text),
            );
            if let Some(concrete) = self.globals.kind(kind).concrete.get(name.text.as_str()) {
                error = error.note(
                    concrete.item.name.span,
                    format!("`{}` is a concrete `{}`; refer to it without `!(…)`", name.text, kind.keyword()),
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
                        return Err(self.error(key.span, format!("Field `{}` is given twice.", key.text)).note(previous, "first given here"));
                    }
                    self.artifact_field(key, value, env, artifact)?;
                }
                Entry::Declare {
                    keyword,
                    name,
                    header,
                    value,
                    span,
                } => {
                    if keyword.text != "eval" {
                        return Err(self.error(
                            keyword.span,
                            format!("An Artifact holds fields and `eval` declarations; `{}` belongs in a `tool`.", keyword.text),
                        ));
                    }
                    artifact.evals.push(self.eval(name, header.as_ref(), value, env, *span)?);
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
        let optional = self.optional(value, env)?;
        match key.text.as_str() {
            "path" => {
                let Some((value, env)) = optional else {
                    return Err(self.error(value.span(), "`path` is required."));
                };
                artifact.path = (self.text(value, &env, "`path`")?, value.span());
            }
            "basis" => {
                artifact.basis = optional
                    .map(|(value, env)| self.boolean(value, &env, "`basis`"))
                    .transpose()?;
            }
            "parts" => {
                if let Some((value, env)) = optional {
                    artifact.parts = self.artifacts(value, &env, "`parts`")?;
                }
            }
            "select" => {
                artifact.select = optional
                    .map(|(value, env)| Ok((self.texts(value, &env, "`select`")?, value.span())))
                    .transpose()?;
            }
            "tags" => {
                if let Some((value, env)) = optional {
                    artifact.tags = self.texts(value, &env, "`tags`")?;
                }
            }
            "fingerprint" => {
                artifact.fingerprint = match optional {
                    None => (LoweredFingerprint::Default, Some(value.span())),
                    Some((value, env)) => (self.fingerprint(value, &env)?, Some(value.span())),
                };
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

    fn fingerprint(&self, value: &'a Expr, env: &Rc<Env<'a>>) -> Result<LoweredFingerprint, LangError> {
        let object = match value {
            Expr::Bool(false, _) => return Ok(LoweredFingerprint::Disabled),
            Expr::Block(_) => match self.json(value, env, Strings::Plain)? {
                Value::Object(object) => object,
                _ => unreachable!("a block reads as an object"),
            },
            Expr::Call { name, args, .. } if name.text == "script" => {
                let arguments = self.arguments(args, env, |field| {
                    if field == "args" { Strings::Argument } else { Strings::Plain }
                })?;
                Map::from_iter([("script".to_owned(), Value::Object(arguments))])
            }
            other => {
                return Err(self.error(
                    other.span(),
                    "`fingerprint` is `false`, `none`, `{ files = […], ignore = […] }` or `script(command = …, args = […])`.",
                ));
            }
        };
        Fingerprint::try_from(object)
            .map(LoweredFingerprint::Declared)
            .map_err(|error| self.error(value.span(), format!("`fingerprint`: {error}")))
    }

    fn spread_evals(&self, value: &'a Expr, env: &Rc<Env<'a>>) -> Result<Vec<LoweredEval>, LangError> {
        let (value, env) = self.chase(value, env.clone())?;
        let Expr::Block(block) = value else {
            return Err(self.error(value.span(), "`...` in an Artifact spreads a block of `eval` declarations."));
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
                    span,
                } if keyword.text == "eval" => self.eval(name, header.as_ref(), value, &env, *span),
                Entry::Spread(value, _) => Err(self.error(value.span(), "Spread the eval blocks one by one into the Artifact.")),
                _ => Err(self.error(block.span, "A spread eval block holds only `eval` declarations.")),
            })
            .collect()
    }

    fn eval(
        &self,
        id: &Name,
        header: Option<&'a Expr>,
        value: &'a Expr,
        env: &Rc<Env<'a>>,
        span: Span,
    ) -> Result<LoweredEval, LangError> {
        crate::config::identifier(&id.text, "An eval id").map_err(|message| self.error(id.span, message))?;
        if id.text.contains('.') {
            return Err(self.error(id.span, "An eval id is one identifier."));
        }
        let (resolved, resolved_env) = self.chase(value, env.clone())?;
        match (header, resolved) {
            (Some(header), Expr::Block(block)) => {
                let (profile, profile_name) = self.profile(header, env, false)?;
                self.eval_fields(id, span, block, &resolved_env, profile, profile_name, header.span(), None)
            }
            (None, Expr::Macro { name, args, span: call }) => {
                let definition = self.macro_decl(ItemKind::Eval, name)?;
                let _frame = self.enter(*call, &name.text)?;
                let macro_env = bind(definition.item, args, &resolved_env, *call)?;
                let Some(header) = &definition.item.header else {
                    return Err(self.error(definition.item.name.span, "An `eval!` macro names its profile: `eval! name(…) : PROFILE = { … }`."));
                };
                let Expr::Block(block) = &definition.item.body else {
                    return Err(self.error(definition.item.body.span(), "An `eval!` macro's body is a block `{ … }`."));
                };
                let (profile, profile_name) = self.profile(header, &macro_env, false)?;
                self.eval_fields(id, span, block, &macro_env, profile, profile_name, header.span(), Some(format!("{}!", name.text)))
            }
            (None, Expr::Ident(name)) => {
                let decl = self.globals.eval.concrete.get(name.text.as_str()).copied().ok_or_else(|| {
                    self.error(name.span, format!("Unknown eval `{}`; declare it with `eval {} : PROFILE = {{ … }}`.", name.text, name.text))
                })?;
                let mut eval = self.global_eval(id, decl)?;
                eval.span = span;
                Ok(eval)
            }
            (Some(_), other) => Err(self.error(other.span(), "`eval id : PROFILE = …` takes a block `{ … }`.")),
            (None, Expr::Block(_)) => Err(self.error(
                id.span,
                format!("Eval `{}` needs a profile: `eval {} : PROFILE = {{ … }}`, with `human`, `runtime(…)` or a profile's name.", id.text, id.text),
            )),
            (None, other) => Err(self.error(other.span(), "An eval is `eval id : PROFILE = { … }`, `eval id = name!(…)` or `eval id = name`.")),
        }
    }

    /// A top-level `eval`, read in the root scope for the Artifact being declared.
    fn global_eval(&self, id: &Name, decl: Decl<'a>) -> Result<LoweredEval, LangError> {
        let item = decl.item;
        let _visit = self.visit(ItemKind::Eval, &item.name.text, id.span)?;
        let Some(header) = &item.header else {
            return Err(self.error(item.name.span, format!("Eval `{}` needs a profile: `eval {} : PROFILE = {{ … }}`.", item.name.text, item.name.text)));
        };
        let Expr::Block(block) = &item.body else {
            return Err(self.error(item.body.span(), "A top-level eval's body is a block `{ … }`."));
        };
        let root = Env::root();
        let (profile, profile_name) = self.profile(header, &root, false)?;
        self.eval_fields(id, item.span, block, &root, profile, profile_name, header.span(), Some(item.name.text.clone()))
    }

    #[expect(clippy::too_many_arguments, reason = "an eval's parts arrive from three declaration forms")]
    fn eval_fields(
        &self,
        id: &Name,
        span: Span,
        block: &'a Block,
        env: &Rc<Env<'a>>,
        profile: Profile,
        profile_name: Option<String>,
        profile_span: Span,
        source: Option<String>,
    ) -> Result<LoweredEval, LangError> {
        let kind = profile.kind();
        let mut eval = LoweredEval {
            id: id.text.clone(),
            id_span: id.span,
            span,
            title: String::new(),
            mutex: Vec::new(),
            red_at: None,
            profile,
            profile_name,
            profile_span,
            instruction: None,
            inputs: Vec::new(),
            source,
        };
        let mut seen: BTreeMap<&str, Span> = BTreeMap::new();
        for entry in &block.entries {
            let Entry::Field { key, value } = entry else {
                return Err(self.error(block.span, "An eval holds `key = value` fields."));
            };
            if let Some(previous) = seen.insert(&key.text, key.span) {
                return Err(self.error(key.span, format!("Field `{}` is given twice.", key.text)).note(previous, "first given here"));
            }
            let Some((value, env)) = self.optional(value, env)? else {
                continue;
            };
            match key.text.as_str() {
                "title" => eval.title = self.text(value, &env, "`title`")?,
                "red_at" => {
                    if kind != ProfileKind::Agent {
                        return Err(self.error(key.span, "`red_at` is for Agent evals."));
                    }
                    let text = self.text(value, &env, "`red_at`")?;
                    eval.red_at = Some(
                        serde_json::from_value(Value::String(text))
                            .map_err(|_| self.error(value.span(), "`red_at` is \"P0\", \"P1\", \"P2\" or \"P3\"."))?,
                    );
                }
                "mutex" => {
                    if kind == ProfileKind::Human {
                        return Err(self.error(key.span, "`mutex` is for runtime and Agent evals."));
                    }
                    eval.mutex = self
                        .texts(value, &env, "`mutex`")?
                        .into_iter()
                        .map(|tag| tag.parse().map_err(|message: String| self.error(value.span(), message)))
                        .collect::<Result<_, _>>()?;
                    crate::config::mutex::validate(&eval.mutex).map_err(|message| self.error(value.span(), message))?;
                }
                "instruction" => {
                    let pieces = self.template(value, &env, "`instruction`")?;
                    if kind == ProfileKind::Runtime
                        && let Some(slot) = pieces.iter().find_map(|piece| match piece {
                            Piece::Slot(slot) => Some(slot),
                            Piece::Text(_) => None,
                        })
                    {
                        return Err(self.error(
                            slot.span(),
                            "A runtime eval's instruction is not hashed, so it names no Artifact: list them in `inputs` and use `@{path …}` in the command's `args`.",
                        ));
                    }
                    eval.instruction = Some((pieces, value.span()));
                }
                "inputs" => {
                    if kind != ProfileKind::Runtime {
                        return Err(self.error(key.span, "`inputs` is for runtime evals; an Agent or Human eval names what it reads in its instruction."));
                    }
                    eval.inputs = self.artifacts(value, &env, "`inputs`")?;
                }
                other => {
                    return Err(self.error(
                        key.span,
                        format!("Unknown field `{other}`; an eval has `title`, `instruction`, `red_at`, `mutex` and, for a runtime eval, `inputs`."),
                    ));
                }
            }
        }
        if eval.title.trim().is_empty() {
            return Err(self.error(id.span, format!("Eval `{}` needs a nonblank `title`.", id.text)));
        }
        if kind != ProfileKind::Runtime && eval.instruction.is_none() {
            return Err(self.error(id.span, format!("Eval `{}` needs an `instruction`.", id.text)));
        }
        Ok(eval)
    }

    /// A profile in an eval's header (`agent(…)` only inside a `profile` declaration).
    fn profile(
        &self,
        expr: &'a Expr,
        env: &Rc<Env<'a>>,
        declaring: bool,
    ) -> Result<(Profile, Option<String>), LangError> {
        let (expr, env) = self.chase(expr, env.clone())?;
        let json = match expr {
            Expr::Ident(name) if name.text == "human" => json!({"kind": "human"}),
            Expr::Ident(name) => {
                let decl = self.globals.profile.concrete.get(name.text.as_str()).copied().ok_or_else(|| {
                    let mut error = self.error(name.span, format!("Unknown profile `{}`.", name.text));
                    if self.globals.profile.macros.contains_key(name.text.as_str()) {
                        error = error.note(
                            self.globals.profile.macros[name.text.as_str()].item.name.span,
                            format!("`{}` is a `profile!` macro: write `{}!(…)`", name.text, name.text),
                        );
                    }
                    error
                })?;
                let (profile, _) = self.named_profile(&name.text, decl)?;
                return Ok((profile, Some(name.text.clone())));
            }
            Expr::Macro { name, args, span } => {
                let definition = self.macro_decl(ItemKind::Profile, name)?;
                let _frame = self.enter(*span, &name.text)?;
                let macro_env = bind(definition.item, args, &env, *span)?;
                let (profile, _) = self.profile(&definition.item.body, &macro_env, true)?;
                return Ok((profile, Some(format!("{}!", name.text))));
            }
            Expr::Call { name, args, .. } if name.text == "runtime" => {
                let mut object = self.arguments(args, &env, |field| {
                    if field == "args" { Strings::Argument } else { Strings::Plain }
                })?;
                object.insert("kind".into(), json!("runtime"));
                Value::Object(object)
            }
            Expr::Call { name, args, .. } if name.text == "agent" => {
                if !declaring {
                    return Err(self.error(
                        name.span,
                        "An Agent eval refers to a named profile: declare `profile NAME = agent(…)` and write `: NAME`.",
                    ));
                }
                let mut object = self.arguments(args, &env, |_| Strings::Plain)?;
                object.insert("kind".into(), json!("agent"));
                Value::Object(object)
            }
            other => {
                return Err(self.error(
                    other.span(),
                    "Expected a profile: `human`, `runtime(…)`, a profile's name or `name!(…)`.",
                ));
            }
        };
        let profile: Profile = serde_json::from_value(json)
            .map_err(|error| self.error(expr.span(), format!("Profile: {error}")))?;
        profile.check().map_err(|message| self.error(expr.span(), message))?;
        Ok((profile, None))
    }

    fn named_profile(&self, name: &str, decl: Decl<'a>) -> Result<(Profile, Option<String>), LangError> {
        let _visit = self.visit(ItemKind::Profile, name, decl.item.name.span)?;
        if decl.item.header.is_some() {
            return Err(self.error(decl.item.name.span, "A profile has no `: …` header."));
        }
        self.profile(&decl.item.body, &Env::root(), true)
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
                let decl = self.globals.tool.concrete.get(name.text.as_str()).copied().ok_or_else(|| {
                    let known = self.globals.tool.concrete.keys().copied();
                    let mut error = self.error(
                        name.span,
                        format!("Unknown toolset `{}`; the toolsets are {}.", name.text, names(known)),
                    );
                    if self.globals.tool.macros.contains_key(name.text.as_str()) {
                        error = error.note(
                            self.globals.tool.macros[name.text.as_str()].item.name.span,
                            format!("`{}` is a `tool!` macro: write `{}!(…)`", name.text, name.text),
                        );
                    }
                    error
                })?;
                self.named_tool(&name.text, decl)
            }
            Expr::Macro { name, args, span } => {
                let definition = self.macro_decl(ItemKind::Tool, name)?;
                let frame = self.enter(*span, &name.text)?;
                let macro_env = bind(definition.item, args, &env, *span)?;
                let Expr::Block(block) = &definition.item.body else {
                    return Err(self.error(definition.item.body.span(), "A `tool!` macro's body is a block `{ … }` with its `namespace`."));
                };
                let namespace = block
                    .entries
                    .iter()
                    .find_map(|entry| match entry {
                        Entry::Field { key, value } if key.text == "namespace" => Some(value),
                        _ => None,
                    })
                    .ok_or_else(|| self.error(definition.item.name.span, "A `tool!` macro sets its `namespace`."))?;
                let namespace = self.text(namespace, &macro_env, "`namespace`")?;
                let toolset = self.toolset(&namespace, *span, block, macro_env, definition.file, true)?;
                drop(frame);
                Ok(vec![toolset])
            }
            other => Err(self.error(other.span(), "Expected a toolset: a `tool`'s name, `name!(…)`, or `a & b`.")),
        }
    }

    fn named_tool(&self, name: &str, decl: Decl<'a>) -> Result<Vec<Toolset<'a>>, LangError> {
        let _visit = self.visit(ItemKind::Tool, name, decl.item.name.span)?;
        if decl.item.header.is_some() {
            return Err(self.error(decl.item.name.span, "A tool has no `: …` header."));
        }
        match &decl.item.body {
            Expr::Block(block) => Ok(vec![self.toolset(name, decl.item.name.span, block, Env::root(), decl.file, false)?]),
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
        from_macro: bool,
    ) -> Result<Toolset<'a>, LangError> {
        ToolFqn::namespace(namespace).map_err(|message| self.error(span, message))?;
        let plugin = match self.origin(file) {
            Origin::Plugin(plugin) => Some(plugin.clone()),
            Origin::Project => None,
        };
        let mut members = Vec::new();
        for entry in &block.entries {
            match entry {
                Entry::Field { key, .. } if key.text == "namespace" && from_macro => {}
                Entry::Field { key, .. } if key.text == "namespace" => {
                    return Err(self.error(key.span, "A `tool`'s name is its namespace; only a `tool!` macro sets `namespace`."));
                }
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
                    return Err(self.error(key.span, format!("Unknown field `{}` in a toolset.", key.text)));
                }
                Entry::Spread(value, _) => {
                    return Err(self.error(value.span(), "Compose toolsets with `&` instead of spreading them."));
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
                            .error(member.span, format!("Two different {} tools are both named `{fqn}`.", member.audience))
                            .note(*previous, "the other definition"));
                    }
                    None => {}
                }
                let id: ToolFqn = fqn.parse().map_err(|message: String| self.error(member.name.span, message))?;
                let invalid = |error: serde_json::Error| self.error(member.span, format!("Tool `{fqn}`: {error}"));
                match member.audience.as_str() {
                    "agent" => {
                        let tool: AgentTool = serde_json::from_value(value.clone()).map_err(invalid)?;
                        composed.views.agent_tools.insert(id, tool);
                    }
                    "human" => {
                        let tool: HumanTool = serde_json::from_value(value.clone()).map_err(invalid)?;
                        composed.views.human_tools.insert(id, tool);
                    }
                    _ => {
                        let session: ToolSession = serde_json::from_value(value.clone()).map_err(invalid)?;
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
                    let Expr::Call { name: command, args: command_args, .. } = self.chase(&arg.value, env.clone())?.0 else {
                        return Err(self.error(arg.value.span(), "A session's `start` and `stop` are `command(command = …, args = […])`."));
                    };
                    if command.text != "command" {
                        return Err(self.error(command.span, "A session's `start` and `stop` are `command(…)`."));
                    }
                    let command = self.arguments(command_args, &env, |field| {
                        if field == "args" { Strings::Argument } else { Strings::Plain }
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
                let op = object
                    .remove("op")
                    .ok_or_else(|| self.error(*span, "`builtin(…)` names its operation: `op = \"read\"`."))?;
                object.insert("builtin".into(), op);
                object
            }
            ("agent" | "human", "command") => {
                let mut object = self.arguments(args, &env, strings)?;
                if let Some(Value::String(session)) = object.get("session").cloned() {
                    object.insert("session".into(), Value::String(format!("{}.{session}", toolset.namespace)));
                }
                object
            }
            ("human", "viewer") => {
                let Some(PluginSource { namespace, viewer: true }) = &toolset.plugin else {
                    return Err(self.error(*span, "Only a plugin with a viewer entry declares a `viewer(…)` tool."));
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
        for kind in [ItemKind::Artf, ItemKind::Tool, ItemKind::Profile, ItemKind::Eval] {
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
                                    default: param.default.as_ref().map(|default| self.sources.text(default.span()).to_owned()),
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

/// The macros an Artifact's expansion went through, for its Definition.
pub(crate) fn used_names(artifact: &LoweredArtifact) -> BTreeSet<String> {
    artifact.evals.iter().filter_map(|eval| eval.source.clone()).collect()
}
