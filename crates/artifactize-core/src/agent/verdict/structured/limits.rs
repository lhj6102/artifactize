//! Responses' structural limits apply to the complete format, including its envelope.
//! https://developers.openai.com/api/docs/guides/structured-outputs#supported-schemas
use serde_json::Value;

/// Responses accepts at most ten nesting levels, 5,000 object properties and 1,000 enum
/// entries. Conservatively measure its 120,000-character name quota in UTF-8 bytes.
/// Encoded owner JSON preserves the value domain instead of rejecting a valid declaration.
const MAX_DEPTH: usize = 10;
const MAX_PROPERTIES: usize = 5_000;
const MAX_ENUMS: usize = 1_000;
const MAX_NAME_BYTES: usize = 120_000;
/// A string enum larger than 250 entries has a separate 15,000-character quota.
const SMALL_ENUM_ENTRIES: usize = 250;
const MAX_LARGE_ENUM_BYTES: usize = 15_000;

#[derive(Default)]
struct Size {
    depth: usize,
    properties: usize,
    enums: usize,
    names: usize,
    large_enum: usize,
}

pub(super) fn fits(schema: &Value) -> bool {
    let size = measure(schema);
    size.depth <= MAX_DEPTH
        && size.properties <= MAX_PROPERTIES
        && size.enums <= MAX_ENUMS
        && size.names <= MAX_NAME_BYTES
        && size.large_enum <= MAX_LARGE_ENUM_BYTES
}

fn measure(schema: &Value) -> Size {
    let properties = schema.get("properties").and_then(Value::as_object);
    let enums = schema.get("enum").and_then(Value::as_array);
    let enum_bytes = enums
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::len)
        .sum::<usize>();
    let own = Size {
        depth: 1,
        large_enum: if enums.is_some_and(|values| values.len() > SMALL_ENUM_ENTRIES) {
            enum_bytes
        } else {
            0
        },
        properties: properties.map_or(0, |properties| properties.len()),
        enums: enums.map_or(0, |values| values.len()),
        names: properties
            .into_iter()
            .flat_map(|properties| properties.keys())
            .map(String::len)
            .sum::<usize>()
            + enum_bytes
            + schema
                .get("const")
                .and_then(Value::as_str)
                .map_or(0, str::len),
    };
    properties
        .into_iter()
        .flat_map(|properties| properties.values())
        .chain(schema.get("items"))
        .chain(
            schema
                .get("anyOf")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        )
        .map(measure)
        .fold(own, |sum, child| Size {
            depth: sum.depth.max(child.depth + 1),
            properties: sum.properties + child.properties,
            enums: sum.enums + child.enums,
            names: sum.names + child.names,
            large_enum: sum.large_enum.max(child.large_enum),
        })
}
