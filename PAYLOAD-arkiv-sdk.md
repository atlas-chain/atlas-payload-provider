# ARKIV Payload Construction

This document describes how the SDK builds ARKIV entity payloads and how to build a
deterministic canonical JSON byte payload before sending it to ARKIV.

## Payload Basics

An ARKIV entity payload is arbitrary bytes. In this SDK the write APIs accept that
payload as a `Uint8Array`:

```ts
type CreateEntityParameters = {
  payload: Uint8Array
  attributes: Attribute[]
  contentType: MimeType | string
  expiresIn: number
}
```

For JSON data, the SDK helper in `src/utils/payload.ts` converts an object into
UTF-8 bytes:

```ts
import { toBytes } from "viem"

export function jsonToPayload(json: object): Uint8Array {
  return toBytes(JSON.stringify(json))
}
```

For plain text, `stringToPayload(data)` returns `toBytes(data)`.

The SDK does not enforce a fixed JSON schema inside the payload. The application
chooses the JSON fields. Existing examples use a shape like this:

```json
{
  "entity": {
    "entityType": "document",
    "entityId": "doc-123",
    "entityContent": "Hello from ARKIV"
  }
}
```

When `contentType` is `"application/json"`, readers can call `entity.toJson()`,
which converts the returned payload bytes to text and then runs `JSON.parse`.

## Data Sent With a Payload

The payload bytes are only one part of an ARKIV mutation. For create and update
operations the SDK accepts:

- `payload`: the entity bytes, usually JSON or text.
- `contentType`: a MIME type such as `"application/json"` or `"text/plain"`.
- `attributes`: queryable key-value attributes stored separately from the
  payload. Attributes must be sorted by name ascending before they are sent.
- `expiresIn`: lifetime in seconds. Create and extend operations convert this to
  ARKIV blocks. Update operations currently encode `expiresAt` as `0`, which the
  contract ignores for updates.

Attributes are metadata for indexing and queries. They are not embedded into the
payload unless your application also puts them there.

## Contract Operation Shape

The SDK batches mutations into a call to the ARKIV contract:

```solidity
execute(
  (
    uint8 operationType,
    bytes32 entityKey,
    bytes payload,
    (bytes32[4] data) contentType,
    (bytes32 name, uint8 valueType, bytes32[4] value)[] attributes,
    uint32 expiresAt,
    address newOwner
  )[] ops
)
```

Operation type values come from `EntityOperationType`:

```ts
enum EntityOperationType {
  Create = 1,
  Update = 2,
  Extend = 3,
  Transfer = 4,
  Delete = 5,
  Expire = 6,
}
```

The SDK builds each operation as follows:

| SDK action | `operationType` | `payload` | `contentType` | `attributes` | `expiresAt` | `newOwner` |
| --- | ---: | --- | --- | --- | --- | --- |
| Create | `1` | `toHex(item.payload)` | encoded MIME | encoded attributes | `ceil(expiresIn / BLOCK_TIME)` | zero address |
| Update | `2` | `toHex(item.payload)` | encoded MIME | encoded attributes | `0` | zero address |
| Delete | `5` | empty bytes | empty MIME | empty array | `0` | zero address |
| Extend | `3` | empty bytes | empty MIME | empty array | `ceil(expiresIn / BLOCK_TIME)` | zero address |
| Transfer | `4` | empty bytes | empty MIME | empty array | `0` | target owner |

For create operations, the SDK derives the entity key before sending the
transaction:

```ts
entityKey = keccak256(
  encodePacked(
    ["uint256", "address", "address", "uint32"],
    [chainId, ARKIV_ADDRESS, owner, nonce],
  ),
)
```

For batched creates, `nonce` is incremented for each create in the batch.

## Entity Key Vs Application Entity ID

During creation, the caller does not provide the ARKIV `entityKey`. The SDK
derives it from `chainId`, the ARKIV registry address, the owner address, and the
owner nonce. That means create operations do not accept an `entityKey` or
protocol-level entity ID chosen by the application.

Applications may still put their own ID inside the JSON payload:

```json
{
  "entity": {
    "entityId": "doc-123",
    "entityType": "document"
  }
}
```

That `entityId` is just application data. ARKIV does not treat it as the
protocol entity key and does not enforce uniqueness for it.

If the application ID should be queryable, also store it as an attribute with a
valid lowercase `Ident32` name, for example:

```ts
attributes: canonicalAttributes([
  { key: "id", value: "doc-123" },
  { key: "type", value: "document" },
])
```

Do not use camelCase attribute names such as `entityId`: attribute names are
validated by the chain as lowercase `Ident32` values. CamelCase is fine inside
the JSON payload, but not as an attribute key.

## MIME And Attribute Encoding

The contract stores MIME type and string attributes in fixed-size byte containers.

### MIME Type

`contentType` is UTF-8 encoded, left-aligned into `bytes32[4]`, and zero padded to
128 bytes. Empty MIME data is four zero `bytes32` values.

For example, `"application/json"` is encoded as the UTF-8 bytes for that string,
followed by zero bytes until the 128-byte container is full.

### Attributes

Each attribute is encoded as:

```ts
{
  name: bytes32,
  valueType: uint8,
  value: bytes32[4],
}
```

Attribute names are UTF-8 encoded, left-aligned into `bytes32`, and zero padded.
Names therefore need to fit into 32 bytes.

Attribute names are validated by the chain as `Ident32`: non-empty, at most 32
bytes, starting with `a` through `z`, then followed by lowercase ASCII letters,
digits, `_`, `-`, or `.`.

String attribute values use `AttributeValueType.String` (`2`) and are UTF-8
encoded into the 128-byte `bytes32[4]` container.

Numeric attribute values use `AttributeValueType.Uint` (`1`). The numeric value is
encoded as a `uint256` in the first `bytes32`; the remaining three words are zero.

Attributes must be sorted by attribute name ascending. The sibling ARKIV CLI
builds `Attribute[]` "sorted by name ascending as the contract requires for
deterministic hashing". The SDK currently preserves the caller's order, so sort
attributes before passing them to `createEntity`, `updateEntity`, or
`mutateEntities`.

Because valid attribute names are restricted to lowercase ASCII, JavaScript's
plain `<` / `>` string comparison gives the same order as byte-wise ascending
comparison for valid names:

```ts
import type { Attribute } from "@atlas-chain/sdk"

export function canonicalAttributes(attributes: Attribute[]): Attribute[] {
  return [...attributes].sort((a, b) => {
    if (a.key < b.key) return -1
    if (a.key > b.key) return 1
    return 0
  })
}
```

Example:

```ts
const attributes = canonicalAttributes([
  { key: "version", value: "1.0" },
  { key: "category", value: "documentation" },
])

// Result:
// [
//   { key: "category", value: "documentation" },
//   { key: "version", value: "1.0" },
// ]
```

## Canonical JSON Form

`jsonToPayload` uses plain `JSON.stringify`. That creates valid JSON payload
bytes, but it is not a full canonicalizer: object key order follows the object's
insertion order. If two clients build the same logical object with different
insertion orders, they can produce different byte payloads.

For ARKIV data that needs stable hashing, signatures, comparisons, or
reproducible tests, canonicalize the JSON first and then encode the canonical
string as UTF-8 bytes.

Recommended canonical JSON rules:

- Use only JSON-compatible values: objects, arrays, strings, finite numbers,
  booleans, and `null`.
- Do not use `BigInt`, `Date`, `Map`, `Set`, functions, symbols, `undefined`,
  `NaN`, or `Infinity` inside the payload object.
- Sort object keys lexicographically at every nesting level.
- Preserve array order.
- Emit compact JSON with no extra whitespace.
- Encode the resulting string as UTF-8 bytes.

Example canonicalizer:

```ts
import { toBytes } from "viem"

type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue | undefined }

function normalizeJson(value: JsonValue): JsonValue {
  if (value === null || typeof value !== "object") {
    return value
  }

  if (Array.isArray(value)) {
    return value.map((item) => normalizeJson(item))
  }

  return Object.keys(value)
    .sort()
    .reduce<{ [key: string]: JsonValue }>((result, key) => {
      const item = value[key]
      if (item !== undefined) {
        result[key] = normalizeJson(item)
      }
      return result
    }, {})
}

export function canonicalJson(value: JsonValue): string {
  return JSON.stringify(normalizeJson(value))
}

export function canonicalJsonToPayload(value: JsonValue): Uint8Array {
  return toBytes(canonicalJson(value))
}
```

Input:

```ts
const data = {
  entity: {
    entityType: "document",
    entityId: "doc-123",
    entityContent: "Hello from ARKIV",
  },
}
```

Canonical JSON string:

```json
{"entity":{"entityContent":"Hello from ARKIV","entityId":"doc-123","entityType":"document"}}
```

Payload bytes:

```ts
const payload = canonicalJsonToPayload(data)
```

Use those bytes in the SDK exactly like `jsonToPayload` output:

```ts
const { entityKey, txHash } = await client.createEntity({
  payload: canonicalJsonToPayload({
    entity: {
      entityType: "document",
      entityId: "doc-123",
      entityContent: "Hello from ARKIV",
    },
  }),
  contentType: "application/json",
  attributes: canonicalAttributes([
    { key: "category", value: "documentation" },
    { key: "version", value: "1.0" },
  ]),
  expiresIn: ExpirationTime.fromDays(30),
})
```

Taken together, a deterministic ARKIV JSON entity should be built as:

```ts
const payload = canonicalJsonToPayload(data)
const attributes = canonicalAttributes(rawAttributes)

await client.createEntity({
  payload,
  contentType: "application/json",
  attributes,
  expiresIn,
})
```

## Full Batch Mutation Example

Use `mutateEntities` when one transaction should contain multiple ARKIV
operations. Each create or update carries its own canonical payload bytes,
content type, sorted attributes, and expiration input. Deletes only need the
entity key.

```ts
import {
  type Attribute,
  type Hex,
  createWalletClient,
  http,
  toBytes,
} from "@atlas-chain/sdk"
import { privateKeyToAccount } from "@atlas-chain/sdk/accounts"
import { braga } from "@atlas-chain/sdk/chains"
import { ExpirationTime } from "@atlas-chain/sdk/utils"

type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue | undefined }

function normalizeJson(value: JsonValue): JsonValue {
  if (value === null || typeof value !== "object") {
    return value
  }

  if (Array.isArray(value)) {
    return value.map((item) => normalizeJson(item))
  }

  return Object.keys(value)
    .sort()
    .reduce<{ [key: string]: JsonValue }>((result, key) => {
      const item = value[key]
      if (item !== undefined) {
        result[key] = normalizeJson(item)
      }
      return result
    }, {})
}

function canonicalJsonToPayload(value: JsonValue): Uint8Array {
  return toBytes(JSON.stringify(normalizeJson(value)))
}

function canonicalAttributes(attributes: Attribute[]): Attribute[] {
  return [...attributes].sort((a, b) => {
    if (a.key < b.key) return -1
    if (a.key > b.key) return 1
    return 0
  })
}

const client = createWalletClient({
  chain: braga,
  transport: http(),
  account: privateKeyToAccount("0x..."),
})

const existingEntityKey =
  "0x1111111111111111111111111111111111111111111111111111111111111111" as Hex
const staleEntityKey =
  "0x2222222222222222222222222222222222222222222222222222222222222222" as Hex

const result = await client.mutateEntities({
  creates: [
    {
      payload: canonicalJsonToPayload({
        entity: {
          body: "First canonical document",
          id: "doc-001",
          type: "document",
        },
      }),
      contentType: "application/json",
      attributes: canonicalAttributes([
        { key: "type", value: "document" },
        { key: "id", value: "doc-001" },
        { key: "version", value: 1 },
      ]),
      expiresIn: ExpirationTime.fromDays(30),
    },
    {
      payload: canonicalJsonToPayload({
        entity: {
          body: "Second canonical document",
          id: "doc-002",
          type: "document",
        },
      }),
      contentType: "application/json",
      attributes: canonicalAttributes([
        { key: "type", value: "document" },
        { key: "id", value: "doc-002" },
        { key: "version", value: 1 },
      ]),
      expiresIn: ExpirationTime.fromDays(30),
    },
  ],
  updates: [
    {
      entityKey: existingEntityKey,
      payload: canonicalJsonToPayload({
        entity: {
          body: "Updated canonical document",
          id: "doc-existing",
          type: "document",
        },
      }),
      contentType: "application/json",
      attributes: canonicalAttributes([
        { key: "id", value: "doc-existing" },
        { key: "status", value: "updated" },
        { key: "type", value: "document" },
        { key: "version", value: 2 },
      ]),
      expiresIn: ExpirationTime.fromDays(30),
    },
  ],
  deletes: [
    {
      entityKey: staleEntityKey,
    },
  ],
})

console.log("Transaction hash:", result.txHash)
console.log("Created entity keys:", result.createdEntities)
console.log("Updated entity keys:", result.updatedEntities)
console.log("Deleted entity keys:", result.deletedEntities)
```

The SDK converts the example above into a single `execute(ops)` call. The
operation array is built in this order: all creates, then all updates, then all
deletes, then extensions, then ownership changes.

Conceptually, the contract receives this shape:

```ts
[
  {
    operationType: 1, // Create
    entityKey: "<derived from owner nonce for doc-001>",
    payload: "0x...", // canonical JSON bytes
    contentType: "<application/json encoded as bytes32[4]>",
    attributes: "<id, type, version sorted by name and ABI encoded>",
    expiresAt: Math.ceil(ExpirationTime.fromDays(30) / BLOCK_TIME),
    newOwner: "0x0000000000000000000000000000000000000000",
  },
  {
    operationType: 1, // Create
    entityKey: "<derived from owner nonce for doc-002>",
    payload: "0x...", // canonical JSON bytes
    contentType: "<application/json encoded as bytes32[4]>",
    attributes: "<id, type, version sorted by name and ABI encoded>",
    expiresAt: Math.ceil(ExpirationTime.fromDays(30) / BLOCK_TIME),
    newOwner: "0x0000000000000000000000000000000000000000",
  },
  {
    operationType: 2, // Update
    entityKey: existingEntityKey,
    payload: "0x...", // canonical JSON bytes
    contentType: "<application/json encoded as bytes32[4]>",
    attributes: "<id, status, type, version sorted by name and ABI encoded>",
    expiresAt: 0,
    newOwner: "0x0000000000000000000000000000000000000000",
  },
  {
    operationType: 5, // Delete
    entityKey: staleEntityKey,
    payload: "0x",
    contentType: "<empty bytes32[4]>",
    attributes: [],
    expiresAt: 0,
    newOwner: "0x0000000000000000000000000000000000000000",
  },
]
```

## Reading The Payload Back

When querying, include payload data if you need the bytes:

```ts
const entity = await publicClient.getEntity(entityKey)
const text = entity.toText()
const json = entity.toJson()
```

Raw queries and query builders can omit payload bytes for lighter responses. If
payload data was not requested, `entity.payload` is `undefined` and `toText()` /
`toJson()` will throw.
