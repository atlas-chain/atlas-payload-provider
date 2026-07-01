use secp256k1::ecdsa::{RecoverableSignature, RecoveryId};
use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};
use sha3::{Digest, Keccak256};

use crate::model::{
    PayloadReceipt, PayloadSignature, canonicalize_receipt, legacy_hosting_receipt_for_record,
    receipt_for_record,
};

#[derive(Clone)]
pub struct EthereumSigner {
    secret_key: SecretKey,
    address: String,
}

impl std::fmt::Debug for EthereumSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EthereumSigner")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

impl EthereumSigner {
    pub fn from_private_key_hex(value: &str) -> Result<Self, String> {
        let bytes = decode_fixed_hex(value, 32)?;
        let array: [u8; 32] = bytes
            .try_into()
            .expect("decode_fixed_hex returned exactly 32 bytes");
        let secret_key = SecretKey::from_byte_array(array)
            .map_err(|error| format!("not a valid secp256k1 private key: {error}"))?;
        let secp = Secp256k1::new();
        let public_key = PublicKey::from_secret_key(&secp, &secret_key);
        let address = address_for_public_key(&public_key);

        Ok(Self {
            secret_key,
            address,
        })
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    pub fn sign_record(
        &self,
        metadata: &crate::model::PayloadMetadata,
    ) -> Result<PayloadSignature, String> {
        self.sign_receipt(receipt_for_record(metadata))
    }

    pub fn sign_receipt(&self, receipt: PayloadReceipt) -> Result<PayloadSignature, String> {
        let message = canonicalize_receipt(&receipt);
        let hash = eip191_hash(message.as_bytes());
        let secp = Secp256k1::new();
        let signature = secp.sign_ecdsa_recoverable(Message::from_digest(hash), &self.secret_key);
        let (recovery_id, compact) = signature.serialize_compact();
        let recovery_byte: i32 = recovery_id.into();
        let v = u8::try_from(recovery_byte).expect("recovery id fits in u8") + 27;
        let r = prefixed_hex(&compact[..32]);
        let s = prefixed_hex(&compact[32..]);
        let mut combined = Vec::with_capacity(65);
        combined.extend_from_slice(&compact);
        combined.push(v);

        Ok(PayloadSignature {
            scheme: "eip191".to_string(),
            signer: self.address.clone(),
            receipt,
            message_hash: prefixed_hex(&hash),
            signature: prefixed_hex(&combined),
            r,
            s,
            v,
        })
    }
}

pub fn validate_payload_signature(
    metadata: &crate::model::PayloadMetadata,
    signature: &PayloadSignature,
) -> Result<(), String> {
    if signature.scheme != "eip191" {
        return Err(format!("unsupported signature scheme {}", signature.scheme));
    }

    let expected_receipt = receipt_for_record(metadata);
    let legacy_receipt = legacy_hosting_receipt_for_record(metadata);
    if signature.receipt != expected_receipt && signature.receipt != legacy_receipt {
        return Err("signature receipt does not match payload record".to_string());
    }

    let message = canonicalize_receipt(&signature.receipt);
    let hash = eip191_hash(message.as_bytes());
    if signature.message_hash != prefixed_hex(&hash) {
        return Err("signature messageHash does not match receipt".to_string());
    }

    if !(signature.v == 27 || signature.v == 28) {
        return Err(format!("signature v must be 27 or 28, got {}", signature.v));
    }

    let r = decode_prefixed_hex(&signature.r, 32)?;
    let s = decode_prefixed_hex(&signature.s, 32)?;
    let full = decode_prefixed_hex(&signature.signature, 65)?;
    if full[..32] != r || full[32..64] != s || full[64] != signature.v {
        return Err("signature, r, s, and v fields are inconsistent".to_string());
    }

    let mut compact = [0u8; 64];
    compact[..32].copy_from_slice(&r);
    compact[32..].copy_from_slice(&s);
    let recovery_id = RecoveryId::try_from(i32::from(signature.v - 27))
        .map_err(|error| format!("invalid recovery id: {error}"))?;
    let recoverable = RecoverableSignature::from_compact(&compact, recovery_id)
        .map_err(|error| format!("invalid recoverable signature: {error}"))?;
    let secp = Secp256k1::new();
    let public_key = secp
        .recover_ecdsa(Message::from_digest(hash), &recoverable)
        .map_err(|error| format!("signature recovery failed: {error}"))?;
    let recovered_address = address_for_public_key(&public_key);
    if !signature.signer.eq_ignore_ascii_case(&recovered_address) {
        return Err(format!(
            "signature signer {} does not match recovered address {recovered_address}",
            signature.signer
        ));
    }

    Ok(())
}

pub fn signature_is_current_payload_receipt(
    metadata: &crate::model::PayloadMetadata,
    signature: &PayloadSignature,
) -> bool {
    signature.receipt == receipt_for_record(metadata)
        && validate_payload_signature(metadata, signature).is_ok()
}

fn eip191_hash(message: &[u8]) -> [u8; 32] {
    let prefix = format!("\x19Ethereum Signed Message:\n{}", message.len());
    let mut hasher = Keccak256::new();
    hasher.update(prefix.as_bytes());
    hasher.update(message);
    let digest = hasher.finalize();
    let mut output = [0u8; 32];
    output.copy_from_slice(&digest);
    output
}

fn address_for_public_key(public_key: &PublicKey) -> String {
    let serialized = public_key.serialize_uncompressed();
    let mut hasher = Keccak256::new();
    hasher.update(&serialized[1..]);
    let digest = hasher.finalize();
    prefixed_hex(&digest[12..])
}

fn decode_fixed_hex(value: &str, expected_bytes: usize) -> Result<Vec<u8>, String> {
    let trimmed = value.trim();
    let hex = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    if hex.len() != expected_bytes * 2 {
        return Err(format!(
            "expected {} hex characters, got {}",
            expected_bytes * 2,
            hex.len()
        ));
    }
    decode_hex_body(hex)
}

fn decode_prefixed_hex(value: &str, expected_bytes: usize) -> Result<Vec<u8>, String> {
    let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    else {
        return Err("expected 0x-prefixed hex string".to_string());
    };
    if hex.len() != expected_bytes * 2 {
        return Err(format!(
            "expected {} hex bytes, got {}",
            expected_bytes,
            hex.len() / 2
        ));
    }
    decode_hex_body(hex)
}

fn decode_hex_body(hex: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let chars = hex.as_bytes();
    for index in (0..chars.len()).step_by(2) {
        let chunk = std::str::from_utf8(&chars[index..index + 2])
            .map_err(|error| format!("invalid utf8 in hex string: {error}"))?;
        let byte = u8::from_str_radix(chunk, 16)
            .map_err(|error| format!("invalid hex byte at index {index}: {error}"))?;
        bytes.push(byte);
    }
    Ok(bytes)
}

fn prefixed_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(2 + bytes.len() * 2);
    output.push_str("0x");
    output.push_str(&hex_lower(bytes));
    output
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
    use crate::model::PayloadMetadata;

    const DEV_PRIVATE_KEY: &str =
        "0x4f3edf983ac636a65a842ce7c78d9aa706d3b113bce9c46f30d7d395b9c4b9b5";

    fn record() -> PayloadMetadata {
        PayloadMetadata {
            id: "69811a86589a02a9b82695c741aeae410985a5d35b8e3906a445633fa52075f9".to_string(),
            namespace: "atlas.blocks".to_string(),
            content_type: Some("text/plain".to_string()),
            size_bytes: 5,
            checksum: "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                .to_string(),
            submitted_at: "2026-06-22T13:06:23Z".to_string(),
            signature: None,
        }
    }

    #[test]
    fn derives_ethereum_address_from_private_key() {
        let signer = EthereumSigner::from_private_key_hex(DEV_PRIVATE_KEY).unwrap();

        assert_eq!(
            signer.address(),
            "0xa23fe79dc6d9ecc325d187a858f298e953add9b8"
        );
    }

    #[test]
    fn signs_and_recovers_payload_receipt() {
        let signer = EthereumSigner::from_private_key_hex(DEV_PRIVATE_KEY).unwrap();
        let mut record = record();
        let signature = signer.sign_record(&record).unwrap();

        assert_eq!(signature.scheme, "eip191");
        assert_eq!(signature.signer, signer.address());
        assert_eq!(signature.receipt.action, "payloadReceived");
        assert!(signature.v == 27 || signature.v == 28);
        record.signature = Some(signature.clone());
        assert!(validate_payload_signature(&record, &signature).is_ok());
        assert!(signature_is_current_payload_receipt(&record, &signature));
    }

    #[test]
    fn rejects_tampered_receipt() {
        let signer = EthereumSigner::from_private_key_hex(DEV_PRIVATE_KEY).unwrap();
        let mut record = record();
        let mut signature = signer.sign_record(&record).unwrap();
        signature.receipt.size_bytes = 6;
        record.signature = Some(signature.clone());

        assert!(validate_payload_signature(&record, &signature).is_err());
    }

    #[test]
    fn validates_legacy_hosting_claim_for_existing_records() {
        let signer = EthereumSigner::from_private_key_hex(DEV_PRIVATE_KEY).unwrap();
        let record = record();
        let signature = signer
            .sign_receipt(legacy_hosting_receipt_for_record(&record))
            .unwrap();

        assert_eq!(signature.receipt.action, "hostPayload");
        assert!(validate_payload_signature(&record, &signature).is_ok());
        assert!(!signature_is_current_payload_receipt(&record, &signature));
    }
}
