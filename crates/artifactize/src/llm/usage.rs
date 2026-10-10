//! Numeric provider counters, with opaque legacy metadata preserved only at the JSON edge.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, ops::Deref};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usage {
    counters: BTreeMap<String, u64>,
    metadata: BTreeMap<String, Value>,
}
impl Deref for Usage {
    type Target = BTreeMap<String, u64>;
    fn deref(&self) -> &Self::Target {
        &self.counters
    }
}
impl From<BTreeMap<String, u64>> for Usage {
    fn from(counters: BTreeMap<String, u64>) -> Self {
        Self {
            counters,
            metadata: BTreeMap::new(),
        }
    }
}
impl Serialize for Usage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.counters.len() + self.metadata.len()))?;
        for (name, value) in &self.counters {
            map.serialize_entry(name, value)?;
        }
        for (name, value) in &self.metadata {
            map.serialize_entry(name, value)?;
        }
        map.end()
    }
}
impl<'de> Deserialize<'de> for Usage {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let values = BTreeMap::<String, Value>::deserialize(deserializer)?;
        let mut usage = Self::default();
        for (name, value) in values {
            match value.as_u64() {
                Some(value) => {
                    usage.counters.insert(name, value);
                }
                None => {
                    usage.metadata.insert(name, value);
                }
            }
        }
        Ok(usage)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_metadata_round_trips_without_entering_numeric_totals() {
        let value =
            serde_json::json!({"inputTokens":17,"vendorDetail":{"cache":true},"invalid":-1});
        let usage: Usage = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage["inputTokens"], 17);
        assert_eq!(serde_json::to_value(usage).unwrap(), value);
    }
}
