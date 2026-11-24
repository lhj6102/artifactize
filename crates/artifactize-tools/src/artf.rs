//! The definition view of `.artf` bytes, shared by artifactsum and file tools.
mod syntax;

pub use syntax::{AliasInvocation, AliasSyntaxError, Masked, mask};

use std::{
    borrow::Cow,
    collections::BTreeMap,
    fs::File,
    io::{self, Read},
    path::Path,
};

/// A canonical `.artf` definition: remove only the top-level `annotations` key and normalize
/// profile alias overrides. Comments and spelling are not meaning. Invalid UTF-8 or syntax stays
/// raw bytes.
pub fn definition(bytes: &[u8]) -> Cow<'_, [u8]> {
    let Ok(source) = std::str::from_utf8(bytes) else {
        return Cow::Borrowed(bytes);
    };
    let Ok(masked) = mask(source) else {
        return Cow::Borrowed(bytes);
    };
    let Some(canonical_aliases) = masked
        .invocations()
        .iter()
        .map(|invocation| canonical_invocation(source, invocation))
        .collect::<Option<Vec<_>>>()
    else {
        return Cow::Borrowed(bytes);
    };
    let Ok(mut document) = toml::from_str::<toml::Table>(&masked.toml) else {
        return Cow::Borrowed(bytes);
    };
    document.remove("annotations");
    let serialized = toml::to_string(&canonical(toml::Value::Table(document)))
        .expect("a parsed TOML table serializes");
    Cow::Owned(
        masked
            .unmask_with(&serialized, |index, _| canonical_aliases[index].clone())
            .into_bytes(),
    )
}

fn canonical_invocation(source: &str, invocation: &AliasInvocation) -> Option<String> {
    let Some(span) = &invocation.overrides_span else {
        return Some(format!("!{}", invocation.name));
    };
    let wrapped = format!("_ = {{{}}}", &source[span.clone()]);
    let mut table = toml::from_str::<toml::Table>(&wrapped).ok()?;
    if table.len() != 1 {
        return None;
    }
    let toml::Value::Table(overrides) = table.remove("_")? else {
        return None;
    };
    if overrides.is_empty() {
        return Some(format!("!{}", invocation.name));
    }
    let toml::Value::Table(sorted) = canonical(toml::Value::Table(overrides)) else {
        unreachable!("canonical preserves tables");
    };
    Some(format!(
        "!{}({})",
        invocation.name,
        format_table_entries(&sorted)
    ))
}

fn format_table_entries(table: &toml::Table) -> String {
    table
        .iter()
        .map(|(key, value)| format!("{} = {}", format_key(key), format_inline_value(value)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_key(key: &str) -> String {
    if !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        key.to_owned()
    } else {
        toml::Value::String(key.to_owned()).to_string()
    }
}

fn format_inline_value(value: &toml::Value) -> String {
    match value {
        toml::Value::Table(table) => {
            if table.is_empty() {
                "{}".to_owned()
            } else {
                format!("{{ {} }}", format_table_entries(table))
            }
        }
        toml::Value::Array(values) => {
            let items = values
                .iter()
                .map(format_inline_value)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{items}]")
        }
        scalar => scalar.to_string(),
    }
}

// Cargo feature unification may enable TOML's insertion-ordered maps. Sort every table
// explicitly so the view does not depend on that feature or on authored key order.
fn canonical(value: toml::Value) -> toml::Value {
    match value {
        toml::Value::Table(table) => toml::Value::Table(
            table
                .into_iter()
                .map(|(key, value)| (key, canonical(value)))
                .collect::<BTreeMap<_, _>>()
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
