//! Typed reads of expressions. A value is read where it is used, in the scope it was written
//! in: a parameter stands for its argument read in the caller's scope, so names never capture.

use std::rc::Rc;

use artifactize_tools::artf::lexer::{Part, SlotKind};
use serde_json::{Map, Value};

use super::{
    LangError,
    ast::{Element, Entry, Expr, Span},
    expand::{Expander, Piece, SlotRef, SlotTarget},
    scope::Env,
};

/// How strings become JSON for a constructor's arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Strings {
    /// Text only: `@{param …}` is filled in, no other slot is allowed.
    Plain,
    /// Text that may name its own Artifact: `@{artf self}`.
    Described,
    /// A command argument: one `@{path …}` slot at its end, after an optional `--flag=`.
    Argument,
}

/// The longest chain of macro calls one declaration may expand through.
pub(super) const MAX_DEPTH: usize = 32;
/// Every expression read counts once; this bounds a declaration that would expand
/// exponentially through macros that call each other several times.
pub(super) const MAX_NODES: usize = 1_000_000;

impl<'a> Expander<'a> {
    pub(super) fn tick(&self, span: Span) -> Result<(), LangError> {
        let nodes = self.nodes.get() + 1;
        self.nodes.set(nodes);
        if nodes > MAX_NODES {
            return Err(self.error(
                span,
                format!("Expanding the project reads more than {MAX_NODES} expressions; a macro expands too much."),
            ));
        }
        Ok(())
    }

    /// Follow parameters to the expression they stand for and the scope to read it in.
    pub(super) fn chase(
        &self,
        mut expr: &'a Expr,
        mut env: Rc<Env<'a>>,
    ) -> Result<(&'a Expr, Rc<Env<'a>>), LangError> {
        loop {
            self.tick(expr.span())?;
            let Expr::Ident(name) = expr else {
                return Ok((expr, env));
            };
            let Some(bound) = env.get(&name.text) else {
                return Ok((expr, env));
            };
            let bound = bound.clone();
            expr = bound.expr;
            env = bound.env;
        }
    }

    /// A string with its slots, parameters filled in and other slots kept typed.
    pub(super) fn template(
        &self,
        expr: &'a Expr,
        env: &Rc<Env<'a>>,
        what: &str,
    ) -> Result<Vec<Piece>, LangError> {
        let (expr, env) = self.chase(expr, env.clone())?;
        match expr {
            Expr::Str(literal, _) => {
                let mut pieces = Vec::new();
                for part in &literal.parts {
                    match part {
                        Part::Text(text) => pieces.push(Piece::Text(text.clone())),
                        Part::Slot(slot) => {
                            let span = self.span(slot.span.clone(), expr.span());
                            let word = |index: usize| -> (String, Span) {
                                let (text, range) = &slot.words[index];
                                (text.clone(), self.span(range.clone(), expr.span()))
                            };
                            match slot.kind {
                                SlotKind::Param => {
                                    let (name, name_span) = word(0);
                                    let bound = env.get(&name).cloned().ok_or_else(|| {
                                        self.error(
                                            name_span,
                                            format!("`@{{param {name}}}` names no parameter in scope."),
                                        )
                                    })?;
                                    pieces.extend(self.spliced(bound.expr, &bound.env, &name, name_span)?);
                                }
                                SlotKind::Artf => {
                                    let (target, target_span) = word(0);
                                    pieces.push(Piece::Slot(SlotRef::Artf {
                                        target: self.slot_target(&target, target_span)?,
                                        span,
                                    }));
                                }
                                SlotKind::Path => {
                                    let (text, target_span) = word(0);
                                    let (target, path) =
                                        text.split_once('/').unwrap_or((text.as_str(), ""));
                                    if text.ends_with('/') {
                                        return Err(self.error(
                                            target_span,
                                            format!("`@{{path {text}}}`: write the Artifact as `@{{path {target}}}`, or a path after the `/`."),
                                        ));
                                    }
                                    artifactize_tools::scope::logical_path(path).map_err(|error| {
                                        self.error(target_span, format!("`@{{path {text}}}`: {error}"))
                                    })?;
                                    let start = target_span.range().start;
                                    let name_span =
                                        Span::new(target_span.file, start..start + target.len());
                                    pieces.push(Piece::Slot(SlotRef::Path {
                                        target: self.slot_target(target, name_span)?,
                                        path: path.to_owned(),
                                        span,
                                    }));
                                }
                                SlotKind::Tool => {
                                    let (target, target_span) = word(0);
                                    let (audience, audience_span) = word(1);
                                    let (tool, _) = word(2);
                                    if !matches!(audience.as_str(), "agent" | "human") {
                                        return Err(self.error(
                                            audience_span,
                                            "A tool slot's audience is `agent` or `human`.",
                                        ));
                                    }
                                    pieces.push(Piece::Slot(SlotRef::Tool {
                                        target: self.slot_target(&target, target_span)?,
                                        audience,
                                        tool,
                                        span,
                                    }));
                                }
                            }
                        }
                    }
                }
                Ok(pieces)
            }
            Expr::Number(number, _) => Ok(vec![Piece::Text(number.clone())]),
            Expr::Bool(value, _) => Ok(vec![Piece::Text(value.to_string())]),
            other => Err(self.error(other.span(), format!("{what} takes a string."))),
        }
    }

    /// A parameter's value spliced into a string: its own pieces, never read for slots again.
    fn spliced(
        &self,
        expr: &'a Expr,
        env: &Rc<Env<'a>>,
        name: &str,
        span: Span,
    ) -> Result<Vec<Piece>, LangError> {
        let (resolved, resolved_env) = self.chase(expr, env.clone())?;
        match resolved {
            Expr::Str(..) | Expr::Number(..) | Expr::Bool(..) => {
                self.template(resolved, &resolved_env, "A parameter in a string")
            }
            Expr::Ident(global) => Ok(vec![Piece::Text(global.text.clone())]),
            other => Err(self
                .error(span, format!("`@{{param {name}}}` is not text, so it cannot fill a string."))
                .note(other.span(), "its value")),
        }
    }

    fn slot_target(&self, word: &str, span: Span) -> Result<SlotTarget, LangError> {
        if word == "self" {
            return Ok(SlotTarget::SelfRef);
        }
        crate::config::validation::artifact_name(word, "An Artifact name")
            .map_err(|message| self.error(span, message))?;
        self.names.borrow_mut().insert((span, word.to_owned()));
        Ok(SlotTarget::Named(word.to_owned(), span))
    }

    /// A span inside a string literal: the slot's own range in the literal's file.
    fn span(&self, range: std::ops::Range<usize>, literal: Span) -> Span {
        Span::new(literal.file, range)
    }

    /// Text without slots other than parameters.
    pub(super) fn text(&self, expr: &'a Expr, env: &Rc<Env<'a>>, what: &str) -> Result<String, LangError> {
        let pieces = self.template(expr, env, what)?;
        self.plain(pieces, what)
    }

    pub(super) fn plain(&self, pieces: Vec<Piece>, what: &str) -> Result<String, LangError> {
        pieces
            .into_iter()
            .map(|piece| match piece {
                Piece::Text(text) => Ok(text),
                Piece::Slot(slot) => Err(self.error(
                    slot.span(),
                    format!("{what} takes text; `@{{artf}}`, `@{{path}}` and `@{{tool}}` slots belong in instructions and arguments."),
                )),
            })
            .collect()
    }

    /// `none` means the field is not given.
    pub(super) fn optional(
        &self,
        expr: &'a Expr,
        env: &Rc<Env<'a>>,
    ) -> Result<Option<(&'a Expr, Rc<Env<'a>>)>, LangError> {
        let (expr, env) = self.chase(expr, env.clone())?;
        Ok((!matches!(expr, Expr::None(_))).then_some((expr, env)))
    }

    pub(super) fn boolean(&self, expr: &'a Expr, env: &Rc<Env<'a>>, what: &str) -> Result<bool, LangError> {
        match self.chase(expr, env.clone())?.0 {
            Expr::Bool(value, _) => Ok(*value),
            other => Err(self.error(other.span(), format!("{what} takes `true` or `false`."))),
        }
    }

    /// The items of a list, its spreads flattened, each with its scope.
    pub(super) fn list(
        &self,
        expr: &'a Expr,
        env: &Rc<Env<'a>>,
        what: &str,
    ) -> Result<Vec<(&'a Expr, Rc<Env<'a>>)>, LangError> {
        let (expr, env) = self.chase(expr, env.clone())?;
        let Expr::Array(elements, _) = expr else {
            return Err(self.error(expr.span(), format!("{what} takes a list `[…]`.")));
        };
        let mut items = Vec::new();
        for element in elements {
            match element {
                Element::Value(value) => items.push((value, env.clone())),
                Element::Spread(value, _) => items.extend(self.list(value, &env, what)?),
            }
        }
        Ok(items)
    }

    pub(super) fn texts(&self, expr: &'a Expr, env: &Rc<Env<'a>>, what: &str) -> Result<Vec<String>, LangError> {
        self.list(expr, env, what)?
            .into_iter()
            .map(|(item, env)| self.text(item, &env, what))
            .collect()
    }

    /// Artifacts named by identifier: `[engine-core, ...deps]`.
    pub(super) fn artifacts(
        &self,
        expr: &'a Expr,
        env: &Rc<Env<'a>>,
        what: &str,
    ) -> Result<Vec<(String, Span)>, LangError> {
        self.list(expr, env, what)?
            .into_iter()
            .map(|(item, env)| {
                let (item, _) = self.chase(item, env)?;
                match item {
                    Expr::Ident(name) if !name.text.contains('.') => {
                        if !self.globals.artf.concrete.contains_key(name.text.as_str()) {
                            let mut error = self.error(name.span, format!("Unknown Artifact `{}`.", name.text));
                            if self.globals.artf.macros.contains_key(name.text.as_str()) {
                                error = error.note(
                                    self.globals.artf.macros[name.text.as_str()].item.name.span,
                                    format!("`{}` is an `artf!` macro; bind an Artifact with `artf NAME = {}!(…)`", name.text, name.text),
                                );
                            }
                            return Err(error);
                        }
                        self.names.borrow_mut().insert((name.span, name.text.clone()));
                        Ok((name.text.clone(), name.span))
                    }
                    other => Err(self.error(other.span(), format!("{what} lists Artifacts by name."))),
                }
            })
            .collect()
    }

    /// JSON for a constructor's arguments or a schema: blocks are objects and `none` leaves a
    /// field out.
    pub(super) fn json(&self, expr: &'a Expr, env: &Rc<Env<'a>>, strings: Strings) -> Result<Value, LangError> {
        let (expr, env) = self.chase(expr, env.clone())?;
        Ok(match expr {
            Expr::Str(..) => Value::String(self.string(expr, &env, strings)?),
            Expr::Number(number, span) => Value::Number(
                number
                    .parse::<serde_json::Number>()
                    .map_err(|_| self.error(*span, "This number has no JSON form."))?,
            ),
            Expr::Bool(value, _) => Value::Bool(*value),
            Expr::None(span) => {
                return Err(self.error(*span, "`none` leaves a field out; it is not a value in a list."));
            }
            Expr::Array(..) => Value::Array(
                self.list(expr, &env, "This value")?
                    .into_iter()
                    .map(|(item, env)| self.json(item, &env, strings))
                    .collect::<Result<_, _>>()?,
            ),
            Expr::Block(block) => {
                let mut object = Map::new();
                for entry in &block.entries {
                    let Entry::Field { key, value } = entry else {
                        return Err(self.error(block.span, "An object holds `key = value` fields."));
                    };
                    if let Some((value, env)) = self.optional(value, &env)? {
                        if object
                            .insert(key.text.clone(), self.json(value, &env, strings)?)
                            .is_some()
                        {
                            return Err(self.error(key.span, format!("Field `{}` is given twice.", key.text)));
                        }
                    }
                }
                Value::Object(object)
            }
            Expr::Ident(name) => Value::String(name.text.clone()),
            other => {
                return Err(self.error(other.span(), "Expected a string, number, boolean, list or object."));
            }
        })
    }

    /// A string as a constructor argument: an `Argument` keeps its one path slot in the
    /// `@{path NAME/…}` form the declaration types parse.
    pub(super) fn string(&self, expr: &'a Expr, env: &Rc<Env<'a>>, strings: Strings) -> Result<String, LangError> {
        let pieces = self.template(expr, env, "This value")?;
        let mut text = String::new();
        for piece in pieces {
            match (piece, strings) {
                (Piece::Text(part), Strings::Argument) => text.push_str(&part.replace("@{", "@@{")),
                (Piece::Text(part), _) => text.push_str(&part),
                (Piece::Slot(SlotRef::Artf { target, span }), Strings::Described) => {
                    text.push_str(&self.bound(&target, span)?);
                }
                (Piece::Slot(SlotRef::Path { target, path, span }), Strings::Argument) => {
                    let name = self.bound(&target, span)?;
                    text.push_str("@{path ");
                    text.push_str(&name);
                    if !path.is_empty() {
                        text.push('/');
                        text.push_str(&path);
                    }
                    text.push('}');
                }
                (Piece::Slot(slot), _) => {
                    return Err(self.error(
                        slot.span(),
                        match strings {
                            Strings::Argument => "An argument takes a `@{path …}` slot, not this one.",
                            Strings::Described => "A description may name its Artifact with `@{artf self}` only.",
                            Strings::Plain => "This value takes text; slots belong in instructions and arguments.",
                        },
                    ));
                }
            }
        }
        Ok(text)
    }

    /// The Artifact a slot names, `self` bound to the one being expanded.
    pub(super) fn bound(&self, target: &SlotTarget, span: Span) -> Result<String, LangError> {
        match target {
            SlotTarget::Named(name, _) => Ok(name.clone()),
            SlotTarget::SelfRef => self
                .current()
                .ok_or_else(|| self.error(span, "`self` names the Artifact being declared; there is none here.")),
        }
    }

    /// Named arguments of a constructor as one JSON object; `none` leaves an argument out.
    pub(super) fn arguments(
        &self,
        args: &'a [super::ast::Arg],
        env: &Rc<Env<'a>>,
        strings: impl Fn(&str) -> Strings,
    ) -> Result<Map<String, Value>, LangError> {
        let mut object = Map::new();
        for arg in args {
            if let Some((value, env)) = self.optional(&arg.value, env)? {
                if object
                    .insert(arg.name.text.clone(), self.json(value, &env, strings(&arg.name.text))?)
                    .is_some()
                {
                    return Err(self.error(arg.name.span, format!("Argument `{}` is given twice.", arg.name.text)));
                }
            }
        }
        Ok(object)
    }
}
