use std::collections::BTreeSet;

use serde::{Deserialize, Deserializer, de::Error};
use serde_json::Number;

// Optional fields may be absent, but explicit JSON null is not an omission.
pub(super) fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

fn integer<'de, D>(deserializer: D, max: u64, message: &str) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let number = Number::deserialize(deserializer)?;
    // CCDD accepts integral JSON numbers even when written as 1.0 or 1e3.
    let value = number.as_f64().ok_or_else(|| D::Error::custom(message))?;
    if value < 1.0 || value > max as f64 || value.fract() != 0.0 {
        return Err(D::Error::custom(message));
    }
    Ok(value as u64)
}

pub(super) fn positive_integer<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    integer(
        deserializer,
        9_007_199_254_740_991,
        "Expected a positive safe integer (1–9007199254740991).",
    )
    .map(Some)
}

pub(super) fn timeout<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    integer(
        deserializer,
        2_147_483_647,
        "timeoutMs must be an integer from 1 through 2147483647.",
    )
    .map(|value| Some(value as u32))
}

pub(super) fn identifier(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(format!(
            "{label} must match [A-Za-z0-9][A-Za-z0-9_-]{{0,63}}."
        ));
    }
    Ok(())
}

pub(super) fn text(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{label} must be nonblank text."));
    }
    Ok(())
}

pub(super) fn script(command: &str, args: &[String]) -> Result<(), String> {
    text(command, "Script command")?;
    if command.bytes().any(|byte| byte.is_ascii_control())
        || args.iter().any(|arg| arg.contains('\0'))
    {
        return Err("A script requires a fixed command without control characters and string args without NUL.".into());
    }
    Ok(())
}

pub(super) fn path(value: &str) -> Result<(), String> {
    let windows_absolute =
        value.as_bytes().get(1) == Some(&b':') && value.as_bytes().get(2) == Some(&b'/');
    if value.encode_utf16().count() > 1024
        || windows_absolute
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b'\\')
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("Execution input must be a safe project-relative path.".into());
    }
    Ok(())
}

pub(super) fn paths(values: &[String], label: &str) -> Result<(), String> {
    if values.len() > 64 || values.iter().collect::<BTreeSet<_>>().len() != values.len() {
        return Err(format!(
            "{label} must contain at most 64 unique project-relative paths."
        ));
    }
    for value in values {
        path(value).map_err(|message| format!("{label}: {message}"))?;
    }
    Ok(())
}
