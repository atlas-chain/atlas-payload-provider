import { sha256 } from "@noble/hashes/sha2.js";
import { keccak_256 } from "@noble/hashes/sha3.js";
import type { PayloadReceipt } from "./types.ts";

/** Lowercase hex, no `0x` prefix. */
export function toHex(bytes: Uint8Array): string {
  let out = "";
  for (const b of bytes) out += b.toString(16).padStart(2, "0");
  return out;
}

/** Lowercase hex with `0x` prefix. */
export function toHexPrefixed(bytes: Uint8Array): string {
  return "0x" + toHex(bytes);
}

function utf8(input: string): Uint8Array {
  return new TextEncoder().encode(input);
}

/**
 * Payload content address, matching `store::payload_id` in the Rust server:
 * sha256(namespace_utf8 || 0x00 || payload_bytes), lowercase hex, no `0x` prefix.
 */
export function payloadId(namespace: string, payload: Uint8Array): string {
  const ns = utf8(namespace);
  const buf = new Uint8Array(ns.length + 1 + payload.length);
  buf.set(ns, 0);
  buf[ns.length] = 0;
  buf.set(payload, ns.length + 1);
  return toHex(sha256(buf));
}

/**
 * Payload checksum, matching `store::checksum_for` in the Rust server:
 * "sha256:" + lowercase hex of sha256(payload_bytes).
 */
export function checksumFor(payload: Uint8Array): string {
  return "sha256:" + toHex(sha256(payload));
}

/**
 * Build the canonical receipt JSON exactly as the Rust server serializes it
 * (`model::canonicalize_receipt` → `serde_json::to_string` over the struct,
 * whose fields are declared in this exact order).
 *
 * NOTE: `JSON.stringify` emits keys in insertion order for string keys, so the
 * object below must list fields in struct-declaration order.
 */
export function canonicalizeReceipt(receipt: PayloadReceipt): string {
  const ordered: Record<string, unknown> = {
    service: receipt.service,
    action: receipt.action,
    payloadId: receipt.payloadId,
    namespace: receipt.namespace,
    checksum: receipt.checksum,
    sizeBytes: receipt.sizeBytes,
    submittedAt: receipt.submittedAt,
  };
  return JSON.stringify(ordered);
}

/**
 * EIP-191 message digest, matching `signer::eip191_hash`:
 * keccak256("\x19Ethereum Signed Message:\n" + decimalByteLength || messageBytes).
 */
export function eip191Hash(messageBytes: Uint8Array): Uint8Array {
  const prefix = utf8(`\u0019Ethereum Signed Message:\n${messageBytes.length}`);
  const buf = new Uint8Array(prefix.length + messageBytes.length);
  buf.set(prefix, 0);
  buf.set(messageBytes, prefix.length);
  return keccak_256(buf);
}

/**
 * Canonical Ethereum receipt for a payload metadata object, using the current
 * `payloadReceived` action. Useful for recomputing what the server should have
 * signed.
 */
export function receiptForMetadata(meta: {
  id: string;
  namespace: string;
  checksum: string;
  sizeBytes: number;
  submittedAt: string;
}): PayloadReceipt {
  return {
    service: "atlas-payload-provider",
    action: "payloadReceived",
    payloadId: meta.id,
    namespace: meta.namespace,
    checksum: meta.checksum,
    sizeBytes: meta.sizeBytes,
    submittedAt: meta.submittedAt,
  };
}
