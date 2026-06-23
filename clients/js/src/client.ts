import {
  checksumFor,
  payloadId,
  toHex,
} from "./receipt.ts";
import type {
  PayloadMetadata,
  PayloadSignature,
  StatusResponse,
  SubmitPayloadInput,
  SubmitResponse,
} from "./types.ts";

export interface ClientOptions {
  /** Base URL of the payload provider, e.g. "https://payload.atlas.arkiv-global.net". */
  baseUrl: string;
  /** Optional bearer token sent as `Authorization: Bearer <key>`. Required when the server has ingress protection. */
  bearerKey?: string;
  /** Optional fetch override (defaults to the global fetch). */
  fetch?: typeof fetch;
}

/**
 * Thin HTTP client for the Atlas Payload Provider REST API.
 * See the repo README for endpoint semantics. All methods are async.
 */
export class PayloadProviderClient {
  private readonly baseUrl: string;
  private readonly bearerKey?: string;
  private readonly fetchImpl: typeof fetch;

  constructor(opts: ClientOptions) {
    this.baseUrl = opts.baseUrl.replace(/\/+$/, "");
    this.bearerKey = opts.bearerKey;
    this.fetchImpl = opts.fetch ?? fetch;
  }

  private url(path: string): string {
    return `${this.baseUrl}${path}`;
  }

  private authHeaders(): Record<string, string> {
    return this.bearerKey ? { Authorization: `Bearer ${this.bearerKey}` } : {};
  }

  private async parse(response: Response): Promise<unknown> {
    const text = await response.text();
    if (!response.ok) {
      throw new Error(`${response.status} ${response.statusText}: ${text}`);
    }
    if (!text) return null;
    try {
      return JSON.parse(text);
    } catch {
      return text;
    }
  }

  /** `GET /healthz` */
  async health(): Promise<{ ok: boolean; payloadCount: number; totalBytes: number }> {
    const res = await this.fetchImpl(this.url("/healthz"));
    return (await this.parse(res)) as {
      ok: boolean;
      payloadCount: number;
      totalBytes: number;
    };
  }

  /** `GET /status` */
  async status(): Promise<StatusResponse> {
    const res = await this.fetchImpl(this.url("/status"));
    return (await this.parse(res)) as StatusResponse;
  }

  /**
   * `POST /payloads`. Returns the server response including the receipt
   * signature when signing is enabled.
   */
  async submit(input: SubmitPayloadInput): Promise<SubmitResponse> {
    if ((input.payload == null) === (input.payloadBase64 == null)) {
      throw new Error(
        "exactly one of `payload` or `payloadBase64` must be provided",
      );
    }
    const body: Record<string, unknown> = {
      namespace: input.namespace,
    };
    if (input.contentType) body.contentType = input.contentType;

    if (input.payloadBase64 != null) {
      body.payloadBase64 = input.payloadBase64;
    } else {
      const bytes =
        typeof input.payload === "string"
          ? new TextEncoder().encode(input.payload)
          : (input.payload as Uint8Array);
      body.payloadBase64 = bytesToBase64(bytes);
    }

    const res = await this.fetchImpl(this.url("/payloads"), {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        ...this.authHeaders(),
      },
      body: JSON.stringify(body),
    });
    return (await this.parse(res)) as SubmitResponse;
  }

  /** `GET /payloads` — list payload summaries. */
  async list(): Promise<PayloadMetadata[]> {
    const res = await this.fetchImpl(this.url("/payloads"), {
      headers: this.authHeaders(),
    });
    const data = (await this.parse(res)) as { payloads?: PayloadMetadata[] } | PayloadMetadata[];
    return Array.isArray(data) ? data : (data.payloads ?? []);
  }

  /** `GET /payloads/{id}` — payload metadata + full signature. */
  async get(id: string): Promise<PayloadMetadata> {
    const res = await this.fetchImpl(this.url(`/payloads/${id}`), {
      headers: this.authHeaders(),
    });
    const data = (await this.parse(res)) as { ok: boolean; payload: PayloadMetadata };
    return data.payload;
  }

  /** `GET /payloads/{id}/signature` — signature object only. */
  async getSignature(id: string): Promise<{ ok: boolean; payloadId: string; signature: PayloadSignature }> {
    const res = await this.fetchImpl(this.url(`/payloads/${id}/signature`), {
      headers: this.authHeaders(),
    });
    return (await this.parse(res)) as {
      ok: boolean;
      payloadId: string;
      signature: PayloadSignature;
    };
  }

  /** `GET /payloads/{id}/raw` — decoded payload bytes. */
  async getRaw(id: string): Promise<{ bytes: Uint8Array; contentType: string }> {
    const res = await this.fetchImpl(this.url(`/payloads/${id}/raw`), {
      headers: this.authHeaders(),
    });
    if (!res.ok) {
      throw new Error(`${res.status} ${res.statusText}: ${await res.text()}`);
    }
    const buffer = new Uint8Array(await res.arrayBuffer());
    return {
      bytes: buffer,
      contentType: res.headers.get("content-type") ?? "application/octet-stream",
    };
  }
}

/**
 * Predict the content address and checksum the server will assign to a payload
 * without submitting it. Useful for comparing against the server's response.
 */
export function predictPayloadId(namespace: string, payload: Uint8Array | string): {
  id: string;
  checksum: string;
  sizeBytes: number;
} {
  const bytes =
    typeof payload === "string" ? new TextEncoder().encode(payload) : payload;
  return {
    id: payloadId(namespace, bytes),
    checksum: checksumFor(bytes),
    sizeBytes: bytes.length,
  };
}

// Bun has both `btoa` (base64 of a binary string) and `Buffer`. Use the global
// `Buffer` when available (Bun/Node) for correct UTF-8 → base64 of arbitrary
// bytes, and fall back to a manual implementation otherwise.
interface BufferLike {
  from(b: Uint8Array): { toString(encoding: "base64"): string };
  from(s: string, encoding: "base64"): Uint8Array;
}
type GlobalWithBuffer = { Buffer?: BufferLike };

export function bytesToBase64(bytes: Uint8Array): string {
  const g = globalThis as GlobalWithBuffer;
  if (g.Buffer) return g.Buffer.from(bytes).toString("base64");
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin);
}

export function base64ToBytes(b64: string): Uint8Array {
  const g = globalThis as GlobalWithBuffer;
  if (g.Buffer) return g.Buffer.from(b64, "base64");
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

// Re-exported for callers that want the low-level helpers without importing
// receipt.ts separately.
export { toHex };
