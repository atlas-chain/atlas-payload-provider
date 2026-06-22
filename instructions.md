# Atlas Payload Provider Integration

Run `atlas-payload-provider` as a small HTTP service next to Atlas network services that need a simple data availability layer. Clients submit payload bytes, the service validates them, persists them by content address, and hosts them for later retrieval.

The default local endpoint is:

```text
http://<provider-host>:28883
```

## Required Runtime Configuration

```env
LISTEN_HOST=0.0.0.0
LISTEN_PORT=28883
HTML_TITLE="Atlas Payload Provider"
PAYLOAD_DIR=/data/payloads
MAX_PAYLOAD_BYTES=1048576
INGRESS_BEARER_KEY=<optional submit token>
SIGNER_PRIVATE_KEY=<optional 0x-prefixed ethereum private key>
```

Leave `INGRESS_BEARER_KEY` unset for open local development. Set it in shared or public environments so `POST /payloads` requires `Authorization: Bearer <token>`.

Set `SIGNER_PRIVATE_KEY` to have the provider sign every newly accepted payload receipt with an Ethereum-style EIP-191 secp256k1 signature. The private key is never returned by the API; `/status` exposes only the derived signer address.

## API Shape

Submit a payload:

```bash
curl -X POST http://localhost:28883/payloads \
  -H 'content-type: application/json' \
  -d '{
    "namespace": "atlas.blocks",
    "contentType": "application/octet-stream",
    "payloadBase64": "aGVsbG8="
  }'
```

Read metadata and encoded payload:

```text
GET /payloads/<payload-id>
```

Read raw bytes:

```text
GET /payloads/<payload-id>/raw
```

Check service health and inventory:

```text
GET /healthz
GET /status
GET /payloads
```

Payload IDs are SHA-256 content addresses over `namespace || 0x00 || payload bytes`. Checksums are SHA-256 over only the payload bytes.

When signing is enabled, payload records include a receipt signature confirming that this provider received this exact payload:

```json
{
  "signature": {
    "scheme": "eip191",
    "signer": "0x...",
    "receipt": {
      "service": "atlas-payload-provider",
      "action": "payloadReceived",
      "payloadId": "..."
    },
    "messageHash": "0x...",
    "signature": "0x...",
    "r": "0x...",
    "s": "0x...",
    "v": 27
  }
}
```

## Docker Compose Example

```yaml
services:
  payload-provider:
    image: ghcr.io/atlas-chain/atlas-payload-provider:main
    ports:
      - "28883:28883"
    environment:
      LISTEN_HOST: "0.0.0.0"
      LISTEN_PORT: "28883"
      HTML_TITLE: "Atlas Payload Provider"
      PAYLOAD_DIR: /data/payloads
      MAX_PAYLOAD_BYTES: "1048576"
      INGRESS_BEARER_KEY: ${INGRESS_BEARER_KEY}
      SIGNER_PRIVATE_KEY: ${SIGNER_PRIVATE_KEY}
    volumes:
      - ./data/payloads:/data/payloads
    restart: unless-stopped
```

## Operating Notes

- Keep `PAYLOAD_DIR` on durable storage; every accepted payload is persisted as one JSON record.
- Use `/healthz` for liveness checks and `/status` for current storage inventory.
- Increase `MAX_PAYLOAD_BYTES` only when downstream nodes are ready to serve and consume larger payloads.
