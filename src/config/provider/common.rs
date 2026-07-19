//! Shared ACL provider parsing helpers.

use std::collections::BTreeMap;
use std::time::Duration;

use a3s_acl::ast::{Block, Value};
use serde_json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};
use url::Url;

use crate::providers::{AnySearchConfig, CredentialSource, ProviderHttpConfig, TavilyConfig};
use crate::{Result, SearchError};

use super::super::MAX_ACL_EXACT_INTEGER;
use super::{COMMON_ATTRIBUTES, MAX_JSON_DEPTH};

pub(super) fn apply_anysearch_http_config(
    config: AnySearchConfig,
    block: &Block,
    provider: &str,
) -> Result<AnySearchConfig> {
    match provider_http_config(block, provider)? {
        Some(http) => Ok(config.with_http_config(http)),
        None => Ok(config),
    }
}

pub(super) fn apply_tavily_http_config(
    config: TavilyConfig,
    block: &Block,
    provider: &str,
) -> Result<TavilyConfig> {
    match provider_http_config(block, provider)? {
        Some(http) => Ok(config.with_http_config(http)),
        None => Ok(config),
    }
}

fn provider_http_config(block: &Block, provider: &str) -> Result<Option<ProviderHttpConfig>> {
    let timeout = optional_u64(block, provider, "http_timeout")?;
    let max_response_bytes = optional_u64(block, provider, "max_response_bytes")?;
    if timeout == Some(0) {
        return Err(config_error(
            provider,
            "attribute \"http_timeout\" must be greater than zero",
        ));
    }
    if max_response_bytes == Some(0) {
        return Err(config_error(
            provider,
            "attribute \"max_response_bytes\" must be greater than zero",
        ));
    }
    if timeout.is_none() && max_response_bytes.is_none() {
        return Ok(None);
    }

    let mut http = ProviderHttpConfig::default();
    if let Some(timeout) = timeout {
        http = http.with_timeout(Duration::from_secs(timeout));
    }
    if let Some(max_response_bytes) = max_response_bytes {
        let max_response_bytes = usize::try_from(max_response_bytes).map_err(|_| {
            config_error(
                provider,
                "attribute \"max_response_bytes\" exceeds this platform's supported size",
            )
        })?;
        http = http.with_max_response_bytes(max_response_bytes);
    }
    Ok(Some(http))
}

pub(super) fn reject_unknown_attributes(
    block: &Block,
    provider: &str,
    provider_attributes: &[&str],
) -> Result<()> {
    for attribute in block.attributes.keys() {
        if !COMMON_ATTRIBUTES.contains(&attribute.as_str())
            && !provider_attributes.contains(&attribute.as_str())
        {
            return Err(config_error(
                provider,
                format!("unknown attribute \"{attribute}\""),
            ));
        }
    }
    Ok(())
}

pub(super) fn optional_bool(
    block: &Block,
    provider: &str,
    attribute: &str,
) -> Result<Option<bool>> {
    block
        .attributes
        .get(attribute)
        .map(|value| match value {
            Value::Bool(value) => Ok(*value),
            _ => Err(attribute_type_error(provider, attribute, "a boolean")),
        })
        .transpose()
}

pub(super) fn optional_number(
    block: &Block,
    provider: &str,
    attribute: &str,
) -> Result<Option<f64>> {
    block
        .attributes
        .get(attribute)
        .map(|value| match value {
            Value::Number(value) => Ok(*value),
            _ => Err(attribute_type_error(provider, attribute, "a number")),
        })
        .transpose()
}

pub(super) fn optional_u64(block: &Block, provider: &str, attribute: &str) -> Result<Option<u64>> {
    optional_number(block, provider, attribute)?
        .map(|value| {
            if value.is_finite()
                && value >= 0.0
                && value.fract() == 0.0
                && value <= MAX_ACL_EXACT_INTEGER
            {
                Ok(value as u64)
            } else {
                Err(attribute_type_error(
                    provider,
                    attribute,
                    "a non-negative integer",
                ))
            }
        })
        .transpose()
}

pub(super) fn optional_u8(block: &Block, provider: &str, attribute: &str) -> Result<Option<u8>> {
    optional_u64(block, provider, attribute)?
        .map(|value| {
            u8::try_from(value).map_err(|_| {
                attribute_type_error(provider, attribute, "an integer no greater than 255")
            })
        })
        .transpose()
}

pub(super) fn optional_string(
    block: &Block,
    provider: &str,
    attribute: &str,
) -> Result<Option<String>> {
    block
        .attributes
        .get(attribute)
        .map(|value| match value {
            Value::String(value) => Ok(value.trim().to_ascii_lowercase()),
            _ => Err(attribute_type_error(provider, attribute, "a string")),
        })
        .transpose()
}

pub(super) fn optional_non_empty_string(
    block: &Block,
    provider: &str,
    attribute: &str,
) -> Result<Option<String>> {
    let value = block
        .attributes
        .get(attribute)
        .map(|value| match value {
            Value::String(value) => Ok(value.trim().to_string()),
            _ => Err(attribute_type_error(provider, attribute, "a string")),
        })
        .transpose()?;
    if value.as_deref() == Some("") {
        return Err(config_error(
            provider,
            format!("attribute \"{attribute}\" must not be empty"),
        ));
    }
    Ok(value)
}

pub(super) fn optional_url(block: &Block, provider: &str, attribute: &str) -> Result<Option<Url>> {
    optional_non_empty_string(block, provider, attribute)?
        .map(|value| {
            Url::parse(&value).map_err(|_| {
                config_error(
                    provider,
                    format!("attribute \"{attribute}\" must be a valid URL"),
                )
            })
        })
        .transpose()
}

pub(super) fn optional_string_list(
    block: &Block,
    provider: &str,
    attribute: &str,
) -> Result<Option<Vec<String>>> {
    block
        .attributes
        .get(attribute)
        .map(|value| match value {
            Value::List(values) => values
                .iter()
                .map(|value| match value {
                    Value::String(value) if !value.trim().is_empty() => {
                        Ok(value.trim().to_string())
                    }
                    Value::String(_) => Err(config_error(
                        provider,
                        format!("attribute \"{attribute}\" must not contain empty strings"),
                    )),
                    _ => Err(attribute_type_error(
                        provider,
                        attribute,
                        "a list of strings",
                    )),
                })
                .collect(),
            _ => Err(attribute_type_error(
                provider,
                attribute,
                "a list of strings",
            )),
        })
        .transpose()
}

pub(super) fn optional_credential(
    block: &Block,
    provider: &str,
    attribute: &str,
) -> Result<Option<CredentialSource>> {
    let Some(value) = block.attributes.get(attribute) else {
        return Ok(None);
    };
    let credential = match value {
        Value::Null => CredentialSource::none(),
        Value::String(value) if !value.trim().is_empty() => {
            CredentialSource::value(value.trim().to_string())
        }
        Value::String(_) => {
            return Err(config_error(
                provider,
                format!("credential attribute \"{attribute}\" must not be empty"),
            ));
        }
        Value::Call(name, arguments) if name == "env" => match arguments.as_slice() {
            [Value::String(variable)] if valid_environment_variable(variable) => {
                CredentialSource::environment(variable.clone())
            }
            _ => {
                return Err(config_error(
                    provider,
                    format!("credential attribute \"{attribute}\" must use env(\"VARIABLE_NAME\")"),
                ));
            }
        },
        _ => {
            return Err(config_error(
                provider,
                format!(
                    "credential attribute \"{attribute}\" must be a string, null, or env(\"VARIABLE_NAME\")"
                ),
            ));
        }
    };
    Ok(Some(credential))
}

fn valid_environment_variable(variable: &str) -> bool {
    let mut characters = variable.chars();
    matches!(characters.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

pub(super) fn acl_object_to_json_map(
    provider: &str,
    attribute: &str,
    value: &Value,
) -> Result<BTreeMap<String, JsonValue>> {
    let json = acl_value_to_json(provider, attribute, value, 0)?;
    let JsonValue::Object(object) = json else {
        return Err(attribute_type_error(provider, attribute, "an object"));
    };
    Ok(object.into_iter().collect())
}

fn acl_value_to_json(
    provider: &str,
    attribute: &str,
    value: &Value,
    depth: usize,
) -> Result<JsonValue> {
    if depth > MAX_JSON_DEPTH {
        return Err(config_error(
            provider,
            format!("attribute \"{attribute}\" exceeds the maximum nesting depth"),
        ));
    }
    match value {
        Value::String(value) => Ok(JsonValue::String(value.clone())),
        Value::Number(value) => JsonNumber::from_f64(*value)
            .map(JsonValue::Number)
            .ok_or_else(|| {
                config_error(
                    provider,
                    format!("attribute \"{attribute}\" contains a non-finite number"),
                )
            }),
        Value::Bool(value) => Ok(JsonValue::Bool(*value)),
        Value::Null => Ok(JsonValue::Null),
        Value::List(values) => values
            .iter()
            .map(|value| acl_value_to_json(provider, attribute, value, depth + 1))
            .collect::<Result<Vec<_>>>()
            .map(JsonValue::Array),
        Value::Object(values) => {
            let mut object = JsonMap::new();
            for (key, value) in values {
                if key.trim().is_empty() {
                    return Err(config_error(
                        provider,
                        format!("attribute \"{attribute}\" contains an empty object key"),
                    ));
                }
                if object.contains_key(key) {
                    return Err(config_error(
                        provider,
                        format!("attribute \"{attribute}\" contains duplicate key \"{key}\""),
                    ));
                }
                object.insert(
                    key.clone(),
                    acl_value_to_json(provider, attribute, value, depth + 1)?,
                );
            }
            Ok(JsonValue::Object(object))
        }
        Value::Call(_, _) => Err(config_error(
            provider,
            format!("attribute \"{attribute}\" cannot contain function calls"),
        )),
    }
}

fn attribute_type_error(provider: &str, attribute: &str, expected: &str) -> SearchError {
    config_error(
        provider,
        format!("attribute \"{attribute}\" must be {expected}"),
    )
}

pub(super) fn config_error(provider: &str, message: impl AsRef<str>) -> SearchError {
    SearchError::Parse(format!("provider \"{provider}\": {}", message.as_ref()))
}
