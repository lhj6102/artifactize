//! The project scope: every `artf`, `artf!`, `tool` and profile name in one namespace, read
//! from every file before anything resolves. Parameters shadow globals inside a macro's body;
//! arguments are read in their caller's scope.

use std::{collections::BTreeMap, rc::Rc};

use super::{
    LangError, Origin, Sources,
    ast::{Arg, Expr, FileId, Item, ItemKind, ProfileItem, Span},
};

/// A declaration with the file it came from.
#[derive(Clone, Copy)]
pub(super) struct Decl<'a> {
    pub item: &'a Item,
    pub file: FileId,
}

/// A profile with the file it came from.
#[derive(Clone, Copy)]
pub(super) struct ProfileDecl<'a> {
    pub item: &'a ProfileItem,
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
    pub profiles: BTreeMap<&'a str, ProfileDecl<'a>>,
}

impl<'a> Globals<'a> {
    pub fn kind(&self, kind: ItemKind) -> &Namespace<'a> {
        match kind {
            ItemKind::Artf => &self.artf,
            ItemKind::Tool => &self.tool,
        }
    }

    fn kind_mut(&mut self, kind: ItemKind) -> &mut Namespace<'a> {
        match kind {
            ItemKind::Artf => &mut self.artf,
            ItemKind::Tool => &mut self.tool,
        }
    }

    /// Pass one: every declaration of every file. A name is declared once across every kind,
    /// so what follows `:` or `&` reads unambiguously where it stands.
    pub fn collect(
        sources: &Sources,
        parsed: &'a [(FileId, Vec<Item>)],
        profiles: &'a [(FileId, Vec<ProfileItem>)],
    ) -> Result<Self, LangError> {
        let mut globals = Self::default();
        let mut taken: BTreeMap<&'a str, (String, Span)> = BTreeMap::new();
        let mut claim = |name: &'a str, what: String, span: Span| -> Result<(), LangError> {
            match taken.insert(name, (what.clone(), span)) {
                Some((previous, previous_span)) => Err(LangError::new(
                    span,
                    format!("`{name}` is already declared as {previous}; a name is declared once across `artf`, `tool` and profiles."),
                )
                .note(previous_span, format!("the {previous}"))),
                None => Ok(()),
            }
        };
        for (file, items) in parsed {
            let origin = &sources.file(*file).origin;
            for item in items {
                if let Origin::Plugin(plugin) = origin {
                    plugin_item(item, &plugin.namespace)?;
                }
                let is_macro = item.params.is_some();
                let bang = if is_macro { "!" } else { "" };
                claim(
                    &item.name.text,
                    format!("`{}{bang} {}`", item.kind.keyword(), item.name.text),
                    item.name.span,
                )?;
                let namespace = globals.kind_mut(item.kind);
                let table = if is_macro {
                    &mut namespace.macros
                } else {
                    &mut namespace.concrete
                };
                table.insert(item.name.text.as_str(), Decl { item, file: *file });
            }
        }
        for (file, items) in profiles {
            for item in items {
                claim(
                    &item.name.text,
                    format!("`profile {}`", item.name.text),
                    item.name.span,
                )?;
                globals
                    .profiles
                    .insert(item.name.text.as_str(), ProfileDecl { item, file: *file });
            }
        }
        Ok(globals)
    }
}

/// A plugin contributes its namespace's toolsets and `artf!` macros, nothing project-specific.
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
