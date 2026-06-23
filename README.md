# Atlas Payload Provider


Atlas Payload Provider is a small data availability service for accepting, validating, storing, and serving payload bytes. Payloads are content-addressed, stored on disk as JSON records, and can optionally include an Ethereum-style EIP-191 receipt signature proving that this provider received the payload.

## Quick Start

```bash
cargo run
```

The default HTTP endpoint is:

```text
http://127.0.0.1:28883
```

Open the browser UI at `http://127.0.0.1:28883/`, or submit payloads directly to the API.

## Configuration

Configuration is read from environment variables.

| Variable | Default | Description |
| --- | --- | --- |
| `LISTEN_HOST` | `0.0.0.0` | HTTP bind host. |
| `LISTEN_PORT` | `28883` | HTTP bind port. |
| `WEB_WORKERS` | `4` | Tokio worker thread count. |
| `HTML_TITLE` | `Atlas Payload Provider` | Browser UI title. |
| `PAYLOAD_DIR` | `data/payloads` | Directory for persisted payload records. |
| `MAX_PAYLOAD_BYTES` | `1048576` | Maximum decoded payload size. |
| `INGRESS_BEARER_KEY` | unset | Optional bearer token required for `POST /payloads`. |
| `SIGNER_PRIVATE_KEY` | unset | Optional 0x-prefixed secp256k1 private key for EIP-191 receipt signing. |

Example with signing and ingress protection:

```bash
INGRESS_BEARER_KEY=change-me \
SIGNER_PRIVATE_KEY=0x0000000000000000000000000000000000000000000000000000000000000001 \
cargo run
```

Use a real private key for any shared environment. The private key is never returned by the API; `/status` exposes only the derived signer address.

## API

### Health

```http
GET /healthz
```

Returns a minimal liveness response.

```json
{
  "ok": true,
  "payloadCount": 2,
  "totalBytes": 128
}
```

### Status

```http
GET /status
```

Returns service configuration, storage inventory, signing status, and the latest payload summaries.

```json
{
  "ok": true,
  "service": "atlas-payload-provider",
  "payloadDir": "data/payloads",
  "payloadCount": 2,
  "totalBytes": 128,
  "maxPayloadBytes": 1048576,
  "ingressProtected": false,
  "signingEnabled": true,
  "signerAddress": "0x...",
  "latest": []
}
```

### Submit Payload

```http
POST /payloads
Content-Type: application/json
Authorization: Bearer <INGRESS_BEARER_KEY>
```

`Authorization` is required only when `INGRESS_BEARER_KEY` is configured.

```json
{
  "namespace": "atlas.blocks",
  "contentType": "application/octet-stream",
  "payloadBase64": "aGVsbG8="
}
```

Successful submissions return `201 Created` for a new payload and `200 OK` for an already known payload.
The response returns payload metadata and any receipt signature, but omits `payloadBase64` so large payload bodies are not echoed back.

```json
{
  "ok": true,
  "created": true,
  "payload": {
    "id": "917c1c82e7c7796c10affac9e5566ca876780196e83070b311e3c82226bd09a1",
    "namespace": "atlas.blocks",
    "contentType": "application/octet-stream",
    "sizeBytes": 5,
    "checksum": "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
    "submittedAt": "2026-06-22T12:00:00Z",
    "signature": {
      "scheme": "eip191",
      "signer": "0x...",
      "receipt": {
        "service": "atlas-payload-provider",
        "action": "payloadReceived",
        "payloadId": "917c1c82e7c7796c10affac9e5566ca876780196e83070b311e3c82226bd09a1",
        "namespace": "atlas.blocks",
        "checksum": "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        "sizeBytes": 5,
        "submittedAt": "2026-06-22T12:00:00Z"
      },
      "messageHash": "0x...",
      "signature": "0x...",
      "r": "0x...",
      "s": "0x...",
      "v": 27
    }
  }
}
```

Payload IDs are SHA-256 content addresses over `namespace || 0x00 || payload bytes`. `checksum` is SHA-256 over only the decoded payload bytes.

### Submit ARKIV Payload

```http
POST /arkiv/payloads
Content-Type: application/json
Authorization: Bearer <INGRESS_BEARER_KEY>
```

`/arkiv/payloads` is an adapter for ARKIV entity payload construction. It does not send an on-chain transaction; it canonicalizes or validates ARKIV-shaped payload input, stores the resulting bytes through the same provider receipt flow, and returns normalized ARKIV context that clients can use with the ARKIV SDK.

Submit canonical JSON:

```json
{
  "payloadJson": {
    "entity": {
      "entityType": "document",
      "entityId": "doc-123",
      "entityContent": "Hello from ARKIV"
    }
  },
  "attributes": [
    { "key": "type", "value": "document" },
    { "key": "id", "value": "doc-123" },
    { "key": "version", "value": 1 }
  ],
  "expiresIn": 2592000
}
```

The adapter sorts object keys at every JSON level and stores the compact UTF-8 JSON bytes with `contentType: "application/json"` and default `namespace: "arkiv.entities"`. You may provide `namespace`, `contentType`, `entityKey`, and either `payloadJson` or `payloadBase64`, but not both payload fields.

Successful responses include payload metadata plus normalized ARKIV context. They do not echo the encoded payload body.

```json
{
  "ok": true,
  "created": true,
  "arkiv": {
    "namespace": "arkiv.entities",
    "contentType": "application/json",
    "payloadEncoding": "canonicalJson",
    "attributes": [
      { "key": "id", "value": "doc-123" },
      { "key": "type", "value": "document" },
      { "key": "version", "value": 1 }
    ],
    "expiresIn": 2592000
  },
  "payload": {
    "id": "...",
    "namespace": "arkiv.entities",
    "contentType": "application/json",
    "sizeBytes": 92,
    "checksum": "sha256:...",
    "submittedAt": "2026-06-22T12:00:00Z",
    "signature": {
      "scheme": "eip191",
      "signer": "0x..."
    }
  }
}
```

ARKIV attributes are validated with the chain-compatible `Ident32` shape: non-empty, at most 32 bytes, first character `a` through `z`, then lowercase ASCII letters, digits, `_`, `-`, or `.`. String attribute values are limited to 128 bytes. Attributes are returned sorted by `key`.

### List Payloads

```http
GET /payloads
```

Returns payload summaries. Summaries omit `payloadBase64` but include signature metadata when available.

### Read Payload Metadata

```http
GET /payloads/{id}
```

Returns payload metadata and the full receipt signature when one exists. It does not include `payloadBase64`.

### Read Payload Signature

```http
GET /payloads/{id}/signature
```

Returns only the signature object and payload ID. Unsigned payloads return `404 Not Found`.

### Read Raw Payload Bytes

```http
GET /payloads/{id}/raw
```

Returns the decoded payload body with the stored `contentType`, or `application/octet-stream` when no content type was submitted.

## Receipt Signatures

When `SIGNER_PRIVATE_KEY` is set, each accepted payload receives a signature over a canonical JSON receipt:

```json
{
  "service": "atlas-payload-provider",
  "action": "payloadReceived",
  "payloadId": "<payload id>",
  "namespace": "<namespace>",
  "checksum": "<payload checksum>",
  "sizeBytes": 5,
  "submittedAt": "<timestamp>"
}
```

The receipt is signed with the Ethereum signed message prefix (`EIP-191`) and a secp256k1 private key. This signature is a provider receipt only; it does not submit an Ethereum transaction or prove on-chain inclusion.

## Packaging

Create a local release package with:

```bash
scripts/package.sh
```

The script builds with `cargo build --locked --profile release`, stages the binary with README and deployment docs, then writes `dist/atlas-payload-provider-<version>-<target>.tar.gz` and a matching `.sha256` file.

GitHub Actions runs the same script on pushes to `main`, on any pushed tag, and by manual dispatch. Tag builds also upload the archive and checksum to the matching GitHub release.

## Docker

```bash
docker compose up --build
```

The GitHub Actions package workflow publishes Docker images to GitHub Packages:

```bash
docker pull ghcr.io/atlas-chain/atlas-payload-provider:main
```

Pushes to `main` publish the `main` and commit SHA tags. Version tags publish the matching tag, commit SHA tag, and `latest`.

See `instructions.md` for an operator-oriented Docker Compose example and runtime notes.
