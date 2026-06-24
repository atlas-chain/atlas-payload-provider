use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::model::{PayloadReceiptContext, PayloadRecord, PayloadSubmission};

const MAX_NAMESPACE_BYTES: usize = 64;
const MAX_CONTENT_TYPE_BYTES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedPayload {
    pub namespace: String,
    pub content_type: Option<String>,
    pub bytes: Vec<u8>,
    pub receipt_context: PayloadReceiptContext,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationFailure {
    EmptyNamespace,
    NamespaceTooLong { max: usize, actual: usize },
    InvalidNamespaceCharacter { index: usize, byte: u8 },
    ContentTypeTooLong { max: usize, actual: usize },
    InvalidContentTypeCharacter { index: usize, byte: u8 },
    InvalidNonce { value: String },
    InvalidPayment { value: u64 },
    InvalidBase64 { message: String },
    EmptyPayload,
    PayloadTooLarge { max: usize, actual: usize },
    SizeMismatch { expected: usize, actual: usize },
}

impl std::fmt::Display for ValidationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyNamespace => write!(f, "namespace must not be empty"),
            Self::NamespaceTooLong { max, actual } => {
                write!(f, "namespace is {actual} bytes, maximum is {max}")
            }
            Self::InvalidNamespaceCharacter { index, byte } => write!(
                f,
                "namespace contains invalid byte 0x{byte:02x} at index {index}; use ASCII letters, digits, dot, dash, or underscore"
            ),
            Self::ContentTypeTooLong { max, actual } => {
                write!(f, "contentType is {actual} bytes, maximum is {max}")
            }
            Self::InvalidContentTypeCharacter { index, byte } => write!(
                f,
                "contentType contains invalid byte 0x{byte:02x} at index {index}"
            ),
            Self::InvalidNonce { value } => write!(
                f,
                "nonce must be a non-zero 0x-prefixed 32-byte hex string, got {value:?}"
            ),
            Self::InvalidPayment { value } => {
                write!(f, "payment must be greater than zero, got {value}")
            }
            Self::InvalidBase64 { message } => write!(f, "payloadBase64 is invalid: {message}"),
            Self::EmptyPayload => write!(f, "payload must not be empty"),
            Self::PayloadTooLarge { max, actual } => {
                write!(f, "payload is {actual} bytes, maximum is {max}")
            }
            Self::SizeMismatch { expected, actual } => write!(
                f,
                "stored payload size mismatch: record says {expected} bytes, decoded payload is {actual} bytes"
            ),
        }
    }
}

pub fn validate_submission(
    submission: PayloadSubmission,
    max_payload_bytes: usize,
) -> Result<ValidatedPayload, ValidationFailure> {
    let namespace = validate_namespace(&submission.namespace)?;
    let content_type = validate_content_type(submission.content_type.as_deref())?;
    let nonce = validate_nonce(submission.nonce)?;
    let payment = validate_payment(submission.payment)?;
    let bytes = decode_payload_base64(&submission.payload_base64, max_payload_bytes)?;

    Ok(ValidatedPayload {
        namespace,
        content_type,
        bytes,
        receipt_context: PayloadReceiptContext { nonce, payment },
    })
}

pub fn validate_record_shape(
    record: &PayloadRecord,
    max_payload_bytes: usize,
) -> Result<Vec<u8>, ValidationFailure> {
    validate_namespace(&record.namespace)?;
    validate_content_type(record.content_type.as_deref())?;
    let bytes = decode_payload_base64(&record.payload_base64, max_payload_bytes)?;

    if record.size_bytes != bytes.len() {
        return Err(ValidationFailure::SizeMismatch {
            expected: record.size_bytes,
            actual: bytes.len(),
        });
    }

    Ok(bytes)
}

fn validate_namespace(value: &str) -> Result<String, ValidationFailure> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ValidationFailure::EmptyNamespace);
    }
    if trimmed.len() > MAX_NAMESPACE_BYTES {
        return Err(ValidationFailure::NamespaceTooLong {
            max: MAX_NAMESPACE_BYTES,
            actual: trimmed.len(),
        });
    }

    for (index, byte) in trimmed.bytes().enumerate() {
        if !matches!(
            byte,
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-' | b'_'
        ) {
            return Err(ValidationFailure::InvalidNamespaceCharacter { index, byte });
        }
    }

    Ok(trimmed.to_string())
}

fn validate_content_type(value: Option<&str>) -> Result<Option<String>, ValidationFailure> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.len() > MAX_CONTENT_TYPE_BYTES {
        return Err(ValidationFailure::ContentTypeTooLong {
            max: MAX_CONTENT_TYPE_BYTES,
            actual: trimmed.len(),
        });
    }

    for (index, byte) in trimmed.bytes().enumerate() {
        if !(0x20..=0x7e).contains(&byte) {
            return Err(ValidationFailure::InvalidContentTypeCharacter { index, byte });
        }
    }

    Ok(Some(trimmed.to_string()))
}

fn validate_nonce(value: Option<String>) -> Result<Option<String>, ValidationFailure> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim().to_lowercase();
    let Some(hex) = trimmed.strip_prefix("0x") else {
        return Err(ValidationFailure::InvalidNonce { value });
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ValidationFailure::InvalidNonce { value });
    }
    if hex.bytes().all(|byte| byte == b'0') {
        return Err(ValidationFailure::InvalidNonce { value });
    }
    Ok(Some(trimmed))
}

fn validate_payment(value: Option<u64>) -> Result<Option<u64>, ValidationFailure> {
    if matches!(value, Some(0)) {
        return Err(ValidationFailure::InvalidPayment { value: 0 });
    }
    Ok(value)
}

fn decode_payload_base64(
    value: &str,
    max_payload_bytes: usize,
) -> Result<Vec<u8>, ValidationFailure> {
    let bytes = STANDARD.decode(value.trim().as_bytes()).map_err(|error| {
        ValidationFailure::InvalidBase64 {
            message: error.to_string(),
        }
    })?;

    if bytes.is_empty() {
        return Err(ValidationFailure::EmptyPayload);
    }
    if bytes.len() > max_payload_bytes {
        return Err(ValidationFailure::PayloadTooLarge {
            max: max_payload_bytes,
            actual: bytes.len(),
        });
    }

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn submission(payload_base64: &str) -> PayloadSubmission {
        PayloadSubmission {
            namespace: "atlas.blocks".to_string(),
            content_type: Some(" application/octet-stream ".to_string()),
            payload_base64: payload_base64.to_string(),
            nonce: None,
            payment: None,
        }
    }

    #[test]
    fn valid_submission_passes() {
        let payload = validate_submission(submission("aGVsbG8="), 1024).unwrap();

        assert_eq!(payload.namespace, "atlas.blocks");
        assert_eq!(
            payload.content_type.as_deref(),
            Some("application/octet-stream")
        );
        assert_eq!(payload.bytes, b"hello");
        assert_eq!(payload.receipt_context.nonce, None);
        assert_eq!(payload.receipt_context.payment, None);
    }

    #[test]
    fn rejects_empty_namespace() {
        let mut item = submission("aGVsbG8=");
        item.namespace = " ".to_string();

        assert_eq!(
            validate_submission(item, 1024),
            Err(ValidationFailure::EmptyNamespace)
        );
    }

    #[test]
    fn rejects_invalid_namespace_character() {
        let mut item = submission("aGVsbG8=");
        item.namespace = "atlas/blocks".to_string();

        assert_eq!(
            validate_submission(item, 1024),
            Err(ValidationFailure::InvalidNamespaceCharacter {
                index: 5,
                byte: b'/'
            })
        );
    }

    #[test]
    fn rejects_invalid_base64() {
        assert!(matches!(
            validate_submission(submission("not base64"), 1024),
            Err(ValidationFailure::InvalidBase64 { .. })
        ));
    }

    #[test]
    fn rejects_empty_payload() {
        assert_eq!(
            validate_submission(submission(""), 1024),
            Err(ValidationFailure::EmptyPayload)
        );
    }

    #[test]
    fn validates_receipt_context() {
        let expected_nonce = format!("0x{}", "ab".repeat(32));
        let mut item = submission("aGVsbG8=");
        item.nonce = Some(expected_nonce.clone());
        item.payment = Some(100_000);

        let payload = validate_submission(item, 1024).unwrap();

        assert_eq!(
            payload.receipt_context.nonce.as_deref(),
            Some(expected_nonce.as_str())
        );
        assert_eq!(payload.receipt_context.payment, Some(100_000));
    }

    #[test]
    fn rejects_zero_receipt_context_nonce() {
        let mut item = submission("aGVsbG8=");
        item.nonce = Some(format!("0x{}", "00".repeat(32)));

        assert!(matches!(
            validate_submission(item, 1024),
            Err(ValidationFailure::InvalidNonce { .. })
        ));
    }

    #[test]
    fn rejects_zero_payment() {
        let mut item = submission("aGVsbG8=");
        item.payment = Some(0);

        assert_eq!(
            validate_submission(item, 1024),
            Err(ValidationFailure::InvalidPayment { value: 0 })
        );
    }

    #[test]
    fn rejects_payload_above_limit() {
        assert_eq!(
            validate_submission(submission("aGVsbG8="), 4),
            Err(ValidationFailure::PayloadTooLarge { max: 4, actual: 5 })
        );
    }

    #[test]
    fn validates_stored_shape() {
        let record = PayloadRecord {
            id: "unused".to_string(),
            namespace: "atlas.blocks".to_string(),
            content_type: None,
            size_bytes: 5,
            checksum: "unused".to_string(),
            submitted_at: "2026-01-01T00:00:00Z".to_string(),
            payload_base64: "aGVsbG8=".to_string(),
            signature: None,
        };

        assert_eq!(validate_record_shape(&record, 1024).unwrap(), b"hello");
    }

    #[test]
    fn rejects_stored_size_mismatch() {
        let record = PayloadRecord {
            id: "unused".to_string(),
            namespace: "atlas.blocks".to_string(),
            content_type: None,
            size_bytes: 4,
            checksum: "unused".to_string(),
            submitted_at: "2026-01-01T00:00:00Z".to_string(),
            payload_base64: "aGVsbG8=".to_string(),
            signature: None,
        };

        assert_eq!(
            validate_record_shape(&record, 1024),
            Err(ValidationFailure::SizeMismatch {
                expected: 4,
                actual: 5
            })
        );
    }
}
