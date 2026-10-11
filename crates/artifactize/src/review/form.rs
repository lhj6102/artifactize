//! Owner fields for one verdict: a form for flat schemas, JSON from `$EDITOR` otherwise.

use crossterm::event::{KeyCode, KeyEvent};
use serde_json::{Map, Value, json};

/// Annotations allowed on any schema the form accepts.
const NOTES: [&str; 3] = ["title", "description", "$comment"];
const NUMERIC: [&str; 6] = [
    "type",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
];

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// A `const` value: prefilled and fixed.
    Fixed(Value),
    /// `None` until chosen.
    Bool(Option<bool>),
    /// String `enum` options and the chosen index.
    Choice(Vec<Value>, Option<usize>),
    Text(String),
    Integer(String),
    Number(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub required: bool,
    /// The type and constraints, then any description.
    pub hint: String,
    pub input: Input,
}

impl Field {
    /// The JSON value to submit; `None` leaves an empty optional field out.
    fn value(&self) -> Result<Option<Value>, String> {
        let number = |text: &str, integer: bool| {
            let value: Value = serde_json::from_str(text.trim()).unwrap_or_default();
            let valid = if integer {
                value.is_i64() || value.is_u64()
            } else {
                value.is_number()
            };
            let kind = if integer { "an integer" } else { "a number" };
            valid
                .then_some(value)
                .ok_or_else(|| format!("{}: enter {kind}.", self.name))
        };
        Ok(match &self.input {
            Input::Fixed(value) => Some(value.clone()),
            Input::Bool(value) => value.map(Value::Bool),
            Input::Choice(options, index) => index.map(|index| options[index].clone()),
            Input::Text(text) => (self.required || !text.is_empty()).then(|| json!(text)),
            Input::Integer(text) | Input::Number(text) if text.trim().is_empty() => None,
            Input::Integer(text) => Some(number(text, true)?),
            Input::Number(text) => Some(number(text, false)?),
        })
    }

    /// What the form shows for the current value.
    pub fn display(&self) -> String {
        match &self.input {
            Input::Fixed(value) => format!("{value} (fixed)"),
            Input::Bool(None) | Input::Choice(_, None) => "–".into(),
            Input::Bool(Some(value)) => value.to_string(),
            Input::Choice(options, Some(index)) => options[*index].to_string(),
            Input::Text(text) | Input::Integer(text) | Input::Number(text) => text.clone(),
        }
    }

    fn key(&mut self, key: KeyEvent) {
        let required = self.required;
        match (&mut self.input, key.code) {
            (Input::Text(text) | Input::Integer(text) | Input::Number(text), KeyCode::Char(c)) => {
                text.push(c)
            }
            (
                Input::Text(text) | Input::Integer(text) | Input::Number(text),
                KeyCode::Backspace,
            ) => {
                text.pop();
            }
            (Input::Bool(value), KeyCode::Char('t' | 'y')) => *value = Some(true),
            (Input::Bool(value), KeyCode::Char('f' | 'n')) => *value = Some(false),
            (Input::Bool(value), KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right) => {
                *value = match value {
                    None => Some(true),
                    Some(true) => Some(false),
                    Some(false) if required => Some(true),
                    Some(false) => None,
                }
            }
            (Input::Choice(options, index), KeyCode::Char(' ') | KeyCode::Right) => {
                *index = match *index {
                    None => Some(0),
                    Some(i) if i + 1 < options.len() => Some(i + 1),
                    Some(_) if required => Some(0),
                    Some(_) => None,
                }
            }
            (Input::Choice(options, index), KeyCode::Left) => {
                *index = match *index {
                    Some(0) if !required => None,
                    Some(0) | None => Some(options.len() - 1),
                    Some(i) => Some(i - 1),
                }
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Form {
    pub verdict: crate::runtime::Verdict,
    pub fields: Vec<Field>,
    /// Owner fields as JSON text from `$EDITOR`; once set it replaces `fields`.
    pub json: Option<String>,
    pub selected: usize,
    /// UTF-8 byte boundary of the in-terminal JSON cursor; standalone editor is unchanged.
    pub cursor: usize,
    /// The last local or `human::submit` error.
    pub error: Option<String>,
}

impl Form {
    /// A form for a flat owner schema (or none); any other schema starts from a JSON template.
    pub fn new(verdict: crate::runtime::Verdict, schema: Option<&Value>) -> Self {
        let fields = schema.map_or(Some(Vec::new()), fields);
        let json = fields.is_none().then(|| {
            let template = schema.map_or(json!({}), template);
            serde_json::to_string_pretty(&template).expect("template is JSON")
        });
        Self {
            verdict,
            fields: fields.unwrap_or_default(),
            cursor: json.as_ref().map_or(0, String::len),
            json,
            selected: 0,
            error: None,
        }
    }

    /// JSON to open in `$EDITOR`: the edited text, or the current form values.
    pub fn draft(&self) -> String {
        if let Some(json) = &self.json {
            return json.clone();
        }
        let fields = self.fields.iter().map(|field| {
            let value = match (&field.input, field.value()) {
                (Input::Text(text), _) => json!(text),
                (_, Ok(Some(value))) => value,
                _ => Value::Null,
            };
            (field.name.clone(), value)
        });
        serde_json::to_string_pretty(&Value::Object(fields.collect())).expect("draft is JSON")
    }

    /// The complete result: owner fields plus the chosen verdict.
    pub fn result(&self) -> Result<Value, String> {
        let mut object = match &self.json {
            Some(text) => match serde_json::from_str(text) {
                Ok(Value::Object(object)) => object,
                Ok(_) => return Err("Owner fields must be a JSON object.".into()),
                Err(error) => return Err(format!("Owner fields are not valid JSON: {error}")),
            },
            None => {
                let mut object = Map::new();
                for field in &self.fields {
                    if let Some(value) = field.value()? {
                        object.insert(field.name.clone(), value);
                    }
                }
                object
            }
        };
        if object.contains_key("verdict") {
            return Err("Choose the verdict in the review, not inside the owner fields.".into());
        }
        object.insert("verdict".into(), json!(self.verdict));
        Ok(Value::Object(object))
    }

    /// Replace the JSON text (from `$EDITOR` or a flat form's draft); the cursor moves to its end.
    pub fn set_json(&mut self, text: String) {
        self.cursor = text.len();
        self.json = Some(text);
    }

    /// Bounded paste follows the existing Human result limit, without splitting UTF-8.
    pub fn paste(&mut self, text: &str) {
        if let Some(json) = &mut self.json {
            self.cursor = boundary(json, self.cursor);
            if json.len().saturating_add(text.len()) <= crate::human::MAX_RESULT_BYTES {
                json.insert_str(self.cursor, text);
                self.cursor += text.len();
            } else {
                self.error = Some("Human fields exceed 256000 bytes.".into());
            }
        } else if let Some(Field {
            input: Input::Text(value) | Input::Integer(value) | Input::Number(value),
            ..
        }) = self.fields.get_mut(self.selected)
        {
            if value.len().saturating_add(text.len()) <= crate::human::MAX_RESULT_BYTES {
                value.push_str(text);
            } else {
                self.error = Some("Human fields exceed 256000 bytes.".into());
            }
        }
    }

    /// Multiline editing stays inside monitor. Enter inserts a newline, never submits.
    pub fn inline_key(&mut self, key: KeyEvent) {
        if self.json.is_none() {
            let text_field = self.fields.get(self.selected).is_some_and(|field| {
                matches!(
                    field.input,
                    Input::Text(_) | Input::Integer(_) | Input::Number(_)
                )
            });
            match key.code {
                KeyCode::Char(character)
                    if text_field
                        && !key.modifiers.intersects(
                            crossterm::event::KeyModifiers::CONTROL
                                | crossterm::event::KeyModifiers::ALT,
                        ) =>
                {
                    self.paste(&character.to_string())
                }
                _ if !key.modifiers.intersects(
                    crossterm::event::KeyModifiers::CONTROL | crossterm::event::KeyModifiers::ALT,
                ) =>
                {
                    self.key(key)
                }
                _ => {}
            }
            return;
        }
        let text = self.json.as_ref().expect("JSON mode");
        self.cursor = boundary(text, self.cursor);
        match key.code {
            KeyCode::Char(character)
                if !key.modifiers.intersects(
                    crossterm::event::KeyModifiers::CONTROL | crossterm::event::KeyModifiers::ALT,
                ) =>
            {
                self.paste(&character.to_string())
            }
            KeyCode::Enter => self.paste("\n"),
            KeyCode::Tab => self.paste("  "),
            KeyCode::Left | KeyCode::Backspace if self.cursor > 0 => {
                let previous = text[..self.cursor]
                    .char_indices()
                    .last()
                    .map_or(0, |(index, _)| index);
                if key.code == KeyCode::Backspace {
                    self.json
                        .as_mut()
                        .expect("JSON mode")
                        .drain(previous..self.cursor);
                }
                self.cursor = previous;
            }
            KeyCode::Right | KeyCode::Delete if self.cursor < text.len() => {
                let next = self.cursor
                    + text[self.cursor..]
                        .chars()
                        .next()
                        .expect("not at end")
                        .len_utf8();
                if key.code == KeyCode::Delete {
                    self.json
                        .as_mut()
                        .expect("JSON mode")
                        .drain(self.cursor..next);
                } else {
                    self.cursor = next;
                }
            }
            KeyCode::Home => {
                self.cursor = text[..self.cursor].rfind('\n').map_or(0, |index| index + 1)
            }
            KeyCode::End => {
                self.cursor += text[self.cursor..]
                    .find('\n')
                    .unwrap_or(text.len() - self.cursor)
            }
            KeyCode::Up | KeyCode::Down => {
                let start = text[..self.cursor].rfind('\n').map_or(0, |index| index + 1);
                let column = text[start..self.cursor].chars().count();
                let destination = if key.code == KeyCode::Up {
                    start
                        .checked_sub(1)
                        .map(|end| (text[..end].rfind('\n').map_or(0, |index| index + 1), end))
                } else {
                    text[self.cursor..].find('\n').map(|offset| {
                        let start = self.cursor + offset + 1;
                        let end = start + text[start..].find('\n').unwrap_or(text.len() - start);
                        (start, end)
                    })
                };
                if let Some((start, end)) = destination {
                    self.cursor = start
                        + text[start..end]
                            .char_indices()
                            .nth(column)
                            .map_or(end - start, |(index, _)| index);
                }
            }
            _ => {}
        }
    }

    pub fn key(&mut self, key: KeyEvent) {
        let count = self.fields.len();
        match key.code {
            KeyCode::Up | KeyCode::BackTab if count > 0 => {
                self.selected = (self.selected + count - 1) % count
            }
            KeyCode::Down | KeyCode::Tab if count > 0 => {
                self.selected = (self.selected + 1) % count
            }
            _ if self.json.is_none() => {
                if let Some(field) = self.fields.get_mut(self.selected) {
                    field.key(key);
                }
            }
            _ => {}
        }
    }
}

/// Fields of a flat object schema, or `None` when a form cannot represent it.
fn fields(schema: &Value) -> Option<Vec<Field>> {
    let schema = schema.as_object()?;
    let top = ["type", "properties", "required", "additionalProperties"];
    if schema
        .keys()
        .any(|key| !top.contains(&key.as_str()) && !NOTES.contains(&key.as_str()))
    {
        return None;
    }
    let required: Vec<&str> = match schema.get("required") {
        None => Vec::new(),
        Some(names) => names
            .as_array()?
            .iter()
            .map(Value::as_str)
            .collect::<Option<_>>()?,
    };
    let empty = Map::new();
    let properties = match schema.get("properties") {
        None => &empty,
        Some(properties) => properties.as_object()?,
    };
    properties
        .iter()
        .map(|(name, property)| {
            let (input, hint) = input(property)?;
            Some(Field {
                name: name.clone(),
                required: required.contains(&name.as_str()),
                hint,
                input,
            })
        })
        .collect()
}

fn input(property: &Value) -> Option<(Input, String)> {
    let property = property.as_object()?;
    let only = |keys: &[&str]| {
        property
            .keys()
            .all(|key| keys.contains(&key.as_str()) || NOTES.contains(&key.as_str()))
    };
    let kind = property.get("type").map(Value::as_str);
    let input = if let Some(value) = property.get("const") {
        only(&["const", "type"]).then(|| Input::Fixed(value.clone()))?
    } else if let Some(options) = property.get("enum") {
        let options = options.as_array()?;
        let strings = !options.is_empty() && options.iter().all(Value::is_string);
        (strings && kind.is_none_or(|kind| kind == Some("string")) && only(&["enum", "type"]))
            .then(|| Input::Choice(options.clone(), None))?
    } else {
        match kind?? {
            "boolean" if only(&["type"]) => Input::Bool(None),
            "string" if only(&["type", "minLength", "maxLength"]) => Input::Text(String::new()),
            "integer" if only(&NUMERIC) => Input::Integer(String::new()),
            "number" if only(&NUMERIC) => Input::Number(String::new()),
            _ => return None,
        }
    };
    let constraints = property
        .iter()
        .filter(|(key, _)| !["type", "const", "enum"].contains(&key.as_str()))
        .filter(|(key, _)| !NOTES.contains(&key.as_str()))
        .map(|(key, value)| format!("{key} {value}"));
    let kind = kind.flatten().map(str::to_owned);
    let mut hint = kind
        .into_iter()
        .chain(constraints)
        .collect::<Vec<_>>()
        .join(", ");
    if let Some(description) = property.get("description").and_then(Value::as_str) {
        hint = [hint.as_str(), description]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" — ");
    }
    Some((input, hint))
}

/// A JSON skeleton of a schema for `$EDITOR`: constants and first choices filled, the rest empty.
/// `cursor` moved back onto a UTF-8 character boundary of `text`, at most its end.
pub(crate) fn boundary(text: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(text.len());
    while !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

pub fn template(schema: &Value) -> Value {
    if let Some(value) = ["const", "default"].iter().find_map(|key| schema.get(*key)) {
        return value.clone();
    }
    if let Some(value) = schema.get("enum").and_then(|options| options.get(0)) {
        return value.clone();
    }
    let kind = match &schema["type"] {
        Value::String(kind) => kind.as_str(),
        Value::Array(kinds) => kinds.first().and_then(Value::as_str).unwrap_or_default(),
        _ if schema.get("properties").is_some() => "object",
        _ => "",
    };
    match kind {
        "object" => Value::Object(
            schema["properties"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(name, property)| (name.clone(), template(property)))
                .collect(),
        ),
        "array" => json!([]),
        "string" => json!(""),
        "integer" | "number" => json!(0),
        "boolean" => json!(false),
        _ => Value::Null,
    }
}
