//! The views of `.artf` bytes that file tools and artifactsum share: comments are notes for
//! people, not meaning, so neither shows them nor hashes them.
pub mod lexer;
mod syntax;

/// Masks the `!alias(…)` profiles of TOML definitions from before 0.27, for their migration.
pub use syntax::{AliasInvocation, AliasSyntaxError, Masked, mask};

use std::{
    borrow::Cow,
    fs::File,
    io::{self, Read},
    path::Path,
};

/// What file tools show of a definition: its text with each `//` comment removed and the
/// lines kept, so line numbers still match. Bytes that are not the language stay as they are.
pub fn definition(bytes: &[u8]) -> Cow<'_, [u8]> {
    let Ok(source) = std::str::from_utf8(bytes) else {
        return Cow::Borrowed(bytes);
    };
    let Ok(lexed) = lexer::lex(source) else {
        return Cow::Borrowed(bytes);
    };
    if lexed.comments.is_empty() {
        return Cow::Borrowed(bytes);
    }
    let mut text = String::with_capacity(source.len());
    let mut cursor = 0;
    for comment in &lexed.comments {
        text.push_str(source[cursor..comment.start].trim_end_matches([' ', '\t']));
        cursor = comment.end;
    }
    text.push_str(&source[cursor..]);
    Cow::Owned(text.into_bytes())
}

/// What artifactsum hashes of a definition: its tokens alone, so neither comments nor layout
/// change a fingerprint. Bytes that are not the language are hashed as they are.
pub fn fingerprinted(bytes: &[u8]) -> Cow<'_, [u8]> {
    let Ok(source) = std::str::from_utf8(bytes) else {
        return Cow::Borrowed(bytes);
    };
    let Ok(lexed) = lexer::lex(source) else {
        return Cow::Borrowed(bytes);
    };
    let tokens: Vec<&str> = lexed
        .tokens
        .iter()
        .map(|token| &source[token.span.clone()])
        .collect();
    Cow::Owned(tokens.join(" ").into_bytes())
}

/// File tools normalize a whole declaration before paging its lines. The existing search
/// document bound also bounds this parse; an oversized declaration is an explicit error,
/// never a partial parse that could expose un-fingerprinted presentation text.
const DOCUMENT_BYTES: u64 = 8 * 1024 * 1024;
/// Read one extra byte to distinguish an exact fit from an oversized declaration.
const OVERFLOW_PROBE_BYTES: u64 = 1;

pub(crate) fn reader(file: File, path: &Path) -> io::Result<Box<dyn Read>> {
    if !path.extension().is_some_and(|ext| ext == "artf")
        && !path.file_name().is_some_and(|name| name == ".artf")
    {
        return Ok(Box::new(file));
    }
    let mut bytes = Vec::new();
    file.take(DOCUMENT_BYTES + OVERFLOW_PROBE_BYTES)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > DOCUMENT_BYTES {
        return Err(io::Error::other(
            "Artifact definition exceeds the 8 MiB file-tool bound.",
        ));
    }
    Ok(Box::new(io::Cursor::new(definition(&bytes).into_owned())))
}
