use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sha2::{Digest, Sha256};

use crate::model::{
    PayloadReceiptContext, PayloadRecord, PayloadSignature, PayloadSummary, canonicalize,
    now_iso_second, receipt_for_record_with_context, summarize,
};
use crate::signer::{
    EthereumSigner, signature_is_current_payload_receipt, validate_payload_signature,
};
use crate::validation::{self, ValidatedPayload};

#[derive(Clone, Debug)]
pub struct StoreSnapshot {
    pub payload_count: usize,
    pub total_bytes: usize,
    pub max_payload_bytes: usize,
    pub latest: Vec<PayloadSummary>,
}

#[derive(Clone, Debug)]
pub struct SubmitOutcome {
    pub record: PayloadRecord,
    pub created: bool,
    pub signature: Option<PayloadSignature>,
}

#[derive(Debug)]
pub enum StoreFailure {
    Persistence(String),
    Signing(String),
}

#[derive(Debug)]
struct Inner {
    payloads: HashMap<String, PayloadRecord>,
    newest_first: VecDeque<String>,
    total_bytes: usize,
}

#[derive(Debug)]
pub struct PayloadStore {
    inner: Mutex<Inner>,
    payload_dir: PathBuf,
    max_payload_bytes: usize,
    signer: Option<EthereumSigner>,
}

impl PayloadStore {
    pub fn load(
        payload_dir: PathBuf,
        max_payload_bytes: usize,
        signer: Option<EthereumSigner>,
    ) -> Result<Self, String> {
        std::fs::create_dir_all(&payload_dir).map_err(|error| {
            format!(
                "failed to create payload directory {}: {error}",
                payload_dir.display()
            )
        })?;

        let mut records = Vec::new();
        for entry in std::fs::read_dir(&payload_dir).map_err(|error| {
            format!(
                "failed to read payload directory {}: {error}",
                payload_dir.display()
            )
        })? {
            let entry =
                entry.map_err(|error| format!("failed to read payload dir entry: {error}"))?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }

            let raw = std::fs::read_to_string(&path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
            let record: PayloadRecord = serde_json::from_str(&raw)
                .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
            validate_stored_record(&record, max_payload_bytes)
                .map_err(|error| format!("invalid stored payload {}: {error}", path.display()))?;
            records.push(record);
        }

        records.sort_by(|left, right| right.submitted_at.cmp(&left.submitted_at));

        let mut payloads = HashMap::new();
        let mut newest_first = VecDeque::new();
        let mut total_bytes = 0usize;
        for record in records {
            if payloads.contains_key(&record.id) {
                return Err(format!("duplicate stored payload id {}", record.id));
            }
            total_bytes = total_bytes
                .checked_add(record.size_bytes)
                .ok_or_else(|| "stored payload byte count overflowed".to_string())?;
            newest_first.push_back(record.id.clone());
            payloads.insert(record.id.clone(), record);
        }

        Ok(Self {
            inner: Mutex::new(Inner {
                payloads,
                newest_first,
                total_bytes,
            }),
            payload_dir,
            max_payload_bytes,
            signer,
        })
    }

    pub fn submit(&self, payload: ValidatedPayload) -> Result<SubmitOutcome, StoreFailure> {
        let receipt_context = payload.receipt_context.clone();
        let mut record = record_for(payload);
        self.sign_record_if_enabled(&mut record)?;
        let mut inner = self.inner.lock().expect("payload store lock poisoned");

        if let Some(existing) = inner.payloads.get(&record.id).cloned() {
            let mut existing = existing;
            let needs_current_receipt = existing
                .signature
                .as_ref()
                .map(|signature| !signature_is_current_payload_receipt(&existing, signature))
                .unwrap_or(true);
            if needs_current_receipt {
                existing.signature = None;
                self.sign_record_if_enabled(&mut existing)?;
                if existing.signature.is_some() {
                    persist_record(&self.payload_dir, &existing)?;
                    inner.payloads.insert(existing.id.clone(), existing.clone());
                }
            }

            return Ok(SubmitOutcome {
                signature: self.signature_for_context(&existing, &receipt_context)?,
                record: existing,
                created: false,
            });
        }

        persist_record(&self.payload_dir, &record)?;

        inner.total_bytes = inner
            .total_bytes
            .checked_add(record.size_bytes)
            .expect("payload byte count overflowed");
        inner.newest_first.push_front(record.id.clone());
        inner.payloads.insert(record.id.clone(), record.clone());

        Ok(SubmitOutcome {
            signature: self.signature_for_context(&record, &receipt_context)?,
            record,
            created: true,
        })
    }

    pub fn get(&self, id: &str) -> Option<PayloadRecord> {
        let inner = self.inner.lock().expect("payload store lock poisoned");
        inner.payloads.get(id).cloned()
    }

    pub fn snapshot(&self, limit: usize) -> StoreSnapshot {
        let inner = self.inner.lock().expect("payload store lock poisoned");
        let latest = inner
            .newest_first
            .iter()
            .take(limit)
            .filter_map(|id| inner.payloads.get(id))
            .map(summarize)
            .collect();

        StoreSnapshot {
            payload_count: inner.payloads.len(),
            total_bytes: inner.total_bytes,
            max_payload_bytes: self.max_payload_bytes,
            latest,
        }
    }

    pub fn max_payload_bytes(&self) -> usize {
        self.max_payload_bytes
    }

    pub fn signer_address(&self) -> Option<&str> {
        self.signer.as_ref().map(EthereumSigner::address)
    }

    fn sign_record_if_enabled(&self, record: &mut PayloadRecord) -> Result<(), StoreFailure> {
        if record
            .signature
            .as_ref()
            .is_some_and(|signature| signature_is_current_payload_receipt(record, signature))
        {
            return Ok(());
        }
        record.signature = None;

        let Some(signer) = self.signer.as_ref() else {
            return Ok(());
        };

        record.signature = Some(signer.sign_record(record).map_err(StoreFailure::Signing)?);
        Ok(())
    }

    fn signature_for_context(
        &self,
        record: &PayloadRecord,
        context: &PayloadReceiptContext,
    ) -> Result<Option<PayloadSignature>, StoreFailure> {
        if context.is_empty() {
            return Ok(record.signature.clone());
        }

        let Some(signer) = self.signer.as_ref() else {
            return Ok(None);
        };

        signer
            .sign_receipt(receipt_for_record_with_context(record, context))
            .map(Some)
            .map_err(StoreFailure::Signing)
    }
}

fn record_for(payload: ValidatedPayload) -> PayloadRecord {
    let id = payload_id(&payload.namespace, &payload.bytes);
    let checksum = checksum_for(&payload.bytes);
    let size_bytes = payload.bytes.len();
    let payload_base64 = STANDARD.encode(&payload.bytes);

    PayloadRecord {
        id,
        namespace: payload.namespace,
        content_type: payload.content_type,
        size_bytes,
        checksum,
        submitted_at: now_iso_second(),
        payload_base64,
        signature: None,
    }
}

fn validate_stored_record(record: &PayloadRecord, max_payload_bytes: usize) -> Result<(), String> {
    if !is_payload_id(&record.id) {
        return Err(format!(
            "id is not a 64 character hex digest: {}",
            record.id
        ));
    }

    let bytes = validation::validate_record_shape(record, max_payload_bytes)
        .map_err(|error| error.to_string())?;

    let expected_checksum = checksum_for(&bytes);
    if record.checksum != expected_checksum {
        return Err(format!(
            "checksum mismatch: expected {expected_checksum}, got {}",
            record.checksum
        ));
    }

    let expected_id = payload_id(&record.namespace, &bytes);
    if record.id != expected_id {
        return Err(format!(
            "id mismatch: expected {expected_id}, got {}",
            record.id
        ));
    }

    if let Some(signature) = record.signature.as_ref() {
        validate_payload_signature(record, signature)?;
    }

    Ok(())
}

fn persist_record(payload_dir: &Path, record: &PayloadRecord) -> Result<(), StoreFailure> {
    let final_path = payload_dir.join(format!("{}.json", record.id));
    let temp_path = payload_dir.join(format!(".{}.tmp", record.id));
    let canonical = canonicalize(record);

    std::fs::write(&temp_path, canonical).map_err(|error| {
        StoreFailure::Persistence(format!("failed to write {}: {error}", temp_path.display()))
    })?;

    std::fs::rename(&temp_path, &final_path).map_err(|error| {
        let _ = std::fs::remove_file(&temp_path);
        StoreFailure::Persistence(format!(
            "failed to publish {}: {error}",
            final_path.display()
        ))
    })
}

fn payload_id(namespace: &str, bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(namespace.as_bytes());
    hasher.update([0]);
    hasher.update(bytes);
    hex_lower(&hasher.finalize())
}

fn checksum_for(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{}", hex_lower(&hasher.finalize()))
}

fn is_payload_id(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_payload_dir(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "atlas-payload-store-{name}-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        path
    }

    fn payload(bytes: &[u8]) -> ValidatedPayload {
        ValidatedPayload {
            namespace: "atlas.blocks".to_string(),
            content_type: Some("application/octet-stream".to_string()),
            bytes: bytes.to_vec(),
            receipt_context: PayloadReceiptContext::default(),
        }
    }

    #[test]
    fn submit_persists_payload() {
        let dir = temp_payload_dir("submit");
        let store = PayloadStore::load(dir.clone(), 1024, None).unwrap();

        let outcome = store.submit(payload(b"hello")).unwrap();

        assert!(outcome.created);
        assert_eq!(outcome.record.size_bytes, 5);
        assert!(dir.join(format!("{}.json", outcome.record.id)).exists());
        assert_eq!(store.snapshot(10).payload_count, 1);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn duplicate_submit_is_idempotent() {
        let dir = temp_payload_dir("duplicate");
        let store = PayloadStore::load(dir.clone(), 1024, None).unwrap();

        let first = store.submit(payload(b"hello")).unwrap();
        let second = store.submit(payload(b"hello")).unwrap();

        assert!(first.created);
        assert!(!second.created);
        assert_eq!(first.record.id, second.record.id);
        assert_eq!(store.snapshot(10).payload_count, 1);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn load_reads_persisted_payloads() {
        let dir = temp_payload_dir("load");
        let store = PayloadStore::load(dir.clone(), 1024, None).unwrap();
        let outcome = store.submit(payload(b"hello")).unwrap();
        drop(store);

        let loaded = PayloadStore::load(dir.clone(), 1024, None).unwrap();
        let record = loaded.get(&outcome.record.id).unwrap();

        assert_eq!(record.payload_base64, "aGVsbG8=");
        assert_eq!(loaded.snapshot(10).payload_count, 1);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn checksum_uses_payload_bytes_only() {
        assert_eq!(
            checksum_for(b"hello"),
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn submit_signs_payload_when_signer_is_configured() {
        let dir = temp_payload_dir("signed");
        let signer = EthereumSigner::from_private_key_hex(
            "0x4f3edf983ac636a65a842ce7c78d9aa706d3b113bce9c46f30d7d395b9c4b9b5",
        )
        .unwrap();
        let store = PayloadStore::load(dir.clone(), 1024, Some(signer)).unwrap();

        let outcome = store.submit(payload(b"hello")).unwrap();

        let signature = outcome.record.signature.as_ref().unwrap();
        assert_eq!(
            signature.signer,
            "0xa23fe79dc6d9ecc325d187a858f298e953add9b8"
        );
        assert!(validate_payload_signature(&outcome.record, signature).is_ok());

        let loaded = PayloadStore::load(dir.clone(), 1024, None).unwrap();
        let record = loaded.get(&outcome.record.id).unwrap();
        assert!(record.signature.is_some());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn submit_returns_contextual_signature_without_changing_payload_id() {
        let dir = temp_payload_dir("contextual-signature");
        let signer = EthereumSigner::from_private_key_hex(
            "0x0000000000000000000000000000000000000000000000000000000000000001",
        )
        .unwrap();
        let store = PayloadStore::load(dir.clone(), 1024, Some(signer)).unwrap();
        let nonce = format!("0x{}", "ab".repeat(32));
        let mut contextual_payload = payload(b"hello");
        contextual_payload.receipt_context = PayloadReceiptContext {
            nonce: Some(nonce.clone()),
            payment: Some(100_000),
        };

        let first = store.submit(payload(b"hello")).unwrap();
        let second = store.submit(contextual_payload).unwrap();

        assert_eq!(first.record.id, second.record.id);
        let signature = second.signature.as_ref().unwrap();
        assert_eq!(signature.receipt.nonce.as_deref(), Some(nonce.as_str()));
        assert_eq!(signature.receipt.payment, Some(100_000));
        assert_eq!(
            second.record.signature.as_ref().unwrap().receipt.nonce,
            None
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn duplicate_submit_signs_existing_unsigned_payload_when_signer_is_configured() {
        let dir = temp_payload_dir("upgrade");
        let unsigned_store = PayloadStore::load(dir.clone(), 1024, None).unwrap();
        let unsigned = unsigned_store.submit(payload(b"hello")).unwrap();
        assert!(unsigned.record.signature.is_none());
        drop(unsigned_store);

        let signer = EthereumSigner::from_private_key_hex(
            "0x4f3edf983ac636a65a842ce7c78d9aa706d3b113bce9c46f30d7d395b9c4b9b5",
        )
        .unwrap();
        let signed_store = PayloadStore::load(dir.clone(), 1024, Some(signer)).unwrap();
        let upgraded = signed_store.submit(payload(b"hello")).unwrap();

        assert!(!upgraded.created);
        assert!(upgraded.record.signature.is_some());

        let loaded = PayloadStore::load(dir.clone(), 1024, None).unwrap();
        let record = loaded.get(&upgraded.record.id).unwrap();
        assert!(record.signature.is_some());

        let _ = std::fs::remove_dir_all(dir);
    }
}
