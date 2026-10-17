//! Owner fields for one verdict, as a person enters them. A Human eval's result schemas may
//! use only shapes a form can enter ([`check`] refuses any other when the declaration loads):
//! an object whose properties are strings, integers, numbers, booleans, string choices and
//! constants, lists of strings, and lists of flat objects of those scalars (repeated groups).

use serde_json::{Map, Value, json};

/// Annotations allowed on any schema the form accepts.
const NOTES: [&str; 3] = ["title", "description", "$comment"];
/// Keys of the result object itself, and of each object in a list of groups.
const OBJECT: [&str; 4] = ["type", "properties", "required", "additionalProperties"];
const STRING: [&str; 3] = ["type", "minLength", "maxLength"];
const NUMERIC: [&str; 6] = [
    "type",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "multipleOf",
];
const ARRAY: [&str; 4] = ["type", "items", "minItems", "maxItems"];
/// What a Human result schema may hold, as a declaration error says it.
const SHAPES: &str = "a Human result field is a string (with minLength or maxLength), an \
integer or number, a boolean, a string enum, a const, an array of strings, or an array of \
objects whose properties are those scalars";

/// A key as the form edits with it; each interface maps its own key events onto these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Enter,
    Tab,
    BackTab,
    /// A key the form does not use.
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// A `const` value: filled in and fixed.
    Fixed(Value),
    /// `None` until chosen.
    Bool(Option<bool>),
    /// String `enum` options and the chosen index.
    Choice(Vec<String>, Option<usize>),
    Text(String),
    Integer(String),
    Number(String),
    /// An array of strings, one entry per item; empty entries are left out of the result.
    List(Vec<String>),
    /// An array of flat objects: one group of fields per item. `fields` is an empty item;
    /// an item with nothing entered is left out of the result.
    Groups {
        fields: Vec<Field>,
        items: Vec<Vec<Field>>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub required: bool,
    /// The type and constraints, then any description.
    pub hint: String,
    pub input: Input,
}

/// One row a terminal form moves through: a field, a field of one group, or a group list's
/// `+ Add`. Constants are filled in and get no row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Field(usize),
    Item {
        field: usize,
        item: usize,
        sub: usize,
    },
    Add(usize),
}

impl Field {
    /// The JSON value to submit, named `label` in errors; `None` leaves an empty optional
    /// field out.
    fn value(&self, label: &str) -> Result<Option<Value>, String> {
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
                .ok_or_else(|| format!("{label}: enter {kind}."))
        };
        Ok(match &self.input {
            Input::Fixed(value) => Some(value.clone()),
            Input::Bool(value) => value.map(Value::Bool),
            Input::Choice(options, index) => {
                index.map(|index| Value::String(options[index].clone()))
            }
            Input::Text(text) => (self.required || !text.is_empty()).then(|| json!(text)),
            Input::Integer(text) | Input::Number(text) if text.trim().is_empty() => None,
            Input::Integer(text) => Some(number(text, true)?),
            Input::Number(text) => Some(number(text, false)?),
            Input::List(items) => {
                let items: Vec<_> = items
                    .iter()
                    .filter(|item| !item.trim().is_empty())
                    .collect();
                (self.required || !items.is_empty()).then(|| json!(items))
            }
            Input::Groups { items, .. } => {
                let mut values = Vec::new();
                for (at, item) in items.iter().enumerate().filter(|(_, item)| entered(item)) {
                    let mut object = Map::new();
                    for field in item {
                        let label = format!("{label} {} · {}", at + 1, field.name);
                        if let Some(value) = field.value(&label)? {
                            object.insert(field.name.clone(), value);
                        }
                    }
                    values.push(Value::Object(object));
                }
                (self.required || !values.is_empty()).then_some(Value::Array(values))
            }
        })
    }

    /// Nothing is entered yet.
    fn empty(&self) -> bool {
        match &self.input {
            Input::Fixed(_) => true,
            Input::Bool(value) => value.is_none(),
            Input::Choice(_, index) => index.is_none(),
            Input::Text(text) | Input::Integer(text) | Input::Number(text) => {
                text.trim().is_empty()
            }
            Input::List(items) => items.iter().all(|item| item.trim().is_empty()),
            Input::Groups { items, .. } => !items.iter().any(|item| entered(item)),
        }
    }

    /// What the form shows for the current value.
    pub fn display(&self) -> String {
        match &self.input {
            Input::Fixed(value) => format!("{value} (fixed)"),
            Input::Bool(None) | Input::Choice(_, None) => "–".into(),
            Input::Bool(Some(value)) => value.to_string(),
            Input::Choice(options, Some(index)) => {
                Value::String(options[*index].clone()).to_string()
            }
            Input::Text(text) | Input::Integer(text) | Input::Number(text) => text.clone(),
            Input::List(items) if items.iter().all(String::is_empty) => "–".into(),
            Input::List(items) => items.join("; "),
            Input::Groups { items, .. } => format!("{} item(s)", items.len()),
        }
    }

    fn text_like(&self) -> bool {
        matches!(
            self.input,
            Input::Text(_) | Input::Integer(_) | Input::Number(_) | Input::List(_)
        )
    }

    /// Append pasted or typed text, bounded like the Human result.
    fn paste(&mut self, text: &str) -> Result<(), String> {
        let fits = |size: usize| size.saturating_add(text.len()) <= crate::human::MAX_RESULT_BYTES;
        let too_big = || {
            format!(
                "Human fields exceed {} bytes.",
                crate::human::MAX_RESULT_BYTES
            )
        };
        match &mut self.input {
            Input::List(items) => {
                if !fits(items.iter().map(String::len).sum()) {
                    return Err(too_big());
                }
                match items.last_mut() {
                    Some(item) => item.push_str(text),
                    None => items.push(text.to_owned()),
                }
            }
            Input::Text(value) | Input::Integer(value) | Input::Number(value) => {
                if !fits(value.len()) {
                    return Err(too_big());
                }
                value.push_str(text);
            }
            _ => {}
        }
        Ok(())
    }

    fn key(&mut self, key: Key) {
        let required = self.required;
        match (&mut self.input, key) {
            (Input::Text(text) | Input::Integer(text) | Input::Number(text), Key::Char(c)) => {
                text.push(c)
            }
            (Input::Text(text) | Input::Integer(text) | Input::Number(text), Key::Backspace) => {
                text.pop();
            }
            // A list types into its last item; Enter starts the next, Backspace on an empty
            // item removes it.
            (Input::List(items), Key::Char(c)) => match items.last_mut() {
                Some(item) => item.push(c),
                None => items.push(c.to_string()),
            },
            (Input::List(items), Key::Enter) => items.push(String::new()),
            (Input::List(items), Key::Backspace) => {
                if items.last().is_some_and(String::is_empty) && items.len() > 1 {
                    items.pop();
                } else if let Some(item) = items.last_mut() {
                    item.pop();
                }
            }
            (Input::Bool(value), Key::Char('t' | 'y')) => *value = Some(true),
            (Input::Bool(value), Key::Char('f' | 'n')) => *value = Some(false),
            (Input::Bool(value), Key::Char(' ') | Key::Left | Key::Right) => {
                *value = match value {
                    None => Some(true),
                    Some(true) => Some(false),
                    Some(false) if required => Some(true),
                    Some(false) => None,
                }
            }
            (Input::Choice(options, index), Key::Char(' ') | Key::Right) => {
                *index = match *index {
                    None => Some(0),
                    Some(i) if i + 1 < options.len() => Some(i + 1),
                    Some(_) if required => Some(0),
                    Some(_) => None,
                }
            }
            (Input::Choice(options, index), Key::Left) => {
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

/// Something is entered in a group.
fn entered(item: &[Field]) -> bool {
    item.iter().any(|field| !field.empty())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Form {
    pub verdict: crate::runtime::Verdict,
    pub fields: Vec<Field>,
    /// The selected row of [`Form::rows`].
    pub selected: usize,
    /// The last local or `human::submit` error.
    pub error: Option<String>,
}

impl Form {
    /// The form of `verdict` for its owner schema, or none. A schema a form cannot enter is
    /// refused, as [`check`] refuses it when the declaration loads.
    pub fn new(verdict: crate::runtime::Verdict, schema: Option<&Value>) -> Result<Self, String> {
        let fields = match schema {
            None => Vec::new(),
            Some(Value::Object(schema)) => fields(schema, "")?,
            Some(_) => return Err("A Human result schema must be a JSON object.".into()),
        };
        Ok(Self {
            verdict,
            fields,
            selected: 0,
            error: None,
        })
    }

    /// The complete result: owner fields plus the chosen verdict.
    pub fn result(&self) -> Result<Value, String> {
        let mut object = Map::new();
        for field in &self.fields {
            if let Some(value) = field.value(&field.name)? {
                object.insert(field.name.clone(), value);
            }
        }
        if object.contains_key("verdict") {
            return Err("Choose the verdict in the review, not inside the owner fields.".into());
        }
        object.insert("verdict".into(), json!(self.verdict));
        Ok(Value::Object(object))
    }

    /// The rows a person moves through, in order.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for (index, field) in self.fields.iter().enumerate() {
            match &field.input {
                Input::Fixed(_) => {}
                Input::Groups { items, .. } => {
                    for (item, fields) in items.iter().enumerate() {
                        let subs = fields.iter().enumerate();
                        rows.extend(
                            subs.filter(|(_, sub)| !matches!(sub.input, Input::Fixed(_)))
                                .map(|(sub, _)| Row::Item {
                                    field: index,
                                    item,
                                    sub,
                                }),
                        );
                    }
                    rows.push(Row::Add(index));
                }
                _ => rows.push(Row::Field(index)),
            }
        }
        rows
    }

    /// The selected row, if the form has any.
    pub fn row(&self) -> Option<Row> {
        self.rows().get(self.selected).copied()
    }

    /// The field a row edits.
    pub fn field(&self, row: Row) -> Option<&Field> {
        match row {
            Row::Field(index) | Row::Add(index) => self.fields.get(index),
            Row::Item { field, item, sub } => match &self.fields.get(field)?.input {
                Input::Groups { items, .. } => items.get(item)?.get(sub),
                _ => None,
            },
        }
    }

    fn field_mut(&mut self, row: Row) -> Option<&mut Field> {
        match row {
            Row::Field(index) => self.fields.get_mut(index),
            Row::Add(_) => None,
            Row::Item { field, item, sub } => match &mut self.fields.get_mut(field)?.input {
                Input::Groups { items, .. } => items.get_mut(item)?.get_mut(sub),
                _ => None,
            },
        }
    }

    /// Add an empty item to the group list of field `index`; the selection moves into it.
    pub fn add(&mut self, index: usize) {
        let Some(Input::Groups { fields, items }) =
            self.fields.get_mut(index).map(|field| &mut field.input)
        else {
            return;
        };
        items.push(fields.clone());
        let item = items.len() - 1;
        if let Some(at) = self.rows().iter().position(
            |row| matches!(row, Row::Item { field, item: i, .. } if *field == index && *i == item),
        ) {
            self.selected = at;
        }
    }

    /// Remove item `item` of the group list of field `index`; one empty item always stays.
    pub fn remove(&mut self, index: usize, item: usize) {
        let Some(Input::Groups { fields, items }) =
            self.fields.get_mut(index).map(|field| &mut field.input)
        else {
            return;
        };
        if item < items.len() {
            items.remove(item);
        }
        if items.is_empty() {
            items.push(fields.clone());
        }
        self.selected = self.selected.min(self.rows().len().saturating_sub(1));
    }

    /// Remove the group the selection is in.
    pub fn remove_selected(&mut self) {
        if let Some(Row::Item { field, item, .. }) = self.row() {
            self.remove(field, item);
        }
    }

    /// Bounded paste follows the existing Human result limit, without splitting UTF-8.
    pub fn paste(&mut self, text: &str) {
        let Some(row) = self.row() else {
            return;
        };
        if let Some(field) = self.field_mut(row)
            && let Err(error) = field.paste(text)
        {
            self.error = Some(error);
        }
    }

    /// A key typed into the form. Enter never submits: on `+ Add` it adds a group, in a list
    /// it starts the next item. Keys with Control or Alt are not the form's.
    pub fn inline_key(&mut self, key: Key, modified: bool) {
        if modified {
            return;
        }
        let row = self.row();
        let text = row
            .and_then(|row| self.field(row))
            .is_some_and(Field::text_like);
        match (key, row) {
            (Key::Char(character), _) if text => self.paste(&character.to_string()),
            (Key::Enter | Key::Char(' '), Some(Row::Add(index))) => self.add(index),
            _ => self.key(key),
        }
    }

    pub fn key(&mut self, key: Key) {
        let count = self.rows().len();
        match key {
            Key::Up | Key::BackTab if count > 0 => {
                self.selected = (self.selected + count - 1) % count
            }
            Key::Down | Key::Tab if count > 0 => self.selected = (self.selected + 1) % count,
            _ => {
                if let Some(row) = self.row()
                    && let Some(field) = self.field_mut(row)
                {
                    field.key(key);
                }
            }
        }
    }
}

/// Whether a Human eval may declare `schema` as a result schema: only shapes a form can
/// enter. The error names the offending key and what is allowed instead.
pub fn check(schema: &Map<String, Value>) -> Result<(), String> {
    fields(schema, "").map(drop)
}

/// The fields of an object schema at `path` (empty for the result itself).
fn fields(schema: &Map<String, Value>, path: &str) -> Result<Vec<Field>, String> {
    let at = |key: &str| {
        if path.is_empty() {
            key.to_owned()
        } else {
            format!("{path}.{key}")
        }
    };
    if let Some(key) = schema
        .keys()
        .find(|key| !OBJECT.contains(&key.as_str()) && !NOTES.contains(&key.as_str()))
    {
        let what = if path.is_empty() {
            "A Human result schema"
        } else {
            "An object in a Human result list"
        };
        return Err(format!(
            "{what} may use only type, properties, required and additionalProperties (with \
             title, description and $comment); `{}` is not allowed.",
            at(key)
        ));
    }
    if let Some(kind) = schema.get("type")
        && kind != "object"
    {
        return Err(format!("`{}` must be \"object\".", at("type")));
    }
    if schema
        .get("additionalProperties")
        .is_some_and(|value| !value.is_boolean())
    {
        return Err(format!(
            "`{}` must be true or false in a Human result schema.",
            at("additionalProperties")
        ));
    }
    let empty = Map::new();
    let properties = match schema.get("properties") {
        None => &empty,
        Some(Value::Object(properties)) => properties,
        Some(_) => return Err(format!("`{}` must be an object.", at("properties"))),
    };
    let required: Vec<&str> = match schema.get("required") {
        None => Vec::new(),
        Some(Value::Array(names)) => names
            .iter()
            .map(Value::as_str)
            .collect::<Option<_>>()
            .ok_or_else(|| format!("`{}` must list property names.", at("required")))?,
        Some(_) => return Err(format!("`{}` must list property names.", at("required"))),
    };
    if let Some(name) = required
        .iter()
        .find(|name| !properties.contains_key(**name))
    {
        return Err(format!(
            "`{}` names `{name}`, which `{}` does not declare, so a reviewer could never enter it.",
            at("required"),
            at("properties")
        ));
    }
    properties
        .iter()
        .map(|(name, property)| {
            let nested = !path.is_empty();
            let path = at(&format!("properties.{name}"));
            // An object below the result is a group, whose fields are scalars.
            let (input, hint) = input(property, &path, nested)?;
            Ok(Field {
                name: name.clone(),
                required: required.contains(&name.as_str()),
                hint,
                input,
            })
        })
        .collect()
}

/// The input of the property at `path`, and its hint. Inside a group (`nested`) only scalars
/// are allowed.
fn input(property: &Value, path: &str, nested: bool) -> Result<(Input, String), String> {
    let refuse = |detail: &str| Err(format!("`{path}`: {detail}; {SHAPES}."));
    let Some(property) = property.as_object() else {
        return refuse("a property schema must be an object");
    };
    let only = |keys: &[&str]| match property
        .keys()
        .find(|key| !keys.contains(&key.as_str()) && !NOTES.contains(&key.as_str()))
    {
        Some(key) => Err(format!(
            "`{path}`: `{key}` is not allowed here; {SHAPES}, and only {} may be used on it.",
            keys.join(", ")
        )),
        None => Ok(()),
    };
    let kind = property.get("type");
    let input = if let Some(value) = property.get("const") {
        only(&["const", "type"])?;
        Input::Fixed(value.clone())
    } else if let Some(options) = property.get("enum") {
        only(&["enum", "type"])?;
        let strings: Option<Vec<String>> = options.as_array().and_then(|options| {
            options
                .iter()
                .map(|option| option.as_str().map(str::to_owned))
                .collect()
        });
        match strings {
            Some(strings) if !strings.is_empty() && kind.is_none_or(|kind| kind == "string") => {
                Input::Choice(strings, None)
            }
            _ => return refuse("an enum lists one or more strings"),
        }
    } else {
        if kind.is_some_and(|kind| !kind.is_string()) {
            return refuse("`type` names a single type");
        }
        match kind.and_then(Value::as_str) {
            Some("boolean") => {
                only(&["type"])?;
                Input::Bool(None)
            }
            Some("string") => {
                only(&STRING)?;
                Input::Text(String::new())
            }
            Some("integer") => {
                only(&NUMERIC)?;
                Input::Integer(String::new())
            }
            Some("number") => {
                only(&NUMERIC)?;
                Input::Number(String::new())
            }
            Some("array") if nested => return refuse("a list cannot hold another list"),
            Some("array") => {
                only(&ARRAY)?;
                array(property, path)?
            }
            Some(other) => return refuse(&format!("type \"{other}\" is not one of them")),
            None => return refuse("a property needs a type, an enum or a const"),
        }
    };
    Ok((input, hint(property)))
}

/// A list of strings, or of groups: `items` is a string schema or a flat object schema.
fn array(property: &Map<String, Value>, path: &str) -> Result<Input, String> {
    let path = format!("{path}.items");
    let items = match property.get("items") {
        Some(Value::Object(items)) => items,
        _ => {
            return Err(format!(
                "`{path}` must be a string schema or an object schema; {SHAPES}."
            ));
        }
    };
    match items.get("type").and_then(Value::as_str) {
        Some("string") => {
            if let Some(key) = items
                .keys()
                .find(|key| !STRING.contains(&key.as_str()) && !NOTES.contains(&key.as_str()))
            {
                return Err(format!(
                    "`{path}`: `{key}` is not allowed here; the items of a list of strings may \
                     use only minLength and maxLength."
                ));
            }
            Ok(Input::List(vec![String::new()]))
        }
        Some("object") => {
            let fields = fields(items, &path)?;
            Ok(Input::Groups {
                items: vec![fields.clone()],
                fields,
            })
        }
        _ => Err(format!(
            "`{path}` must be a string schema or an object schema; {SHAPES}."
        )),
    }
}

/// The type and constraints of a property, then any description.
fn hint(property: &Map<String, Value>) -> String {
    let constraints = property
        .iter()
        .filter(|(key, _)| !["type", "const", "enum", "items"].contains(&key.as_str()))
        .filter(|(key, _)| !NOTES.contains(&key.as_str()))
        .map(|(key, value)| format!("{key} {value}"));
    let kind = property
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let hint = kind
        .into_iter()
        .chain(constraints)
        .collect::<Vec<_>>()
        .join(", ");
    match property.get("description").and_then(Value::as_str) {
        Some(description) if hint.is_empty() => description.to_owned(),
        Some(description) => format!("{hint} — {description}"),
        None => hint,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Verdict;

    fn schema(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn human_schemas_take_scalars_lists_of_strings_and_lists_of_flat_objects() {
        let accepted = json!({
            "type": "object",
            "title": "Review",
            "properties": {
                "approved": {"const": true},
                "comment": {"type": "string", "maxLength": 200, "description": "Why"},
                "count": {"type": "integer", "minimum": 0},
                "score": {"type": "number"},
                "ok": {"type": "boolean"},
                "level": {"enum": ["low", "high"]},
                "changes": {"type": "array", "items": {"type": "string", "minLength": 1}, "minItems": 1},
                "findings": {"type": "array", "maxItems": 5, "items": {
                    "type": "object",
                    "properties": {"line": {"type": "integer"}, "problem": {"type": "string"}},
                    "required": ["problem"],
                    "additionalProperties": false
                }}
            },
            "required": ["approved"],
            "additionalProperties": false
        });
        check(&schema(accepted)).unwrap();
        for (refused, says) in [
            (json!({"oneOf": []}), "`oneOf` is not allowed"),
            (json!({"type": "array"}), "`type` must be \"object\""),
            (
                json!({"properties": {"a": {"type": "object"}}}),
                "`properties.a`: type \"object\"",
            ),
            (
                json!({"properties": {"a": {"type": "string", "pattern": "x"}}}),
                "`pattern` is not allowed",
            ),
            (
                json!({"properties": {"a": {"enum": [1, 2]}}}),
                "an enum lists one or more strings",
            ),
            (
                json!({"properties": {"a": {"type": "array", "items": {"type": "integer"}}}}),
                "`properties.a.items` must be",
            ),
            (
                json!({"properties": {"a": {"type": "array", "items": {"type": "object", "properties": {"b": {"type": "array", "items": {"type": "string"}}}}}}}),
                "`properties.a.items.properties.b`: a list cannot hold another list",
            ),
            (
                json!({"properties": {"a": {"type": "string"}}, "required": ["b"]}),
                "names `b`",
            ),
            (
                json!({"additionalProperties": {"type": "string"}}),
                "true or false",
            ),
        ] {
            let error = check(&schema(refused.clone())).unwrap_err();
            assert!(error.contains(says), "{refused}: {error}");
        }
    }

    fn groups_form() -> Form {
        Form::new(
            Verdict::Red,
            Some(&json!({
                "type": "object",
                "properties": {
                    "kind": {"const": "review"},
                    "findings": {"type": "array", "items": {
                        "type": "object",
                        "properties": {
                            "tag": {"const": "x"},
                            "line": {"type": "integer"},
                            "problem": {"type": "string"}
                        },
                        "required": ["problem"]
                    }}
                },
                "required": ["findings"]
            })),
        )
        .unwrap()
    }

    #[test]
    fn groups_are_rows_to_type_into_with_an_add_row_and_empty_groups_are_left_out() {
        // Properties come in name order: findings, kind; line, problem, tag.
        let mut form = groups_form();
        assert_eq!(
            form.rows(),
            [
                Row::Item {
                    field: 0,
                    item: 0,
                    sub: 0
                },
                Row::Item {
                    field: 0,
                    item: 0,
                    sub: 1
                },
                Row::Add(0),
            ]
        );
        form.inline_key(Key::Char('7'), false);
        form.key(Key::Down);
        form.paste("typo");
        form.key(Key::Down);
        form.inline_key(Key::Enter, false);
        assert_eq!(
            form.row(),
            Some(Row::Item {
                field: 0,
                item: 1,
                sub: 0
            })
        );
        assert_eq!(
            form.result().unwrap(),
            json!({"kind": "review", "verdict": "RED", "findings": [
                {"tag": "x", "line": 7, "problem": "typo"}
            ]})
        );
        form.paste("x");
        assert_eq!(
            form.result().unwrap_err(),
            "findings 2 · line: enter an integer."
        );
        form.remove_selected();
        form.remove(0, 0);
        assert_eq!(form.rows().len(), 3, "one empty group always stays");
        assert_eq!(
            form.result().unwrap(),
            json!({"kind": "review", "verdict": "RED", "findings": []})
        );
    }

    #[test]
    fn a_schema_a_form_cannot_enter_is_refused_rather_than_shown() {
        let error = Form::new(
            Verdict::Green,
            Some(&json!({"properties": {"a": {"type": "object"}}})),
        )
        .unwrap_err();
        assert!(error.contains("properties.a"), "{error}");
        assert_eq!(
            Form::new(Verdict::Green, None).unwrap().result().unwrap(),
            json!({"verdict": "GREEN"})
        );
    }
}
