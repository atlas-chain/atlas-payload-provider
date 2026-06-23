use std::collections::HashSet;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::validation::{self, ValidatedPayload, ValidationFailure};

const DEFAULT_NAMESPACE: &str = "arkiv.entities";
const JSON_CONTENT_TYPE: &str = "application/json";
const BINARY_CONTENT_TYPE: &str = "application/octet-stream";
const MAX_ATTRIBUTE_NAME_BYTES: usize = 32;
const MAX_ATTRIBUTE_STRING_BYTES: usize = 128;

#[derive(Clone, Debug, Deserialize)]
pub struct ArkivPayloadSubmission {
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(rename = "contentType", default)]
    pub content_type: Option<String>,
    #[serde(rename = "payloadBase64", default)]
    pub payload_base64: Option<String>,
    #[serde(rename = "payloadJson", default)]
    pub payload_json: Option<Value>,
    #[serde(default)]
    pub attributes: Vec<ArkivAttribute>,
    #[serde(rename = "expiresIn", default)]
    pub expires_in: Option<u64>,
    #[serde(rename = "entityKey", default)]
    pub entity_key: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PreparedArkivPayload {
    pub payload: ValidatedPayload,
    pub context: ArkivPayloadContext,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ArkivPayloadContext {
    pub namespace: String,
    #[serde(rename = "contentType", skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(rename = "payloadEncoding")]
    pub payload_encoding: ArkivPayloadEncoding,
    pub attributes: Vec<ArkivAttribute>,
    #[serde(rename = "expiresIn", skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
    #[serde(rename = "entityKey", skip_serializing_if = "Option::is_none")]
    pub entity_key: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ArkivPayloadEncoding {
    Base64,
    CanonicalJson,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArkivAttribute {
    #[serde(alias = "name")]
    pub key: String,
    pub value: ArkivAttributeValue,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ArkivAttributeValue {
    Uint(u64),
    String(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArkivValidationFailure {
    MissingPayload,
    AmbiguousPayload,
    Payload(ValidationFailure),
    InvalidAttributeName { key: String, reason: String },
    DuplicateAttribute { key: String },
    AttributeStringTooLong { key: String, actual: usize },
    InvalidEntityKey { value: String },
    JsonSerialization(String),
}

impl std::fmt::Display for ArkivValidationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPayload => {
                write!(f, "provide either payloadBase64 or payloadJson")
            }
            Self::AmbiguousPayload => {
                write!(f, "provide only one of payloadBase64 or payloadJson")
            }
            Self::Payload(error) => write!(f, "{error}"),
            Self::InvalidAttributeName { key, reason } => {
                write!(f, "invalid ARKIV attribute name {key:?}: {reason}")
            }
            Self::DuplicateAttribute { key } => {
                write!(f, "duplicate ARKIV attribute name {key:?}")
            }
            Self::AttributeStringTooLong { key, actual } => write!(
                f,
                "ARKIV attribute {key:?} string value is {actual} bytes, maximum is {MAX_ATTRIBUTE_STRING_BYTES}"
            ),
            Self::InvalidEntityKey { value } => {
                write!(
                    f,
                    "entityKey must be a 0x-prefixed 32-byte hex string, got {value:?}"
                )
            }
            Self::JsonSerialization(message) => {
                write!(f, "failed to encode payloadJson: {message}")
            }
        }
    }
}

pub fn prepare_submission(
    submission: ArkivPayloadSubmission,
    max_payload_bytes: usize,
) -> Result<PreparedArkivPayload, ArkivValidationFailure> {
    let namespace = submission
        .namespace
        .unwrap_or_else(|| DEFAULT_NAMESPACE.to_string());
    let mut attributes = canonical_attributes(submission.attributes)?;
    let entity_key = match submission.entity_key {
        Some(value) => Some(validate_entity_key(value)?),
        None => None,
    };

    let (payload_base64, content_type, payload_encoding) =
        match (submission.payload_base64, submission.payload_json) {
            (None, None) => return Err(ArkivValidationFailure::MissingPayload),
            (Some(_), Some(_)) => return Err(ArkivValidationFailure::AmbiguousPayload),
            (Some(payload_base64), None) => (
                payload_base64,
                submission
                    .content_type
                    .or_else(|| Some(BINARY_CONTENT_TYPE.to_string())),
                ArkivPayloadEncoding::Base64,
            ),
            (None, Some(payload_json)) => {
                let canonical = canonical_json_to_bytes(payload_json)?;
                (
                    STANDARD.encode(canonical),
                    submission
                        .content_type
                        .or_else(|| Some(JSON_CONTENT_TYPE.to_string())),
                    ArkivPayloadEncoding::CanonicalJson,
                )
            }
        };

    attributes.shrink_to_fit();
    let payload = validation::validate_submission(
        crate::model::PayloadSubmission {
            namespace,
            content_type,
            payload_base64,
        },
        max_payload_bytes,
    )
    .map_err(ArkivValidationFailure::Payload)?;

    Ok(PreparedArkivPayload {
        context: ArkivPayloadContext {
            namespace: payload.namespace.clone(),
            content_type: payload.content_type.clone(),
            payload_encoding,
            attributes,
            expires_in: submission.expires_in,
            entity_key,
        },
        payload,
    })
}

fn canonical_attributes(
    attributes: Vec<ArkivAttribute>,
) -> Result<Vec<ArkivAttribute>, ArkivValidationFailure> {
    let mut output = Vec::with_capacity(attributes.len());
    let mut seen = HashSet::with_capacity(attributes.len());

    for mut attribute in attributes {
        attribute.key = validate_attribute_name(&attribute.key)?;
        if !seen.insert(attribute.key.clone()) {
            return Err(ArkivValidationFailure::DuplicateAttribute { key: attribute.key });
        }
        if let ArkivAttributeValue::String(value) = &attribute.value {
            let actual = value.len();
            if actual > MAX_ATTRIBUTE_STRING_BYTES {
                return Err(ArkivValidationFailure::AttributeStringTooLong {
                    key: attribute.key,
                    actual,
                });
            }
        }
        output.push(attribute);
    }

    output.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(output)
}

fn validate_attribute_name(value: &str) -> Result<String, ArkivValidationFailure> {
    let key = value.trim();
    if key.is_empty() {
        return Err(ArkivValidationFailure::InvalidAttributeName {
            key: value.to_string(),
            reason: "name must not be empty".to_string(),
        });
    }
    if key.len() > MAX_ATTRIBUTE_NAME_BYTES {
        return Err(ArkivValidationFailure::InvalidAttributeName {
            key: key.to_string(),
            reason: format!(
                "name is {} bytes, maximum is {MAX_ATTRIBUTE_NAME_BYTES}",
                key.len()
            ),
        });
    }

    for (index, byte) in key.bytes().enumerate() {
        let valid = if index == 0 {
            byte.is_ascii_lowercase()
        } else {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-' | b'.')
        };

        if !valid {
            return Err(ArkivValidationFailure::InvalidAttributeName {
                key: key.to_string(),
                reason: if index == 0 {
                    "first byte must be a lowercase ASCII letter".to_string()
                } else {
                    format!("invalid byte 0x{byte:02x} at index {index}")
                },
            });
        }
    }

    Ok(key.to_string())
}

fn validate_entity_key(value: String) -> Result<String, ArkivValidationFailure> {
    let trimmed = value.trim();
    let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    else {
        return Err(ArkivValidationFailure::InvalidEntityKey { value });
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ArkivValidationFailure::InvalidEntityKey { value });
    }

    Ok(format!("0x{}", hex.to_ascii_lowercase()))
}

fn canonical_json_to_bytes(value: Value) -> Result<Vec<u8>, ArkivValidationFailure> {
    serde_json::to_vec(&normalize_json(value))
        .map_err(|error| ArkivValidationFailure::JsonSerialization(error.to_string()))
}

fn normalize_json(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(normalize_json).collect()),
        Value::Object(items) => {
            let mut pairs = items.into_iter().collect::<Vec<_>>();
            pairs.sort_by(|left, right| left.0.cmp(&right.0));

            let mut normalized = Map::new();
            for (key, value) in pairs {
                normalized.insert(key, normalize_json(value));
            }
            Value::Object(normalized)
        }
        value => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonicalizes_json_payload_and_sorts_attributes() {
        let prepared = prepare_submission(
            ArkivPayloadSubmission {
                namespace: None,
                content_type: None,
                payload_base64: None,
                payload_json: Some(json!({
                    "entity": {
                        "entityType": "document",
                        "entityId": "doc-123",
                        "entityContent": "Hello from ARKIV"
                    }
                })),
                attributes: vec![
                    ArkivAttribute {
                        key: "version".to_string(),
                        value: ArkivAttributeValue::Uint(1),
                    },
                    ArkivAttribute {
                        key: "id".to_string(),
                        value: ArkivAttributeValue::String("doc-123".to_string()),
                    },
                ],
                expires_in: Some(30),
                entity_key: None,
            },
            1024,
        )
        .unwrap();

        assert_eq!(prepared.payload.namespace, DEFAULT_NAMESPACE);
        assert_eq!(
            prepared.payload.content_type.as_deref(),
            Some(JSON_CONTENT_TYPE)
        );
        assert_eq!(
            prepared.payload.bytes,
            br#"{"entity":{"entityContent":"Hello from ARKIV","entityId":"doc-123","entityType":"document"}}"#
        );
        assert_eq!(
            prepared
                .context
                .attributes
                .iter()
                .map(|attribute| attribute.key.as_str())
                .collect::<Vec<_>>(),
            vec!["id", "version"]
        );
    }

    #[test]
    fn rejects_invalid_attribute_name() {
        let error = prepare_submission(
            ArkivPayloadSubmission {
                namespace: None,
                content_type: None,
                payload_base64: Some("aGVsbG8=".to_string()),
                payload_json: None,
                attributes: vec![ArkivAttribute {
                    key: "entityId".to_string(),
                    value: ArkivAttributeValue::String("doc-123".to_string()),
                }],
                expires_in: None,
                entity_key: None,
            },
            1024,
        )
        .unwrap_err();

        assert!(matches!(
            error,
            ArkivValidationFailure::InvalidAttributeName { .. }
        ));
    }

    #[test]
    fn normalizes_entity_key() {
        let prepared = prepare_submission(
            ArkivPayloadSubmission {
                namespace: None,
                content_type: None,
                payload_base64: Some("aGVsbG8=".to_string()),
                payload_json: None,
                attributes: Vec::new(),
                expires_in: None,
                entity_key: Some(
                    "0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                        .to_string(),
                ),
            },
            1024,
        )
        .unwrap();

        assert_eq!(
            prepared.context.entity_key.as_deref(),
            Some("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
    }
}
