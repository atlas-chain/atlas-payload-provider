# Simple API Explanation

The sender can upload the payload and receive the detached signature, including the signed receipt, in the same response.

Example call:

```bash
curl -sS -X POST http://127.0.0.1:28883/arkiv/payloads \
  -H 'content-type: application/json' \
  --data '{
    "payloadJson": {
      "entity": {
        "entityType": "document",
        "entityId": "doc-one-call",
        "entityContent": "Hello one call"
      }
    },
    "attributes": [
      { "key": "kind", "value": "document" }
    ]
  }' | jq .
```

Response shape:

```jsonc
{
  "ok": true,
  "created": true,

  // Arkiv-side normalized metadata.
  "arkiv": {
    "attributes": [
      {
        "key": "kind",
        "value": "document"
      }
    ],
    "contentType": "application/json",
    "namespace": "arkiv.entities",
    "payloadEncoding": "canonicalJson"
  },

  // No payloadBase64 here, so no large body is echoed back.
  "payload": {
    "id": "aa1068ca4a884d0e996ca647b5235a6cc97a3d80cfdda324650f5653dfdcb61b",
    "namespace": "arkiv.entities",
    "contentType": "application/json",
    "sizeBytes": 95,
    "checksum": "sha256:816d60fbd138e62d511d15e6d9e40999fb1859a3f287e19ec424193262101e59",
    "submittedAt": "2026-06-23T13:02:48Z",

    // This is the receipt plus signature in the same upload response.
    "signature": {
      "scheme": "eip191",
      "signer": "0x7e5f4552091a69125d5dfcb7b8c2659029395bdf",

      // Receipt is embedded here. This is what was signed.
      "receipt": {
        "service": "atlas-payload-provider",
        "action": "payloadReceived",
        "payloadId": "aa1068ca4a884d0e996ca647b5235a6cc97a3d80cfdda324650f5653dfdcb61b",
        "namespace": "arkiv.entities",
        "checksum": "sha256:816d60fbd138e62d511d15e6d9e40999fb1859a3f287e19ec424193262101e59",
        "sizeBytes": 95,
        "submittedAt": "2026-06-23T13:02:48Z"
      },

      "messageHash": "0xa11f9a98aeab4f3a4d93d1eb6720a824d411b90d7ec976a7ceddfda1513c9724",
      "signature": "0x9bdb0096fb88b974847bfa99da3b15d3d4894d09a78ab4708c60504cd80f8fe12adbcec089dd5f71942c4b6ffcd383890b3ba0f2e0d6301bdad4d1c1d52cf7061b",
      "r": "0x9bdb0096fb88b974847bfa99da3b15d3d4894d09a78ab4708c60504cd80f8fe1",
      "s": "0x2adbcec089dd5f71942c4b6ffcd383890b3ba0f2e0d6301bdad4d1c1d52cf706",
      "v": 27
    }
  }
}
```

Short version:

```text
sender POSTs large payload once
provider stores payload
provider returns small metadata + receipt + signature
sender does not need to call /payloads/{id}/signature unless it lost the signature later
```

