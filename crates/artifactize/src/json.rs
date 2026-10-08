//! Compatibility for optional fields at external JSON boundaries that historically ignored
//! a wrong type. Known values are typed; unrelated JSON is consumed, never inspected or retained.
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, SeqAccess, Visitor, value::MapAccessDeserializer},
};

/// JSON objects only: derived structs also accept positional arrays, which these wire
/// boundaries never did. Deserialize fields directly without building a generic JSON tree.
pub(crate) struct Object<T>(pub T);
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Object<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for ObjectVisitor<T> {
            type Value = Object<T>;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a JSON object")
            }
            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
                T::deserialize(MapAccessDeserializer::new(map)).map(Object)
            }
        }
        deserializer.deserialize_map(ObjectVisitor(std::marker::PhantomData))
    }
}

/// Discard unrelated JSON without building a tree, but still validate primitives and
/// recurse through the deserializer's normal depth limit. IgnoredAny skips number range
/// validation, unlike the former Value edge; deserialize_any preserves those refusals.
pub(crate) struct Ignored;
impl<'de> Deserialize<'de> for Ignored {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct IgnoredVisitor;
        impl<'de> Visitor<'de> for IgnoredVisitor {
            type Value = Ignored;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a JSON value")
            }
            fn visit_bool<E>(self, _: bool) -> Result<Ignored, E> {
                Ok(Ignored)
            }
            fn visit_i64<E>(self, _: i64) -> Result<Ignored, E> {
                Ok(Ignored)
            }
            fn visit_u64<E>(self, _: u64) -> Result<Ignored, E> {
                Ok(Ignored)
            }
            fn visit_f64<E>(self, _: f64) -> Result<Ignored, E> {
                Ok(Ignored)
            }
            fn visit_str<E>(self, _: &str) -> Result<Ignored, E> {
                Ok(Ignored)
            }
            fn visit_string<E>(self, _: String) -> Result<Ignored, E> {
                Ok(Ignored)
            }
            fn visit_unit<E>(self) -> Result<Ignored, E> {
                Ok(Ignored)
            }
            fn visit_none<E>(self) -> Result<Ignored, E> {
                Ok(Ignored)
            }
            fn visit_seq<S: SeqAccess<'de>>(self, mut sequence: S) -> Result<Ignored, S::Error> {
                while sequence.next_element::<Ignored>()?.is_some() {}
                Ok(Ignored)
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Ignored, M::Error> {
                while map.next_key::<String>()?.is_some() {
                    map.next_value::<Ignored>()?;
                }
                Ok(Ignored)
            }
        }
        deserializer.deserialize_any(IgnoredVisitor)
    }
}

pub(crate) struct Optional<T>(pub Option<T>);
impl<T> Default for Optional<T> {
    fn default() -> Self {
        Self(None)
    }
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Optional<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Field<T> {
            Known(T),
            Ignored(Ignored),
        }
        Ok(Self(match Field::deserialize(deserializer)? {
            Field::Known(value) => Some(value),
            Field::Ignored(_) => None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_discard_keeps_json_number_and_recursion_refusals_without_a_tree() {
        for input in [
            "1e400",
            "{\"unknown\":[1e400]}",
            "{\"number\":1e400,\"number\":null}",
        ] {
            assert!(serde_json::from_str::<serde_json::Value>(input).is_err());
            assert!(serde_json::from_str::<Ignored>(input).is_err());
        }
        for input in [
            "18446744073709551616",
            "-9223372036854775809",
            "{\"unknown\":[null,true,0,1.5,\"s\"]}",
        ] {
            assert!(serde_json::from_str::<serde_json::Value>(input).is_ok());
            assert!(serde_json::from_str::<Ignored>(input).is_ok());
        }
        let deep = format!("{}null{}", "[".repeat(200), "]".repeat(200));
        assert!(serde_json::from_str::<serde_json::Value>(&deep).is_err());
        assert!(serde_json::from_str::<Ignored>(&deep).is_err());
    }
}
