//! Scanner and reversible masker for `.artf` profile alias expressions (`!alias`, `!alias(...)`).
//!
//! Standard TOML has no `!` token outside strings and comments, so masking value-position alias
//! invocations into unique quoted strings lets both `toml` and `toml_edit` parse, validate,
//! canonicalize, and losslessly edit `.artf` files while keeping exact source spans.

use std::ops::Range;

/// Length of a TOML multi-line string delimiter (`"""` or `'''`).
const TRIPLE_QUOTE_LEN: usize = 3;
/// Maximum consecutive quotes that can close a TOML multi-line string (two content quotes + three).
const MAX_CLOSING_QUOTE_RUN: usize = 5;
/// Base sentinel prefix for placeholder strings; disambiguated if present in source text.
const PLACEHOLDER_BASE: &str = "__artf_alias_";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasInvocation {
    /// Byte span of `!name` or `!name(...)` in the original source.
    pub span: Range<usize>,
    /// Raw alias name immediately following `!`.
    pub name: String,
    /// Byte span of `name` in the original source.
    pub name_span: Range<usize>,
    /// Byte span strictly inside `(...)` in the original source when parentheses are present.
    pub overrides_span: Option<Range<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasSyntaxError {
    pub offset: usize,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Masked {
    /// Valid TOML with each value-position `!alias` / `!alias(...)` replaced by a unique string.
    pub toml: String,
    prefix: String,
    invocations: Vec<AliasInvocation>,
    spans: Vec<(Range<usize>, Range<usize>)>,
}

impl Masked {
    pub fn invocations(&self) -> &[AliasInvocation] {
        &self.invocations
    }

    /// Return the invocation index if `value` is a placeholder emitted by [`mask`].
    pub fn invocation_index(&self, value: &str) -> Option<usize> {
        let rest = value.strip_prefix(&self.prefix)?;
        let digits = rest.strip_suffix("__")?;
        let index: usize = digits.parse().ok()?;
        (index < self.invocations.len()).then_some(index)
    }

    /// Map a byte offset in [`Self::toml`] back to its byte offset in the original source.
    pub fn original_offset(&self, masked_offset: usize) -> usize {
        let mut delta: isize = 0;
        for (masked, original) in &self.spans {
            if masked_offset < masked.start {
                break;
            }
            if masked_offset < masked.end {
                return original.start;
            }
            delta += original.len() as isize - masked.len() as isize;
        }
        (masked_offset as isize + delta).max(0) as usize
    }

    /// Restore each placeholder in `edited` back to its exact original slice from `original_source`.
    pub fn unmask(&self, edited: &str, original_source: &str) -> String {
        self.unmask_with(edited, |_, invocation| {
            original_source[invocation.span.clone()].to_owned()
        })
    }

    /// Replace each placeholder string literal in `serialized` using `replacement`.
    pub fn unmask_with(
        &self,
        serialized: &str,
        replacement: impl Fn(usize, &AliasInvocation) -> String,
    ) -> String {
        if self.invocations.is_empty() {
            return serialized.to_owned();
        }
        let mut output = serialized.to_owned();
        for (index, invocation) in self.invocations.iter().enumerate() {
            let quoted = format!("\"{}{index}__\"", self.prefix);
            let value = replacement(index, invocation);
            output = output.replace(&quoted, &value);
        }
        output
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Container {
    Array,
    InlineTable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Position {
    Key,
    Value,
}

/// Scan `source` for value-position `!alias` and `!alias(...)` expressions and mask them into
/// collision-free TOML string literals.
pub fn mask(source: &str) -> Result<Masked, AliasSyntaxError> {
    let invocations = scan(source)?;
    if invocations.is_empty() {
        return Ok(Masked {
            toml: source.to_owned(),
            prefix: PLACEHOLDER_BASE.to_owned(),
            invocations,
            spans: Vec::new(),
        });
    }
    let prefix = unique_prefix(source);
    let mut toml = String::with_capacity(source.len());
    let mut spans = Vec::with_capacity(invocations.len());
    let mut cursor = 0;
    for (index, invocation) in invocations.iter().enumerate() {
        toml.push_str(&source[cursor..invocation.span.start]);
        let start = toml.len();
        toml.push('"');
        toml.push_str(&prefix);
        toml.push_str(&index.to_string());
        toml.push_str("__\"");
        let end = toml.len();
        spans.push((start..end, invocation.span.clone()));
        cursor = invocation.span.end;
    }
    toml.push_str(&source[cursor..]);
    Ok(Masked {
        toml,
        prefix,
        invocations,
        spans,
    })
}

fn unique_prefix(source: &str) -> String {
    if !source.contains(PLACEHOLDER_BASE) {
        return PLACEHOLDER_BASE.to_owned();
    }
    let mut nonce = 1_u64;
    loop {
        let candidate = format!("{PLACEHOLDER_BASE}{nonce}_");
        if !source.contains(&candidate) {
            return candidate;
        }
        nonce += 1;
    }
}

fn scan(source: &str) -> Result<Vec<AliasInvocation>, AliasSyntaxError> {
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut stack = Vec::new();
    let mut position = Position::Key;
    let mut invocations = Vec::new();

    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\r' => {
                i += 1;
            }
            b'\n' => {
                if stack.is_empty() {
                    position = Position::Key;
                }
                i += 1;
            }
            b'#' => {
                i = skip_comment(bytes, i);
            }
            b'"' | b'\'' => {
                i = skip_string(bytes, i);
                if position == Position::Value {
                    position = Position::Key;
                }
            }
            b'[' if position == Position::Key && stack.is_empty() => {
                i = skip_table_header(bytes, i);
            }
            b'[' if position == Position::Value => {
                stack.push(Container::Array);
                position = Position::Value;
                i += 1;
            }
            b'{' if position == Position::Value => {
                stack.push(Container::InlineTable);
                position = Position::Key;
                i += 1;
            }
            b']' => {
                if stack.last() == Some(&Container::Array) {
                    stack.pop();
                }
                position = Position::Key;
                i += 1;
            }
            b'}' => {
                if stack.last() == Some(&Container::InlineTable) {
                    stack.pop();
                }
                position = Position::Key;
                i += 1;
            }
            b'=' if position == Position::Key => {
                position = Position::Value;
                i += 1;
            }
            b',' => {
                position = if stack.last() == Some(&Container::Array) {
                    Position::Value
                } else {
                    Position::Key
                };
                i += 1;
            }
            b'!' if position == Position::Value => {
                let (invocation, next) = parse_alias(source, i)?;
                invocations.push(invocation);
                position = Position::Key;
                i = next;
            }
            _ if position == Position::Value => {
                i = skip_scalar(bytes, i);
                position = Position::Key;
            }
            _ => {
                i += 1;
            }
        }
    }
    Ok(invocations)
}

fn parse_alias(source: &str, bang: usize) -> Result<(AliasInvocation, usize), AliasSyntaxError> {
    let bytes = source.as_bytes();
    let name_start = bang + 1;
    let mut name_end = name_start;
    while name_end < bytes.len() && is_alias_byte(bytes[name_end]) {
        name_end += 1;
    }
    if name_end == name_start {
        return Err(AliasSyntaxError {
            offset: bang,
            message: "Expected a profile alias name after `!`.".into(),
        });
    }
    let name = source[name_start..name_end].to_owned();
    let name_span = name_start..name_end;

    let mut lookahead = name_end;
    while lookahead < bytes.len() && matches!(bytes[lookahead], b' ' | b'\t') {
        lookahead += 1;
    }
    if bytes.get(lookahead) == Some(&b'(') {
        let open = lookahead;
        let overrides_start = open + 1;
        let close = find_closing_paren(bytes, open)?;
        return Ok((
            AliasInvocation {
                span: bang..close + 1,
                name,
                name_span,
                overrides_span: Some(overrides_start..close),
            },
            close + 1,
        ));
    }
    Ok((
        AliasInvocation {
            span: bang..name_end,
            name,
            name_span,
            overrides_span: None,
        },
        name_end,
    ))
}

fn find_closing_paren(bytes: &[u8], open: usize) -> Result<usize, AliasSyntaxError> {
    let mut i = open + 1;
    let mut paren_depth: usize = 1;
    let mut brace_depth: usize = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'#' => i = skip_comment(bytes, i),
            b'"' | b'\'' => i = skip_string(bytes, i),
            b'{' => {
                brace_depth += 1;
                i += 1;
            }
            b'}' => {
                if brace_depth == 0 {
                    return Err(AliasSyntaxError {
                        offset: i,
                        message: "Unexpected `}` in profile alias override.".into(),
                    });
                }
                brace_depth -= 1;
                i += 1;
            }
            b'(' => {
                paren_depth += 1;
                i += 1;
            }
            b')' => {
                paren_depth -= 1;
                if paren_depth == 0 {
                    return Ok(i);
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    Err(AliasSyntaxError {
        offset: open,
        message: "Unclosed `(` in profile alias override.".into(),
    })
}

fn is_alias_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn skip_comment(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i
}

fn skip_string(bytes: &[u8], i: usize) -> usize {
    let quote = bytes[i];
    let multiline = bytes.get(i..i + TRIPLE_QUOTE_LEN) == Some(&[quote; TRIPLE_QUOTE_LEN]);
    if multiline {
        skip_multiline_string(bytes, i + TRIPLE_QUOTE_LEN, quote)
    } else {
        skip_single_line_string(bytes, i + 1, quote)
    }
}

fn skip_single_line_string(bytes: &[u8], mut i: usize, quote: u8) -> usize {
    while i < bytes.len() {
        let byte = bytes[i];
        if quote == b'"' && byte == b'\\' {
            i = (i + 2).min(bytes.len());
            continue;
        }
        if byte == quote {
            return i + 1;
        }
        if byte == b'\n' {
            return i;
        }
        i += 1;
    }
    i
}

fn skip_multiline_string(bytes: &[u8], mut i: usize, quote: u8) -> usize {
    while i < bytes.len() {
        let byte = bytes[i];
        if quote == b'"' && byte == b'\\' {
            i = (i + 2).min(bytes.len());
            continue;
        }
        if byte == quote {
            let mut run = 0;
            while i + run < bytes.len() && bytes[i + run] == quote {
                run += 1;
            }
            if run >= TRIPLE_QUOTE_LEN {
                return i + run.min(MAX_CLOSING_QUOTE_RUN);
            }
            i += run;
            continue;
        }
        i += 1;
    }
    i
}

fn skip_table_header(bytes: &[u8], mut i: usize) -> usize {
    let double = bytes.get(i..i + 2) == Some(b"[[");
    i += if double { 2 } else { 1 };
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' => i = skip_string(bytes, i),
            b'\n' => return i,
            b']' => {
                if double {
                    if bytes.get(i..i + 2) == Some(b"]]") {
                        return i + 2;
                    }
                    i += 1;
                } else {
                    return i + 1;
                }
            }
            _ => i += 1,
        }
    }
    i
}

fn skip_scalar(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        if matches!(
            bytes[i],
            b' ' | b'\t' | b'\r' | b'\n' | b'#' | b',' | b']' | b'}' | b'='
        ) {
            break;
        }
        i += 1;
    }
    i
}
