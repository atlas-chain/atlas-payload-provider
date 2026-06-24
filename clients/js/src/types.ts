// Shared types mirroring the Rust server's model.rs.
// Field names match the wire JSON exactly (camelCase as emitted by serde).

export interface PayloadReceipt {
  /** Fixed: "atlas-payload-provider". */
  service: string;
  /** Receipt action. Current scheme: "payloadReceived". Legacy: "hostPayload". */
  action: string;
  /** SHA-256 content address (hex, no 0x prefix) over namespace || 0x00 || payload bytes. */
  payloadId: string;
  namespace: string;
  /** "sha256:" + lowercase hex of SHA-256 over the decoded payload bytes. */
  checksum: string;
  sizeBytes: number;
  /** ISO-8601 UTC, second precision (e.g. "2026-06-23T17:03:19Z"). */
  submittedAt: string;
  /** Optional one-time Arkiv payload-reference nonce. */
  nonce?: string;
  /** Optional signed gas payment amount. */
  payment?: number;
}

export interface PayloadSignature {
  scheme: "eip191";
  /** Lowercase 0x-prefixed Ethereum address of the signer. */
  signer: string;
  receipt: PayloadReceipt;
  /** 0x-prefixed keccak256 of the EIP-191-prefixed canonical receipt JSON. */
  messageHash: string;
  /** 0x-prefixed r || s || v (65 bytes). */
  signature: string;
  /** 0x-prefixed r (32 bytes). */
  r: string;
  /** 0x-prefixed s (32 bytes). */
  s: string;
  /** 27 or 28. */
  v: number;
}

export interface PayloadSummary {
  id: string;
  namespace: string;
  contentType?: string;
  sizeBytes: number;
  checksum: string;
  submittedAt: string;
  signature?: {
    scheme: string;
    signer: string;
    messageHash: string;
    signature: string;
  };
}

export interface PayloadMetadata extends PayloadSummary {
  signature?: PayloadSignature;
}

export interface SubmitResponse {
  ok: boolean;
  /** true for 201 Created (new), false for 200 OK (already known). */
  created: boolean;
  payload: PayloadMetadata;
}

export interface SubmitPayloadInput {
  namespace: string;
  contentType?: string;
  /** Decoded payload bytes. Exactly one of `payload` or `payloadBase64` must be set. */
  payload?: Uint8Array | string;
  /** Base64-encoded payload bytes. */
  payloadBase64?: string;
  /** Optional one-time Arkiv payload-reference nonce. */
  nonce?: string;
  /** Optional signed gas payment amount. */
  payment?: number;
}

export interface StatusResponse {
  ok: boolean;
  service: string;
  payloadDir?: string;
  payloadCount: number;
  totalBytes: number;
  maxPayloadBytes: number;
  ingressProtected: boolean;
  signingEnabled: boolean;
  signerAddress?: string;
  latest: PayloadSummary[];
  endpoints?: string[];
}
