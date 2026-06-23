import { secp256k1 } from "@noble/curves/secp256k1.js";
import { keccak_256 } from "@noble/hashes/sha3.js";
import type { PayloadMetadata, PayloadSignature } from "./types.ts";
import {
  canonicalizeReceipt,
  eip191Hash,
  receiptForMetadata,
  toHex,
  toHexPrefixed,
} from "./receipt.ts";

export interface VerificationResult {
  valid: boolean;
  /** Address recovered from r/s/v over the canonical receipt (lowercase, 0x-prefixed). */
  recoveredAddress: string;
  /** Address the server claims signed (signature.signer). */
  claimedSigner: string;
  /** Recomputed canonical receipt digest (0x-prefixed hex). */
  messageHash: string;
  errors: string[];
}

function hexToBytes(hex: string, expectedBytes: number): Uint8Array {
  const body = hex.startsWith("0x") || hex.startsWith("0X") ? hex.slice(2) : hex;
  if (body.length !== expectedBytes * 2) {
    throw new Error(
      `expected ${expectedBytes} hex bytes, got ${body.length / 2}`,
    );
  }
  const out = new Uint8Array(expectedBytes);
  for (let i = 0; i < expectedBytes; i++) {
    out[i] = parseInt(body.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

function ethereumAddress(publicKeyUncompressed: Uint8Array): string {
  // publicKeyUncompressed = 0x04 || X(32) || Y(32). Drop the 0x04 prefix.
  const digest = keccak_256(publicKeyUncompressed.slice(1));
  return toHexPrefixed(digest.slice(12, 32));
}

/**
 * Verify a server-issued EIP-191 receipt signature against a payload's
 * metadata. Mirrors `signer::validate_payload_signature` from the Rust server.
 *
 * Checks performed:
 *   1. scheme == "eip191"
 *   2. signature.receipt matches the canonical receipt for this metadata
 *      (current `payloadReceived` action OR legacy `hostPayload` action)
 *   3. recomputed messageHash == signature.messageHash
 *   4. v ∈ {27, 28} and r/s/signature fields are internally consistent
 *   5. recovered public key's address == signature.signer (case-insensitive)
 */
export function verifySignature(
  meta: PayloadMetadata,
  signature: PayloadSignature,
): VerificationResult {
  const errors: string[] = [];

  if (signature.scheme !== "eip191") {
    errors.push(`unsupported signature scheme ${signature.scheme}`);
  }

  // The receipt embedded in the signature is authoritative — we sign exactly
  // what the server serialized. Recompute the hash from the embedded receipt.
  const message = canonicalizeReceipt(signature.receipt);
  const hash = eip191Hash(new TextEncoder().encode(message));

  if (toHexPrefixed(hash) !== signature.messageHash) {
    errors.push("signature messageHash does not match the canonical receipt");
  }

  // Cross-check the embedded receipt against the payload metadata (both the
  // current and the legacy action name are accepted, matching the server).
  // Compare via the canonical (struct-order) serialization so that JSON key
  // ordering from the wire does not cause false negatives.
  const current = receiptForMetadata(meta);
  const legacy: typeof current = { ...current, action: "hostPayload" };
  const receiptMatches =
    canonicalizeReceipt(signature.receipt) === canonicalizeReceipt(current) ||
    canonicalizeReceipt(signature.receipt) === canonicalizeReceipt(legacy);
  if (!receiptMatches) {
    errors.push("signature receipt does not match payload metadata");
  }

  let recoveredAddress = "";
  try {
    if (signature.v !== 27 && signature.v !== 28) {
      throw new Error(`signature v must be 27 or 28, got ${signature.v}`);
    }
    const r = BigInt(signature.r);
    const s = BigInt(signature.s);
    const sigObj = new secp256k1.Signature(r, s, signature.v - 27);
    const pub = sigObj.recoverPublicKey(hash).toBytes(false);

    // Internal consistency: r/s/v must equal the packed `signature` field.
    const rBytes = hexToBytes(signature.r, 32);
    const sBytes = hexToBytes(signature.s, 32);
    const sigBytes = new Uint8Array(65);
    sigBytes.set(rBytes, 0);
    sigBytes.set(sBytes, 32);
    sigBytes[64] = signature.v;
    if (toHex(sigBytes) !== (signature.signature.startsWith("0x") ? signature.signature.slice(2) : signature.signature)) {
      errors.push("signature, r, s, and v fields are inconsistent");
    }

    recoveredAddress = ethereumAddress(pub);
    if (recoveredAddress.toLowerCase() !== signature.signer.toLowerCase()) {
      errors.push(
        `recovered address ${recoveredAddress} does not match claimed signer ${signature.signer}`,
      );
    }
  } catch (err) {
    errors.push(`signature recovery failed: ${(err as Error).message}`);
  }

  return {
    valid: errors.length === 0,
    recoveredAddress,
    claimedSigner: signature.signer,
    messageHash: toHexPrefixed(hash),
    errors,
  };
}
