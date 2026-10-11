//! Plain argv templates are tokenized and validated at the declaration boundary.

use serde_json::Value;

#[derive(Debug, Clone)]
pub(crate) struct PlainArgument {
    parts: Vec<Part>,
}

#[derive(Debug, Clone)]
enum Part {
    Literal(String),
    Placeholder(String),
}

impl PlainArgument {
    pub(crate) fn parse(template: &str, schema: &Value) -> Result<Self, String> {
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut rest = template;
        while !rest.is_empty() {
            if rest.starts_with("{{") || rest.starts_with("}}") {
                literal.push(rest.chars().next().unwrap());
                rest = &rest[2..];
            } else if let Some(tail) = rest.strip_prefix('{') {
                let (name, tail) = tail
                    .split_once('}')
                    .ok_or("Unclosed plain tool placeholder.")?;
                if name.is_empty() || name.contains('{') {
                    return Err("Invalid plain tool placeholder.".into());
                }
                if schema["properties"].get(name).is_none() {
                    return Err(format!("Unknown plain tool placeholder: {name}"));
                }
                if !literal.is_empty() {
                    parts.push(Part::Literal(std::mem::take(&mut literal)));
                }
                parts.push(Part::Placeholder(name.to_owned()));
                rest = tail;
            } else {
                let character = rest.chars().next().unwrap();
                if character == '}' {
                    return Err("Unmatched plain tool closing brace.".into());
                }
                literal.push(character);
                rest = &rest[character.len_utf8()..];
            }
        }
        if !literal.is_empty() {
            parts.push(Part::Literal(literal));
        }
        Ok(Self { parts })
    }

    /// Only substitute values; inserted text never becomes another template.
    pub(crate) fn expand(
        &self,
        mut value: impl FnMut(&str) -> Result<String, String>,
    ) -> Result<String, String> {
        self.parts
            .iter()
            .map(|part| match part {
                Part::Literal(text) => Ok(text.clone()),
                Part::Placeholder(name) => value(name),
            })
            .collect()
    }
}
