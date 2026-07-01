use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::macros::format_description;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct PayloadSubmission {
    pub namespace: String,
    #[serde(rename = "contentType", default)]
    pub content_type: Option<String>,
    #[serde(rename = "payloadBase64")]
    pub payload_base64: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payment: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PayloadRecord {
    pub id: String,
    pub namespace: String,
    #[serde(
        rename = "contentType",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub content_type: Option<String>,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: usize,
    pub checksum: String,
    #[serde(rename = "submittedAt")]
    pub submitted_at: String,
    #[serde(rename = "payloadBase64")]
    pub payload_base64: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub signature: Option<PayloadSignature>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PayloadMetadata {
    pub id: String,
    pub namespace: String,
    #[serde(
        rename = "contentType",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub content_type: Option<String>,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: usize,
    pub checksum: String,
    #[serde(rename = "submittedAt")]
    pub submitted_at: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub signature: Option<PayloadSignature>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PayloadSummary {
    pub id: String,
    pub namespace: String,
    #[serde(
        rename = "contentType",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub content_type: Option<String>,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: usize,
    pub checksum: String,
    #[serde(rename = "submittedAt")]
    pub submitted_at: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub signature: Option<PayloadSignatureSummary>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PayloadReceipt {
    pub service: String,
    pub action: String,
    #[serde(rename = "payloadId")]
    pub payload_id: String,
    pub namespace: String,
    pub checksum: String,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: usize,
    #[serde(rename = "submittedAt")]
    pub submitted_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payment: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PayloadReceiptContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payment: Option<u64>,
}

impl PayloadReceiptContext {
    pub fn is_empty(&self) -> bool {
        self.nonce.is_none() && self.payment.is_none()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PayloadSignature {
    pub scheme: String,
    pub signer: String,
    #[serde(alias = "claim")]
    pub receipt: PayloadReceipt,
    #[serde(rename = "messageHash")]
    pub message_hash: String,
    pub signature: String,
    pub r: String,
    pub s: String,
    pub v: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PayloadSignatureSummary {
    pub scheme: String,
    pub signer: String,
    #[serde(rename = "messageHash")]
    pub message_hash: String,
    pub signature: String,
}

pub fn canonicalize(record: &PayloadRecord) -> String {
    serde_json::to_string_pretty(record).expect("payload record serializes to JSON")
}

pub fn summarize(metadata: &PayloadMetadata) -> PayloadSummary {
    PayloadSummary {
        id: metadata.id.clone(),
        namespace: metadata.namespace.clone(),
        content_type: metadata.content_type.clone(),
        size_bytes: metadata.size_bytes,
        checksum: metadata.checksum.clone(),
        submitted_at: metadata.submitted_at.clone(),
        signature: metadata.signature.as_ref().map(summarize_signature),
    }
}

pub fn metadata_for(record: &PayloadRecord) -> PayloadMetadata {
    PayloadMetadata {
        id: record.id.clone(),
        namespace: record.namespace.clone(),
        content_type: record.content_type.clone(),
        size_bytes: record.size_bytes,
        checksum: record.checksum.clone(),
        submitted_at: record.submitted_at.clone(),
        signature: record.signature.clone(),
    }
}

pub fn metadata_with_signature(
    metadata: &PayloadMetadata,
    signature: Option<PayloadSignature>,
) -> PayloadMetadata {
    PayloadMetadata {
        signature,
        ..metadata.clone()
    }
}

pub fn receipt_for_record(metadata: &PayloadMetadata) -> PayloadReceipt {
    receipt_for_record_with_context(metadata, &PayloadReceiptContext::default())
}

pub fn receipt_for_record_with_context(
    metadata: &PayloadMetadata,
    context: &PayloadReceiptContext,
) -> PayloadReceipt {
    PayloadReceipt {
        service: "atlas-payload-provider".to_string(),
        action: "payloadReceived".to_string(),
        payload_id: metadata.id.clone(),
        namespace: metadata.namespace.clone(),
        checksum: metadata.checksum.clone(),
        size_bytes: metadata.size_bytes,
        submitted_at: metadata.submitted_at.clone(),
        nonce: context.nonce.clone(),
        payment: context.payment,
    }
}

pub fn legacy_hosting_receipt_for_record(metadata: &PayloadMetadata) -> PayloadReceipt {
    let mut receipt = receipt_for_record(metadata);
    receipt.action = "hostPayload".to_string();
    receipt
}

pub fn canonicalize_receipt(receipt: &PayloadReceipt) -> String {
    serde_json::to_string(receipt).expect("payload receipt serializes to JSON")
}

fn summarize_signature(signature: &PayloadSignature) -> PayloadSignatureSummary {
    PayloadSignatureSummary {
        scheme: signature.scheme.clone(),
        signer: signature.signer.clone(),
        message_hash: signature.message_hash.clone(),
        signature: signature.signature.clone(),
    }
}

pub fn now_iso_second() -> String {
    let format = format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]Z");
    OffsetDateTime::now_utc()
        .format(format)
        .expect("format infallible for fixed description")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> PayloadRecord {
        PayloadRecord {
            id: "abc".to_string(),
            namespace: "atlas.blocks".to_string(),
            content_type: None,
            size_bytes: 5,
            checksum: "sha256:def".to_string(),
            submitted_at: "2026-01-01T00:00:00Z".to_string(),
            payload_base64: "aGVsbG8=".to_string(),
            signature: None,
        }
    }

    #[test]
    fn canonicalize_omits_absent_content_type() {
        let serialized = canonicalize(&record());
        assert!(serialized.contains("\"namespace\""));
        assert!(serialized.contains("\"payloadBase64\""));
        assert!(!serialized.contains("contentType"));
    }

    #[test]
    fn summary_omits_payload_body() {
        let summary = summarize(&metadata_for(&record()));
        let serialized = serde_json::to_string(&summary).unwrap();
        assert!(serialized.contains("\"sizeBytes\""));
        assert!(!serialized.contains("payloadBase64"));
    }

    #[test]
    fn metadata_omits_payload_body() {
        let metadata = metadata_for(&record());
        let serialized = serde_json::to_string(&metadata).unwrap();
        assert!(serialized.contains("\"sizeBytes\""));
        assert!(!serialized.contains("payloadBase64"));
    }

    #[test]
    fn receipt_uses_record_metadata() {
        let receipt = receipt_for_record(&metadata_for(&record()));

        assert_eq!(receipt.service, "atlas-payload-provider");
        assert_eq!(receipt.action, "payloadReceived");
        assert_eq!(receipt.payload_id, "abc");
        assert_eq!(receipt.size_bytes, 5);
        assert_eq!(receipt.nonce, None);
        assert_eq!(receipt.payment, None);
    }

    #[test]
    fn receipt_can_include_nonce_and_payment_context() {
        let nonce = format!("0x{}", "01".repeat(32));
        let receipt = receipt_for_record_with_context(
            &metadata_for(&record()),
            &PayloadReceiptContext {
                nonce: Some(nonce.clone()),
                payment: Some(100_000),
            },
        );

        assert_eq!(receipt.nonce.as_deref(), Some(nonce.as_str()));
        assert_eq!(receipt.payment, Some(100_000));
    }
}
