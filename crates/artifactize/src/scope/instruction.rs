use std::collections::BTreeSet;

use crate::config::identifier;

#[derive(Debug, PartialEq, Eq)]
pub struct Reference<'a> {
    pub name: &'a str,
    pub start: usize,
    pub end: usize,
}

fn escaped(source: &[u8], index: usize) -> bool {
    source[..index]
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
        % 2
        == 1
}

fn closing_brace(source: &[u8], start: usize) -> Option<usize> {
    let literal = escaped(source, start);
    let mut depth = 0;
    let mut quote = None;
    let mut index = start;
    while index < source.len() {
        let character = source[index];
        if character == b'\\' {
            if !literal || quote.is_some() || !matches!(source.get(index + 1), Some(b'{' | b'}')) {
                index += 1;
            }
        } else if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            }
        } else if matches!(character, b'"' | b'\'' | b'`') {
            quote = Some(character);
        } else if character == b'{' {
            depth += 1;
        } else if character == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

/// Bare tokens only; whole JSON, nested, quoted and escaped brace groups stay literal.
pub fn references(source: &str) -> Vec<Reference<'_>> {
    let bytes = source.as_bytes();
    let mut result = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'{' {
            cursor += 1;
            continue;
        }
        let Some(end) = closing_brace(bytes, cursor) else {
            break;
        };
        let name = &source[cursor + 1..end];
        if (cursor == 0 || bytes[cursor - 1] != b'$')
            && !escaped(bytes, cursor)
            && identifier(name, "Reference").is_ok()
        {
            result.push(Reference {
                name,
                start: cursor,
                end: end + 1,
            });
        }
        cursor = end + 1;
    }
    result
}

pub fn instruction_references(source: &str) -> Vec<&str> {
    let mut seen = BTreeSet::new();
    references(source)
        .into_iter()
        .filter_map(|reference| seen.insert(reference.name).then_some(reference.name))
        .collect()
}
