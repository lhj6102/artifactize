//! Tokens of the `.artf` language: identifiers, numbers, strings with typed `@{…}` slots,
//! punctuation and `//` comments. Strings are decoded once here, so a value spliced into a
//! string later is never read for slots again.

use std::ops::Range;

/// Identifiers start with an ASCII letter and are at most 64 bytes, like Artifact names.
pub const MAX_IDENTIFIER_BYTES: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: Kind,
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Ident(String),
    /// A JSON number, kept as written.
    Number(String),
    Str(Literal),
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Eq,
    Comma,
    Colon,
    Amp,
    Bang,
    Dot,
    Spread,
}

impl Kind {
    /// How a diagnostic names what it found.
    pub fn describe(&self) -> String {
        match self {
            Self::Ident(name) => format!("`{name}`"),
            Self::Number(number) => format!("number {number}"),
            Self::Str(_) => "a string".into(),
            Self::LBrace => "`{`".into(),
            Self::RBrace => "`}`".into(),
            Self::LParen => "`(`".into(),
            Self::RParen => "`)`".into(),
            Self::LBracket => "`[`".into(),
            Self::RBracket => "`]`".into(),
            Self::Eq => "`=`".into(),
            Self::Comma => "`,`".into(),
            Self::Colon => "`:`".into(),
            Self::Amp => "`&`".into(),
            Self::Bang => "`!`".into(),
            Self::Dot => "`.`".into(),
            Self::Spread => "`...`".into(),
        }
    }
}

/// A decoded string: text and typed slots in order. A raw `'''…'''` string is one text part.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Literal {
    pub parts: Vec<Part>,
}

impl Literal {
    /// The text when the string has no slot.
    pub fn plain(&self) -> Option<String> {
        self.parts
            .iter()
            .map(|part| match part {
                Part::Text(text) => Some(text.as_str()),
                Part::Slot(_) => None,
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Text(String),
    Slot(Slot),
}

/// `@{kind word …}`: what kind of value fills it and the words naming it, each with its span.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    pub kind: SlotKind,
    pub words: Vec<(String, Range<usize>)>,
    pub span: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    Artf,
    Path,
    Param,
    Tool,
}

impl SlotKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Artf => "artf",
            Self::Path => "path",
            Self::Param => "param",
            Self::Tool => "tool",
        }
    }

    fn arity(self) -> usize {
        match self {
            Self::Tool => 3,
            Self::Artf | Self::Path | Self::Param => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct SyntaxError {
    pub offset: usize,
    pub message: String,
}

fn error(offset: usize, message: impl Into<String>) -> SyntaxError {
    SyntaxError {
        offset,
        message: message.into(),
    }
}

/// The tokens of a source and the spans of its comments.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Lexed {
    pub tokens: Vec<Token>,
    pub comments: Vec<Range<usize>>,
}

pub fn lex(source: &str) -> Result<Lexed, SyntaxError> {
    let bytes = source.as_bytes();
    let mut lexed = Lexed::default();
    let mut index = 0;
    while index < bytes.len() {
        let start = index;
        let byte = bytes[index];
        let single = |kind| Some((kind, 1));
        let punct = match byte {
            b'{' => single(Kind::LBrace),
            b'}' => single(Kind::RBrace),
            b'(' => single(Kind::LParen),
            b')' => single(Kind::RParen),
            b'[' => single(Kind::LBracket),
            b']' => single(Kind::RBracket),
            b'=' => single(Kind::Eq),
            b',' => single(Kind::Comma),
            b':' => single(Kind::Colon),
            b'&' => single(Kind::Amp),
            b'!' => single(Kind::Bang),
            b'.' if source[index..].starts_with("...") => Some((Kind::Spread, 3)),
            b'.' => single(Kind::Dot),
            _ => None,
        };
        if let Some((kind, width)) = punct {
            index += width;
            lexed.tokens.push(Token {
                kind,
                span: start..index,
            });
            continue;
        }
        match byte {
            b' ' | b'\t' | b'\r' | b'\n' => index += 1,
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index = source[index..]
                    .find('\n')
                    .map_or(bytes.len(), |end| index + end);
                lexed.comments.push(start..index);
            }
            b'"' | b'\'' => {
                let (literal, end) = string(source, index)?;
                index = end;
                lexed.tokens.push(Token {
                    kind: Kind::Str(literal),
                    span: start..index,
                });
            }
            b'-' | b'0'..=b'9' => {
                index = number(source, index)?;
                lexed.tokens.push(Token {
                    kind: Kind::Number(source[start..index].to_owned()),
                    span: start..index,
                });
            }
            byte if byte.is_ascii_alphabetic() => {
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'_' | b'-'))
                {
                    index += 1;
                }
                if index - start > MAX_IDENTIFIER_BYTES {
                    return Err(error(
                        start,
                        format!("An identifier is at most {MAX_IDENTIFIER_BYTES} bytes."),
                    ));
                }
                lexed.tokens.push(Token {
                    kind: Kind::Ident(source[start..index].to_owned()),
                    span: start..index,
                });
            }
            _ => {
                let character = source[index..].chars().next().expect("index on a char");
                return Err(error(index, format!("Unexpected character {character:?}.")));
            }
        }
    }
    Ok(lexed)
}

/// `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`, as JSON writes numbers.
fn number(source: &str, start: usize) -> Result<usize, SyntaxError> {
    let bytes = source.as_bytes();
    let digits = |mut index: usize| {
        let from = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        (index, index - from)
    };
    let mut index = start + usize::from(bytes[start] == b'-');
    let (after, count) = digits(index);
    if count == 0 {
        return Err(error(start, "A number needs a digit after `-`."));
    }
    if count > 1 && bytes[index] == b'0' {
        return Err(error(start, "A number has no leading zero."));
    }
    index = after;
    if bytes.get(index) == Some(&b'.') {
        let (after, count) = digits(index + 1);
        if count == 0 {
            return Err(error(index, "A number needs a digit after `.`."));
        }
        index = after;
    }
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        index += 1;
        if matches!(bytes.get(index), Some(b'+' | b'-')) {
            index += 1;
        }
        let (after, count) = digits(index);
        if count == 0 {
            return Err(error(index, "A number needs a digit in its exponent."));
        }
        index = after;
    }
    if bytes
        .get(index)
        .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_'))
    {
        return Err(error(start, "A number cannot run into a name."));
    }
    Ok(index)
}

/// Decode one string starting at `start` (its opening quote); return it and the offset after it.
fn string(source: &str, start: usize) -> Result<(Literal, usize), SyntaxError> {
    let rest = &source[start..];
    if let Some(body) = rest.strip_prefix("\"\"\"") {
        let close = body
            .find("\"\"\"")
            .ok_or_else(|| error(start, "This `\"\"\"` string has no closing `\"\"\"`."))?;
        let content = start + 3..start + 3 + close;
        let (text, map) = dedent(source, content);
        Ok((decode(&text, &map, false)?, start + 3 + close + 3))
    } else if let Some(body) = rest.strip_prefix("'''") {
        let close = body
            .find("'''")
            .ok_or_else(|| error(start, "This `'''` string has no closing `'''`."))?;
        let content = start + 3..start + 3 + close;
        let (text, map) = dedent(source, content);
        Ok((decode(&text, &map, true)?, start + 3 + close + 3))
    } else if rest.starts_with('"') {
        let bytes = source.as_bytes();
        let mut index = start + 1;
        loop {
            match bytes.get(index) {
                None | Some(b'\n') => {
                    return Err(error(start, "This string has no closing `\"` on its line."));
                }
                Some(b'\\') => index += 2,
                Some(b'"') => break,
                Some(_) => index += 1,
            }
        }
        let content = &source[start + 1..index];
        let map: Vec<usize> = (start + 1..index).collect();
        Ok((decode(content, &map, false)?, index + 1))
    } else {
        Err(error(
            start,
            "A single quote starts only a raw `'''` string; use `\"` for a string.",
        ))
    }
}

/// Multi-line strings drop the line break after the opening quotes, the last line when it
/// holds only the closing quotes' indentation, and the indentation every other line shares.
/// Return the text and the source offset of each of its bytes.
fn dedent(source: &str, content: Range<usize>) -> (String, Vec<usize>) {
    let mut start = content.start;
    let body = &source[content.clone()];
    if body.starts_with("\r\n") {
        start += 2;
    } else if body.starts_with('\n') {
        start += 1;
    }
    let mut lines: Vec<Range<usize>> = Vec::new();
    let mut line_start = start;
    for (offset, byte) in source[start..content.end].bytes().enumerate() {
        if byte == b'\n' {
            let end = start + offset;
            let end = if end > line_start && source.as_bytes()[end - 1] == b'\r' {
                end - 1
            } else {
                end
            };
            lines.push(line_start..end);
            line_start = start + offset + 1;
        }
    }
    lines.push(line_start..content.end);
    if lines.len() > 1
        && source[lines.last().expect("a line").clone()]
            .bytes()
            .all(|byte| matches!(byte, b' ' | b'\t'))
    {
        lines.pop();
    }
    let blank = |line: &Range<usize>| source[line.clone()].trim().is_empty();
    let indent = lines
        .iter()
        .filter(|line| !blank(line))
        .map(|line| {
            source[line.clone()]
                .bytes()
                .take_while(|byte| matches!(byte, b' ' | b'\t'))
                .count()
        })
        .min()
        .unwrap_or(0);
    let mut text = String::new();
    let mut map = Vec::new();
    for (number, line) in lines.iter().enumerate() {
        if number > 0 {
            text.push('\n');
            map.push(line.start.saturating_sub(1));
        }
        if blank(line) {
            continue;
        }
        let from = line.start + indent;
        text.push_str(&source[from..line.end]);
        map.extend(from..line.end);
    }
    (text, map)
}

/// Escapes and slots, read once. A raw string keeps every byte as written.
fn decode(text: &str, map: &[usize], raw: bool) -> Result<Literal, SyntaxError> {
    let at = |index: usize| {
        map.get(index)
            .copied()
            .unwrap_or_else(|| map.last().map_or(0, |last| last + 1))
    };
    if raw {
        return Ok(Literal {
            parts: vec![Part::Text(text.to_owned())],
        });
    }
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = text.char_indices().peekable();
    while let Some((index, character)) = chars.next() {
        let rest = &text[index..];
        if rest.starts_with("@@{") {
            current.push_str("@{");
            chars.next();
            chars.next();
            continue;
        }
        if rest.starts_with("@{") {
            let close = rest
                .find('}')
                .ok_or_else(|| error(at(index), "This `@{` slot has no closing `}`."))?;
            let slot = slot(
                &rest[2..close],
                index + 2,
                map,
                at(index)..at(index + close) + 1,
            )?;
            if !current.is_empty() {
                parts.push(Part::Text(std::mem::take(&mut current)));
            }
            parts.push(Part::Slot(slot));
            while chars.peek().is_some_and(|(next, _)| *next <= index + close) {
                chars.next();
            }
            continue;
        }
        if character != '\\' {
            current.push(character);
            continue;
        }
        let Some((_, escaped)) = chars.next() else {
            return Err(error(at(index), "A string cannot end with `\\`."));
        };
        match escaped {
            '"' => current.push('"'),
            '\\' => current.push('\\'),
            '/' => current.push('/'),
            'b' => current.push('\u{8}'),
            'f' => current.push('\u{c}'),
            'n' => current.push('\n'),
            'r' => current.push('\r'),
            't' => current.push('\t'),
            'u' => {
                let hex: String = (0..4)
                    .filter_map(|_| chars.next().map(|(_, c)| c))
                    .collect();
                let value = u32::from_str_radix(&hex, 16)
                    .ok()
                    .filter(|_| hex.len() == 4)
                    .and_then(char::from_u32)
                    .ok_or_else(|| {
                        error(at(index), "`\\u` takes four hex digits of a character.")
                    })?;
                current.push(value);
            }
            '\n' => {
                // A line-ending backslash joins the next line without its indentation.
                while chars
                    .peek()
                    .is_some_and(|(_, next)| matches!(next, ' ' | '\t' | '\n' | '\r'))
                {
                    chars.next();
                }
            }
            other => {
                return Err(error(
                    at(index),
                    format!("Unknown escape `\\{other}`; write `\\\\` for a backslash."),
                ));
            }
        }
    }
    if !current.is_empty() || parts.is_empty() {
        parts.push(Part::Text(current));
    }
    Ok(Literal { parts })
}

/// The words of `@{kind word …}`, checked for their count; what they name is checked later.
fn slot(
    inner: &str,
    offset: usize,
    map: &[usize],
    span: Range<usize>,
) -> Result<Slot, SyntaxError> {
    let at = |index: usize| map.get(offset + index).copied().unwrap_or(span.start);
    let mut words = Vec::new();
    let mut starts = Vec::new();
    let mut cursor = 0;
    for word in inner.split([' ', '\t', '\n', '\r']) {
        if !word.is_empty() {
            words.push((word.to_owned(), at(cursor)..at(cursor) + word.len()));
            starts.push(cursor);
        }
        cursor += word.len() + 1;
    }
    // A path under an Artifact is a file name, which may hold spaces: `@{path NAME/a b.txt}`
    // runs to the closing brace.
    if words.first().is_some_and(|(kind, _)| kind == "path")
        && words.len() > 2
        && words[1].0.contains('/')
    {
        let start = starts[1];
        let rest = inner[start..].trim_end();
        words.truncate(1);
        words.push((rest.to_owned(), at(start)..at(start) + rest.len()));
    }
    let Some((kind, kind_span)) = words.first().cloned() else {
        return Err(error(
            span.start,
            "An empty `@{}` slot; write `@{artf NAME}`, `@{path NAME}`, `@{param NAME}` or `@{tool NAME AUDIENCE TOOL}`, or `@@{` for a literal `@{`.",
        ));
    };
    let kind = match kind.as_str() {
        "artf" => SlotKind::Artf,
        "path" => SlotKind::Path,
        "param" => SlotKind::Param,
        "tool" => SlotKind::Tool,
        other => {
            return Err(error(
                kind_span.start,
                format!(
                    "Unknown slot kind `{other}`: a slot is `@{{artf …}}`, `@{{path …}}`, `@{{param …}}` or `@{{tool …}}`; write `@@{{` for a literal `@{{`."
                ),
            ));
        }
    };
    words.remove(0);
    if words.len() != kind.arity() {
        return Err(error(
            span.start,
            match kind {
                SlotKind::Tool => "A tool slot names an Artifact, an audience and a tool: `@{tool self human html.view}`.".to_owned(),
                kind => format!("A `{}` slot names one thing: `@{{{} NAME}}`.", kind.name(), kind.name()),
            },
        ));
    }
    Ok(Slot { kind, words, span })
}

#[cfg(test)]
mod tests;
