//! The definition view of `.artf` bytes, shared by artifactsum and file tools.
use std::{
    borrow::Cow,
    fs::File,
    io::{self, Read},
};

/// A canonical TOML definition: remove only the top-level `annotations` key.
/// Comments and spelling are not meaning. Invalid UTF-8 or TOML stays raw bytes.
pub fn definition(bytes: &[u8]) -> Cow<'_, [u8]> {
    let Ok(source) = std::str::from_utf8(bytes) else {
        return Cow::Borrowed(bytes);
    };
    let Ok(mut document) = toml::from_str::<toml::Table>(source) else {
        return Cow::Borrowed(bytes);
    };
    document.remove("annotations");
    Cow::Owned(
        toml::to_string(&canonical(toml::Value::Table(document)))
            .expect("a parsed TOML table serializes")
            .into_bytes(),
    )
}

// Cargo feature unification may enable TOML's insertion-ordered maps. Sort every table
// explicitly so the view does not depend on that feature or on authored key order.
fn canonical(value: toml::Value) -> toml::Value {
    match value {
        toml::Value::Table(table) => toml::Value::Table(
            table
                .into_iter()
                .map(|(key, value)| (key, canonical(value)))
                .collect::<std::collections::BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        toml::Value::Array(values) => {
            toml::Value::Array(values.into_iter().map(canonical).collect())
        }
        value => value,
    }
}

/// File tools normalize a whole declaration before paging its lines. The existing search
/// document bound also bounds this parse; an oversized declaration is an explicit error,
/// never a partial parse that could expose un-fingerprinted presentation text.
const DOCUMENT_BYTES: u64 = 8 * 1024 * 1024;
pub(crate) fn reader(file: File, path: &str) -> io::Result<Box<dyn Read>> {
    if !path.ends_with(".artf") {
        return Ok(Box::new(file));
    }
    let mut bytes = Vec::new();
    file.take(DOCUMENT_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > DOCUMENT_BYTES {
        return Err(io::Error::other(
            "Artifact definition exceeds the 8 MiB file-tool bound.",
        ));
    }
    Ok(Box::new(io::Cursor::new(definition(&bytes).into_owned())))
}
