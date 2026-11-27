//! Parse `.artf` and `.artfprofiles` tokens into items. Only the syntax is checked here; names
//! and kinds resolve once every file has been read.

use super::ast::*;
use artifactize_tools::artf::lexer::{self, Kind, Token};

/// Names a binding may not take: they read as literals or as the enclosing Artifact.
const RESERVED: [&str; 4] = ["self", "none", "true", "false"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub span: Span,
    pub message: String,
}

pub fn parse(file: FileId, source: &str) -> Result<Vec<Item>, ParseError> {
    let mut parser = Parser::new(file, source)?;
    let mut items = Vec::new();
    while parser.peek().is_some() {
        items.push(parser.item()?);
    }
    Ok(items)
}

/// An `.artfprofiles` file: `profile` declarations only.
pub fn parse_profiles(file: FileId, source: &str) -> Result<Vec<ProfileItem>, ParseError> {
    let mut parser = Parser::new(file, source)?;
    let mut items = Vec::new();
    while parser.peek().is_some() {
        items.push(parser.profile()?);
    }
    Ok(items)
}

struct Parser {
    file: FileId,
    tokens: Vec<Token>,
    position: usize,
    end: usize,
}

impl Parser {
    fn new(file: FileId, source: &str) -> Result<Self, ParseError> {
        let lexed = lexer::lex(source).map_err(|error| ParseError {
            span: Span::new(file, error.offset..error.offset),
            message: error.message,
        })?;
        Ok(Self {
            file,
            tokens: lexed.tokens,
            position: 0,
            end: source.len(),
        })
    }

    fn peek(&self) -> Option<&Kind> {
        self.tokens.get(self.position).map(|token| &token.kind)
    }

    fn peek_at(&self, ahead: usize) -> Option<&Kind> {
        self.tokens
            .get(self.position + ahead)
            .map(|token| &token.kind)
    }

    fn span_here(&self) -> Span {
        self.tokens.get(self.position).map_or_else(
            || Span::new(self.file, self.end..self.end),
            |token| Span::new(self.file, token.span.clone()),
        )
    }

    fn previous_span(&self) -> Span {
        let token = &self.tokens[self.position - 1];
        Span::new(self.file, token.span.clone())
    }

    fn error<T>(&self, expected: &str) -> Result<T, ParseError> {
        let found = self
            .peek()
            .map_or_else(|| "the end of the file".to_owned(), Kind::describe);
        Err(ParseError {
            span: self.span_here(),
            message: format!("Expected {expected}, found {found}."),
        })
    }

    fn eat(&mut self, kind: &Kind) -> bool {
        if self.peek() == Some(kind) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: &Kind, expected: &str) -> Result<Span, ParseError> {
        if self.eat(kind) {
            Ok(self.previous_span())
        } else {
            self.error(expected)
        }
    }

    fn ident(&mut self, expected: &str) -> Result<Name, ParseError> {
        match self.peek() {
            Some(Kind::Ident(text)) => {
                let text = text.clone();
                self.position += 1;
                Ok(Name {
                    text,
                    span: self.previous_span(),
                })
            }
            _ => self.error(expected),
        }
    }

    /// `a.b.c`, joined without spaces around its dots.
    fn dotted(&mut self, expected: &str) -> Result<Name, ParseError> {
        let mut name = self.ident(expected)?;
        while self.peek() == Some(&Kind::Dot) && matches!(self.peek_at(1), Some(Kind::Ident(_))) {
            self.position += 1;
            let part = self.ident("a name after `.`")?;
            name.text.push('.');
            name.text.push_str(&part.text);
            name.span = name.span.to(part.span);
        }
        Ok(name)
    }

    fn binding(&mut self, kind: ItemKind) -> Result<Name, ParseError> {
        let name = if kind == ItemKind::Tool {
            self.dotted("the tool's name")?
        } else {
            self.ident("a name")?
        };
        if name.text.split('.').any(|part| RESERVED.contains(&part)) {
            return Err(ParseError {
                span: name.span,
                message: format!("`{}` is reserved and cannot name a declaration.", name.text),
            });
        }
        Ok(name)
    }

    fn item(&mut self) -> Result<Item, ParseError> {
        let start = self.span_here();
        let kind = match self.peek() {
            Some(Kind::Ident(word)) => match word.as_str() {
                "artf" => ItemKind::Artf,
                "tool" => ItemKind::Tool,
                "eval" => {
                    return Err(ParseError {
                        span: start,
                        message: "An eval is declared inside the `artf` block it judges: `eval ID : PROFILE = { … }`.".into(),
                    });
                }
                "profile" => {
                    return Err(ParseError {
                        span: start,
                        message: "Profiles are declared in `.artfprofiles` files: `profile NAME : agent = { … }`.".into(),
                    });
                }
                _ => return self.error("a declaration: `artf` or `tool`"),
            },
            _ => return self.error("a declaration: `artf` or `tool`"),
        };
        self.position += 1;
        let is_macro = self.eat(&Kind::Bang);
        if is_macro && kind == ItemKind::Tool {
            return Err(ParseError {
                span: start.to(self.previous_span()),
                message: "There are no `tool!` macros: declare a toolset once with `tool NAME = { … }` and compose it with `&`.".into(),
            });
        }
        let name = self.binding(kind)?;
        let params = if is_macro { Some(self.params()?) } else { None };
        let header = if self.eat(&Kind::Colon) {
            Some(self.expr()?)
        } else {
            None
        };
        self.expect(&Kind::Eq, "`=`")?;
        let body = self.expr()?;
        Ok(Item {
            kind,
            params,
            name,
            header,
            span: start.to(body.span()),
            body,
        })
    }

    fn params(&mut self) -> Result<Vec<Param>, ParseError> {
        self.expect(&Kind::LParen, "`(` and the macro's parameters")?;
        let mut params = Vec::new();
        while !self.eat(&Kind::RParen) {
            let name = self.ident("a parameter name or `)`")?;
            if RESERVED.contains(&name.text.as_str()) {
                return Err(ParseError {
                    span: name.span,
                    message: format!("`{}` is reserved and cannot name a parameter.", name.text),
                });
            }
            let default = if self.eat(&Kind::Eq) {
                Some(self.expr()?)
            } else {
                None
            };
            params.push(Param { name, default });
            if !self.eat(&Kind::Comma) && self.peek() != Some(&Kind::RParen) {
                return self.error("`,` or `)`");
            }
        }
        Ok(params)
    }

    fn args(&mut self) -> Result<Vec<Arg>, ParseError> {
        let mut args = Vec::new();
        while !self.eat(&Kind::RParen) {
            let name = self.ident("an argument name or `)`")?;
            self.expect(
                &Kind::Eq,
                "`=` after the argument's name: arguments are named",
            )?;
            let value = self.expr()?;
            args.push(Arg { name, value });
            if !self.eat(&Kind::Comma) && self.peek() != Some(&Kind::RParen) {
                return self.error("`,` or `)`");
            }
        }
        Ok(args)
    }

    fn expr(&mut self) -> Result<Expr, ParseError> {
        let first = self.term()?;
        if self.peek() != Some(&Kind::Amp) {
            return Ok(first);
        }
        let mut terms = vec![first];
        while self.eat(&Kind::Amp) {
            terms.push(self.term()?);
        }
        let span = terms[0].span().to(terms[terms.len() - 1].span());
        Ok(Expr::And(terms, span))
    }

    fn term(&mut self) -> Result<Expr, ParseError> {
        let span = self.span_here();
        let Some(kind) = self.peek().cloned() else {
            return self.error("a value");
        };
        match kind {
            Kind::Str(literal) => {
                self.position += 1;
                Ok(Expr::Str(literal, span))
            }
            Kind::Number(number) => {
                self.position += 1;
                Ok(Expr::Number(number, span))
            }
            Kind::LBracket => {
                self.position += 1;
                let mut elements = Vec::new();
                while !self.eat(&Kind::RBracket) {
                    if self.eat(&Kind::Spread) {
                        let spread = self.previous_span();
                        let value = self.term()?;
                        elements.push(Element::Spread(value, spread));
                    } else {
                        elements.push(Element::Value(self.expr()?));
                    }
                    if !self.eat(&Kind::Comma) && self.peek() != Some(&Kind::RBracket) {
                        return self.error("`,` or `]`");
                    }
                }
                Ok(Expr::Array(elements, span.to(self.previous_span())))
            }
            Kind::LBrace => self.block().map(Expr::Block),
            Kind::Ident(word) if matches!(word.as_str(), "true" | "false") => {
                self.position += 1;
                Ok(Expr::Bool(word == "true", span))
            }
            Kind::Ident(word) if word == "none" => Err(ParseError {
                span,
                message: "`none` is not a value: leave the field out.".into(),
            }),
            Kind::Ident(_) => {
                let name = self.dotted("a value")?;
                if self.peek() == Some(&Kind::Bang) && self.peek_at(1) == Some(&Kind::LParen) {
                    self.position += 2;
                    let args = self.args()?;
                    Ok(Expr::Macro {
                        span: name.span.to(self.previous_span()),
                        name,
                        args,
                    })
                } else if self.eat(&Kind::LParen) {
                    let args = self.args()?;
                    Ok(Expr::Call {
                        span: name.span.to(self.previous_span()),
                        name,
                        args,
                    })
                } else {
                    Ok(Expr::Ident(name))
                }
            }
            _ => self.error("a value"),
        }
    }

    fn block(&mut self) -> Result<Block, ParseError> {
        let open = self.expect(&Kind::LBrace, "`{`")?;
        let mut entries = Vec::new();
        while !self.eat(&Kind::RBrace) {
            entries.push(self.entry()?);
            self.eat(&Kind::Comma);
        }
        Ok(Block {
            entries,
            span: open.to(self.previous_span()),
        })
    }

    fn entry(&mut self) -> Result<Entry, ParseError> {
        let start = self.span_here();
        if self.eat(&Kind::Spread) {
            let value = self.term()?;
            return Ok(Entry::Spread(value.clone(), start.to(value.span())));
        }
        if let Some(Kind::Str(literal)) = self.peek().cloned() {
            self.position += 1;
            let Some(text) = literal.plain() else {
                return Err(ParseError {
                    span: start,
                    message: "A key cannot hold a slot.".into(),
                });
            };
            self.expect(&Kind::Eq, "`=` after the key")?;
            let value = self.expr()?;
            return Ok(Entry::Field {
                key: Name { text, span: start },
                value,
            });
        }
        let first = self.ident("a field, a declaration, `...` or `}`")?;
        if self.eat(&Kind::Eq) {
            let value = self.expr()?;
            return Ok(Entry::Field { key: first, value });
        }
        let name = self.dotted("a name, or `=` after a field")?;
        let header = if self.eat(&Kind::Colon) {
            Some(self.expr()?)
        } else {
            None
        };
        self.expect(&Kind::Eq, "`=`")?;
        let value = self.expr()?;
        Ok(Entry::Declare {
            span: start.to(value.span()),
            keyword: first,
            name,
            header,
            value,
        })
    }

    /// `profile NAME : KIND = { … }` or `profile NAME : KIND = match VAR { LABEL => { … }, … }`.
    fn profile(&mut self) -> Result<ProfileItem, ParseError> {
        let start = self.span_here();
        if !matches!(self.peek(), Some(Kind::Ident(word)) if word == "profile") {
            return self
                .error("a `profile` declaration: an `.artfprofiles` file holds only profiles");
        }
        self.position += 1;
        if self.peek() == Some(&Kind::Bang) {
            return self.error("a profile's name: there are no `profile!` macros");
        }
        let name = self.ident("the profile's name")?;
        if RESERVED.contains(&name.text.as_str()) {
            return Err(ParseError {
                span: name.span,
                message: format!("`{}` is reserved and cannot name a declaration.", name.text),
            });
        }
        self.expect(
            &Kind::Colon,
            "`:` and what evaluates with the profile: `profile NAME : agent = { … }`",
        )?;
        let kind = self.ident("what evaluates with the profile: `agent`")?;
        self.expect(&Kind::Eq, "`=`")?;
        let body = if matches!(self.peek(), Some(Kind::Ident(word)) if word == "match") {
            self.position += 1;
            let variable = self.ident("the environment variable the profile matches")?;
            self.expect(&Kind::LBrace, "`{` and the profile's branches")?;
            let mut arms = Vec::new();
            while !self.eat(&Kind::RBrace) {
                let label = self.ident("a branch: `LABEL => { … }`, or `}`")?;
                self.expect(&Kind::Arrow, "`=>` after the branch's label")?;
                arms.push((label, self.block()?));
                if !self.eat(&Kind::Comma) && self.peek() != Some(&Kind::RBrace) {
                    return self.error("`,` or `}`");
                }
            }
            ProfileBody::Match { variable, arms }
        } else {
            ProfileBody::Setting(self.block()?)
        };
        Ok(ProfileItem {
            name,
            kind,
            body,
            span: start.to(self.previous_span()),
        })
    }
}

#[cfg(test)]
mod tests;
