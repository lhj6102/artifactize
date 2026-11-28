//! The syntax tree of `.artf` and `.artfprofiles` files, every node with the span it was
//! written at.

use artifactize_tools::artf::lexer::Literal;

/// Which `.artf` file a span lies in: an index into the files read together.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(pub u32);

/// A byte range in one file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(file: FileId, range: std::ops::Range<usize>) -> Self {
        let bound =
            |offset: usize| u32::try_from(offset).expect("a definition file is under 4 GiB");
        Self {
            file,
            start: bound(range.start),
            end: bound(range.end),
        }
    }

    pub fn to(self, end: Self) -> Self {
        Self {
            file: self.file,
            start: self.start,
            end: end.end.max(self.end),
        }
    }

    pub fn range(self) -> std::ops::Range<usize> {
        self.start as usize..self.end as usize
    }
}

/// The top-level declarations of an `.artf` file. Their names share one namespace with the
/// profiles of `.artfprofiles` files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ItemKind {
    Artf,
    Tool,
}

impl ItemKind {
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Artf => "artf",
            Self::Tool => "tool",
        }
    }
}

/// A name as written: an identifier, or `.`-joined identifiers for a tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name {
    pub text: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: Name,
    pub default: Option<Expr>,
}

/// `kind name = body`, `kind name : header = body`, or an `artf! name(params) …` macro.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub kind: ItemKind,
    pub params: Option<Vec<Param>>,
    pub name: Name,
    pub header: Option<Expr>,
    pub body: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arg {
    pub name: Name,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Str(Literal, Span),
    Number(String, Span),
    Bool(bool, Span),
    /// A parameter or a global, by name.
    Ident(Name),
    Array(Vec<Element>, Span),
    Block(Block),
    /// A built-in constructor: `builtin(…)`, `command(…)`, `runtime(…)`, `script(…)`.
    Call {
        name: Name,
        args: Vec<Arg>,
        span: Span,
    },
    /// An `artf!` macro invocation `name!(…)`.
    Macro {
        name: Name,
        args: Vec<Arg>,
        span: Span,
    },
    /// Tool composition `a & b`.
    And(Vec<Expr>, Span),
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Self::Str(_, span)
            | Self::Number(_, span)
            | Self::Bool(_, span)
            | Self::Array(_, span)
            | Self::Call { span, .. }
            | Self::Macro { span, .. }
            | Self::And(_, span) => *span,
            Self::Ident(name) => name.span,
            Self::Block(block) => block.span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Element {
    Value(Expr),
    Spread(Expr, Span),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub entries: Vec<Entry>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    /// `key = value`; a key is an identifier or a string.
    Field {
        key: Name,
        value: Expr,
    },
    /// `eval id : profile = body` or `agent|human|session name = tool`.
    Declare {
        keyword: Name,
        name: Name,
        header: Option<Expr>,
        value: Expr,
        span: Span,
    },
    Spread(Expr, Span),
}

/// One profile of an `.artfprofiles` file: `profile NAME : agent = { … }`, or
/// `profile NAME : agent = match VAR { default => { … }, LABEL => { … } }`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileItem {
    pub name: Name,
    /// What evaluates with it, after `:`.
    pub kind: Name,
    pub body: ProfileBody,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProfileBody {
    /// One complete setting.
    Setting(Block),
    /// A complete setting per value of an environment variable, `default` among them.
    Match {
        variable: Name,
        arms: Vec<(Name, Block)>,
    },
}
