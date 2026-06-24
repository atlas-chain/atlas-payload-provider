# @atlas-chain/payload-client

A small TypeScript client and EIP-191 receipt-signature verifier for the
[Atlas Payload Provider](../../README.md). Designed to run on [Bun](https://bun.sh).

The provider issues an Ethereum-style (EIP-191) signature over a canonical JSON
receipt for every accepted payload. This library:

- submits and reads payloads (`POST /payloads`, `GET /payloads/{id}`, ...),
- recomputes the canonical receipt **exactly** as the Rust server serializes it,
- recovers the signer address from `r/s/v` and checks it against the server's
  claimed `signerAddress`.

## Setup

The env vars live in the repository root `.env` (already git-ignored):

```dotenv
PAYLOAD_PROVIDER_URL=https://payload.atlas.arkiv-global.net
INGRESS_BEARER_KEY=<bearer token, only needed when the server is ingress-protected>
```

Install dependencies once:

```bash
cd clients/js
bun install
```

## Examples

Both examples auto-load the repository root `.env` via Bun.

```bash
# Submit a payload and verify the returned receipt signature.
bun run examples/submit-and-verify.ts "Hello Atlas"

# Verify an already-stored payload by id.
bun run examples/verify.ts 508f7404354d20c6592dd3862b0f7849990108421ee23ef2b5d17d06eaa1d295
```

## Library usage

```ts
import {
  PayloadProviderClient,
  predictPayloadId,
  verifySignature,
} from "@atlas-chain/payload-client";

const client = new PayloadProviderClient({
  baseUrl: process.env.PAYLOAD_PROVIDER_URL!,
  bearerKey: process.env.INGRESS_BEARER_KEY,
});

const { id } = predictPayloadId("atlas.test", "Hello Atlas");
const { payload } = await client.submit({
  namespace: "atlas.test",
  contentType: "text/plain",
  payload: "Hello Atlas",
});

const result = verifySignature(payload, payload.signature!);
console.log(result.valid, result.recoveredAddress);
```

## How verification works

The server builds a receipt (`src/model.rs` → `PayloadReceipt`) and serializes
it with `serde_json::to_string` — compact, keys in struct-declaration order:

```json
{"service":"atlas-payload-provider","action":"payloadReceived","payloadId":"<id>","namespace":"<ns>","checksum":"sha256:<hex>","sizeBytes":25,"submittedAt":"<iso>","nonce":"0x<optional bytes32>","payment":100000}
```

`nonce` and `payment` are present only when the submission supplied
them, for example from ARKIV reference-mode SDK calls.

The signed digest is `keccak256("\x19Ethereum Signed Message:\n" + len + receipt)` (see
`src/signer.rs` → `eip191_hash`). This library reproduces that byte-for-byte and
then recovers the secp256k1 public key from `r/s/v` to derive the Ethereum
address, comparing it to the server's claimed `signer`.

| Rust (server)                  | TypeScript (this client)        |
| ------------------------------ | ------------------------------- |
| `store::payload_id`            | `payloadId`                     |
| `store::checksum_for`          | `checksumFor`                   |
| `model::canonicalize_receipt`  | `canonicalizeReceipt`           |
| `signer::eip191_hash`          | `eip191Hash`                    |
| `signer::validate_payload_signature` | `verifySignature`          |

## Layout

```
src/
  types.ts     wire types mirroring the Rust model
  receipt.ts   payload id / checksum / canonical receipt / EIP-191 digest
  verify.ts    signature recovery + verification
  client.ts    PayloadProviderClient HTTP wrapper
  index.ts     barrel exports
examples/
  submit-and-verify.ts
  verify.ts
```
