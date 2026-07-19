//! Defensive normalization for provider-controlled JSON metadata.

use serde_json::{Map, Value};

use super::protocol::sanitize_provider_text_with_secrets;

const MAX_METADATA_DEPTH: usize = 4;
const MAX_METADATA_NODES: usize = 128;
const MAX_OBJECT_FIELDS: usize = 32;
const MAX_ARRAY_ITEMS: usize = 32;
const MAX_KEY_CHARS: usize = 64;
const MAX_STRING_CHARS: usize = 512;

pub(crate) struct SanitizedMetadata {
    pub(crate) value: Value,
    pub(crate) truncated: bool,
}

/// Bounds untrusted provider metadata and redacts configured credentials.
///
/// Metadata is auxiliary: an oversized or unexpectedly nested value must not
/// make otherwise useful search results fail. The sanitizer therefore keeps a
/// deterministic prefix and reports whether information was truncated.
pub(crate) fn sanitize_provider_metadata(value: Value, secrets: &[&str]) -> SanitizedMetadata {
    let mut sanitizer = MetadataSanitizer {
        secrets,
        remaining_nodes: MAX_METADATA_NODES,
        truncated: false,
    };
    let value = sanitizer.sanitize(value, 0).unwrap_or(Value::Null);
    SanitizedMetadata {
        value,
        truncated: sanitizer.truncated,
    }
}

struct MetadataSanitizer<'a> {
    secrets: &'a [&'a str],
    remaining_nodes: usize,
    truncated: bool,
}

impl MetadataSanitizer<'_> {
    fn sanitize(&mut self, value: Value, depth: usize) -> Option<Value> {
        if self.remaining_nodes == 0 {
            self.truncated = true;
            return None;
        }
        self.remaining_nodes -= 1;

        match value {
            Value::Null | Value::Bool(_) | Value::Number(_) => Some(value),
            Value::String(value) => Some(Value::String(self.sanitize_string(value))),
            Value::Array(values) => self.sanitize_array(values, depth),
            Value::Object(values) => self.sanitize_object(values, depth),
        }
    }

    fn sanitize_string(&mut self, value: String) -> String {
        if value.chars().count() > MAX_STRING_CHARS {
            self.truncated = true;
        }
        sanitize_provider_text_with_secrets(&value, MAX_STRING_CHARS, self.secrets)
    }

    fn sanitize_array(&mut self, values: Vec<Value>, depth: usize) -> Option<Value> {
        if depth >= MAX_METADATA_DEPTH {
            self.truncated = true;
            return None;
        }
        if values.len() > MAX_ARRAY_ITEMS {
            self.truncated = true;
        }

        let values = values
            .into_iter()
            .take(MAX_ARRAY_ITEMS)
            .filter_map(|value| self.sanitize(value, depth + 1))
            .collect();
        Some(Value::Array(values))
    }

    fn sanitize_object(&mut self, values: Map<String, Value>, depth: usize) -> Option<Value> {
        if depth >= MAX_METADATA_DEPTH {
            self.truncated = true;
            return None;
        }

        let mut entries: Vec<_> = values.into_iter().collect();
        entries.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        if entries.len() > MAX_OBJECT_FIELDS {
            self.truncated = true;
        }

        let mut sanitized = Map::new();
        for (key, value) in entries.into_iter().take(MAX_OBJECT_FIELDS) {
            if key.chars().count() > MAX_KEY_CHARS {
                self.truncated = true;
            }
            let key = sanitize_provider_text_with_secrets(&key, MAX_KEY_CHARS, self.secrets);
            if key.is_empty() {
                self.truncated = true;
                continue;
            }
            let Some(value) = self.sanitize(value, depth + 1) else {
                continue;
            };
            if sanitized.insert(key, value).is_some() {
                self.truncated = true;
            }
        }
        Some(Value::Object(sanitized))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn preserves_small_metadata_without_a_truncation_signal() {
        let metadata =
            sanitize_provider_metadata(json!({"search_depth": "advanced", "automatic": true}), &[]);

        assert_eq!(
            metadata.value,
            json!({"automatic": true, "search_depth": "advanced"})
        );
        assert!(!metadata.truncated);
    }

    #[test]
    fn redacts_secrets_in_keys_and_values() {
        let metadata = sanitize_provider_metadata(
            json!({"key-secret": {"credential": "Bearer secret"}}),
            &["secret"],
        );
        let serialized = serde_json::to_string(&metadata.value).unwrap();

        assert!(!serialized.contains("secret"));
        assert!(serialized.contains("[REDACTED]"));
    }
}
