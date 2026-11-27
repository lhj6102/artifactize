//! Parse `.artf` tokens into items. Only the syntax is checked here; names and kinds resolve
//! once every file has been read.

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
    let lexed = lexer::lex(source).map_err(|error| ParseError {
        span: Span::new(file, error.offset..error.offset),
        message: error.message,
    })?;
    let mut parser = Parser {
        file,
        tokens: lexed.tokens,
        position: 0,
        end: source.len(),
    };
    let mut items = Vec::new();
    while parser.peek().is_some() {
        items.push(parser.item()?);
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

    fn binding(&mut self, kind: ItemKind, is_macro: bool) -> Result<Name, ParseError> {
        let name = if kind == ItemKind::Tool && !is_macro {
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
                "profile" => ItemKind::Profile,
                "eval" => ItemKind::Eval,
                _ => return self.error("a declaration: `artf`, `tool`, `profile` or `eval`"),
            },
            _ => return self.error("a declaration: `artf`, `tool`, `profile` or `eval`"),
        };
        self.position += 1;
        let is_macro = self.eat(&Kind::Bang);
        let name = self.binding(kind, is_macro)?;
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
            Kind::Ident(word) if word == "none" => {
                self.position += 1;
                Ok(Expr::None(span))
            }
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
}

#[cfg(test)]
mod tests;
