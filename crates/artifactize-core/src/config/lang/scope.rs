//! The project scope: one namespace per kind of declaration, each holding concrete bindings and
//! macros apart, read from every file before anything resolves. Parameters shadow globals
//! inside a macro's body; arguments are read in their caller's scope.

use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use super::{
    LangError, Origin, Sources,
    ast::{Arg, Block, Element, Entry, Expr, FileId, Item, ItemKind, Span},
};

/// A declaration with the file it came from.
#[derive(Clone, Copy)]
pub(super) struct Decl<'a> {
    pub item: &'a Item,
    pub file: FileId,
}

#[derive(Default)]
pub(super) struct Namespace<'a> {
    pub concrete: BTreeMap<&'a str, Decl<'a>>,
    pub macros: BTreeMap<&'a str, Decl<'a>>,
}

#[derive(Default)]
pub(super) struct Globals<'a> {
    pub artf: Namespace<'a>,
    pub tool: Namespace<'a>,
    pub profile: Namespace<'a>,
    pub eval: Namespace<'a>,
}

impl<'a> Globals<'a> {
    pub fn kind(&self, kind: ItemKind) -> &Namespace<'a> {
        match kind {
            ItemKind::Artf => &self.artf,
            ItemKind::Tool => &self.tool,
            ItemKind::Profile => &self.profile,
            ItemKind::Eval => &self.eval,
        }
    }

    fn kind_mut(&mut self, kind: ItemKind) -> &mut Namespace<'a> {
        match kind {
            ItemKind::Artf => &mut self.artf,
            ItemKind::Tool => &mut self.tool,
            ItemKind::Profile => &mut self.profile,
            ItemKind::Eval => &mut self.eval,
        }
    }

    /// Pass one: every declaration of every file, duplicates rejected with both places.
    pub fn collect(
        sources: &Sources,
        parsed: &'a [(FileId, Vec<Item>)],
    ) -> Result<Self, LangError> {
        let mut globals = Self::default();
        for (file, items) in parsed {
            let origin = &sources.file(*file).origin;
            for item in items {
                if let Origin::Plugin(plugin) = origin {
                    plugin_item(item, &plugin.namespace)?;
                }
                let is_macro = item.params.is_some();
                let namespace = globals.kind_mut(item.kind);
                let table = if is_macro {
                    &mut namespace.macros
                } else {
                    &mut namespace.concrete
                };
                if let Some(previous) =
                    table.insert(item.name.text.as_str(), Decl { item, file: *file })
                {
                    let bang = if is_macro { "!" } else { "" };
                    return Err(LangError::new(
                        item.name.span,
                        format!(
                            "`{}{bang} {}` is declared twice.",
                            item.kind.keyword(),
                            item.name.text
                        ),
                    )
                    .note(previous.item.name.span, "first declared here"));
                }
            }
        }
        globals.acyclic()?;
        Ok(globals)
    }

    /// Macros expand statically: no macro may reach itself through the macros it calls.
    fn acyclic(&self) -> Result<(), LangError> {
        let mut calls: CallGraph<'_> = BTreeMap::new();
        for kind in [
            ItemKind::Artf,
            ItemKind::Tool,
            ItemKind::Profile,
            ItemKind::Eval,
        ] {
            for (name, decl) in &self.kind(kind).macros {
                let mut found = Vec::new();
                item_calls(decl.item, &mut found);
                let resolved = found
                    .into_iter()
                    .flat_map(|(expected, callee, span)| {
                        let kinds: Vec<ItemKind> = match expected {
                            Some(kind) => vec![kind],
                            None => vec![
                                ItemKind::Artf,
                                ItemKind::Tool,
                                ItemKind::Profile,
                                ItemKind::Eval,
                            ],
                        };
                        kinds
                            .into_iter()
                            .filter(|kind| self.kind(*kind).macros.contains_key(callee))
                            .map(move |kind| (kind, callee, span))
                            .collect::<Vec<_>>()
                    })
                    .collect();
                calls.insert((kind, name), resolved);
            }
        }
        let mut done = BTreeSet::new();
        for start in calls.keys() {
            let mut stack = Vec::new();
            visit(*start, &calls, &mut done, &mut stack)?;
        }
        Ok(())
    }
}

type CallGraph<'a> = BTreeMap<(ItemKind, &'a str), Vec<(ItemKind, &'a str, Span)>>;

fn visit<'a>(
    node: (ItemKind, &'a str),
    calls: &CallGraph<'a>,
    done: &mut BTreeSet<(ItemKind, &'a str)>,
    stack: &mut Vec<((ItemKind, &'a str), Span)>,
) -> Result<(), LangError> {
    if done.contains(&node) {
        return Ok(());
    }
    for (kind, callee, span) in calls.get(&node).into_iter().flatten() {
        let next = (*kind, *callee);
        if next == node || stack.iter().any(|(entry, _)| *entry == next) {
            let mut error = LangError::new(
                *span,
                format!("Macro `{callee}!` calls itself; a macro expands once, without recursion."),
            );
            for (frame, call) in stack.iter().rev() {
                error = error.note(*call, format!("called from `{}!`", frame.1));
            }
            return Err(error);
        }
        stack.push((node, *span));
        visit(next, calls, done, stack)?;
        stack.pop();
    }
    done.insert(node);
    Ok(())
}

/// The macro calls in an item, each with the kind its position expects when that is known.
fn item_calls<'a>(item: &'a Item, found: &mut Vec<(Option<ItemKind>, &'a str, Span)>) {
    for param in item.params.iter().flatten() {
        if let Some(default) = &param.default {
            expr_calls(default, None, found);
        }
    }
    let header = match item.kind {
        ItemKind::Artf => Some(ItemKind::Tool),
        ItemKind::Eval => Some(ItemKind::Profile),
        ItemKind::Tool | ItemKind::Profile => None,
    };
    if let Some(expr) = &item.header {
        expr_calls(expr, header, found);
    }
    let body = match item.kind {
        ItemKind::Artf => Some(ItemKind::Artf),
        ItemKind::Tool => Some(ItemKind::Tool),
        ItemKind::Profile => Some(ItemKind::Profile),
        ItemKind::Eval => Some(ItemKind::Eval),
    };
    expr_calls(&item.body, body, found);
}

fn expr_calls<'a>(
    expr: &'a Expr,
    expected: Option<ItemKind>,
    found: &mut Vec<(Option<ItemKind>, &'a str, Span)>,
) {
    match expr {
        Expr::Macro { name, args, span } => {
            found.push((expected, &name.text, *span));
            args_calls(args, found);
        }
        Expr::Call { args, .. } => args_calls(args, found),
        Expr::And(terms, _) => {
            for term in terms {
                expr_calls(term, Some(ItemKind::Tool), found);
            }
        }
        Expr::Array(elements, _) => {
            for element in elements {
                match element {
                    Element::Value(value) | Element::Spread(value, _) => {
                        expr_calls(value, None, found);
                    }
                }
            }
        }
        Expr::Block(block) => block_calls(block, found),
        Expr::Str(..) | Expr::Number(..) | Expr::Bool(..) | Expr::None(_) | Expr::Ident(_) => {}
    }
}

fn args_calls<'a>(args: &'a [Arg], found: &mut Vec<(Option<ItemKind>, &'a str, Span)>) {
    for arg in args {
        expr_calls(&arg.value, None, found);
    }
}

fn block_calls<'a>(block: &'a Block, found: &mut Vec<(Option<ItemKind>, &'a str, Span)>) {
    for entry in &block.entries {
        match entry {
            Entry::Field { value, .. } | Entry::Spread(value, _) => expr_calls(value, None, found),
            Entry::Declare {
                keyword,
                header,
                value,
                ..
            } => {
                let eval = keyword.text == "eval";
                if let Some(header) = header {
                    expr_calls(header, eval.then_some(ItemKind::Profile), found);
                }
                expr_calls(value, eval.then_some(ItemKind::Eval), found);
            }
        }
    }
}

/// A plugin contributes its namespace's toolsets and macros, nothing project-specific.
fn plugin_item(item: &Item, namespace: &str) -> Result<(), LangError> {
    let within = item.name.text == namespace
        || item
            .name
            .text
            .strip_prefix(namespace)
            .is_some_and(|rest| rest.starts_with('.'));
    match (item.kind, item.params.is_some()) {
        (_, true) => Ok(()),
        (ItemKind::Tool, false) if within => Ok(()),
        (ItemKind::Tool, false) => Err(LangError::new(
            item.name.span,
            format!("A plugin declares tools under its namespace `{namespace}`."),
        )),
        (kind, false) => Err(LangError::new(
            item.name.span,
            format!(
                "A plugin declares its toolset and macros; a concrete `{}` belongs to the project.",
                kind.keyword()
            ),
        )),
    }
}

/// A parameter's value: its expression and the scope to read it in.
#[derive(Clone)]
pub(super) struct Bound<'a> {
    pub expr: &'a Expr,
    pub env: Rc<Env<'a>>,
}

/// The parameters in scope. The root scope has none: identifiers there are globals.
#[derive(Default)]
pub(super) struct Env<'a> {
    pub params: BTreeMap<&'a str, Bound<'a>>,
}

impl<'a> Env<'a> {
    pub fn root() -> Rc<Self> {
        Rc::new(Self::default())
    }

    pub fn get(&self, name: &str) -> Option<&Bound<'a>> {
        self.params.get(name)
    }
}

/// Bind a macro's parameters to a call's arguments; defaults read in the root scope.
pub(super) fn bind<'a>(
    definition: &'a Item,
    args: &'a [Arg],
    caller: &Rc<Env<'a>>,
    call: Span,
) -> Result<Rc<Env<'a>>, LangError> {
    let params = definition.params.as_deref().unwrap_or_default();
    let mut bound = BTreeMap::new();
    for arg in args {
        if !params.iter().any(|param| param.name.text == arg.name.text) {
            return Err(LangError::new(
                arg.name.span,
                format!(
                    "`{}!` has no parameter `{}`; its parameters are {}.",
                    definition.name.text,
                    arg.name.text,
                    names(params.iter().map(|param| param.name.text.as_str()))
                ),
            )
            .note(definition.name.span, "the macro is declared here"));
        }
        if bound
            .insert(
                arg.name.text.as_str(),
                Bound {
                    expr: &arg.value,
                    env: caller.clone(),
                },
            )
            .is_some()
        {
            return Err(LangError::new(
                arg.name.span,
                format!("Argument `{}` is given twice.", arg.name.text),
            ));
        }
    }
    for param in params {
        if bound.contains_key(param.name.text.as_str()) {
            continue;
        }
        let Some(default) = &param.default else {
            return Err(LangError::new(
                call,
                format!(
                    "`{}!` needs the argument `{}`, which has no default.",
                    definition.name.text, param.name.text
                ),
            )
            .note(param.name.span, "the parameter is declared here"));
        };
        bound.insert(
            param.name.text.as_str(),
            Bound {
                expr: default,
                env: Env::root(),
            },
        );
    }
    Ok(Rc::new(Env { params: bound }))
}

pub(super) fn names<'b>(names: impl Iterator<Item = &'b str>) -> String {
    let listed: Vec<_> = names.map(|name| format!("`{name}`")).collect();
    if listed.is_empty() {
        "none".into()
    } else {
        listed.join(", ")
    }
}
