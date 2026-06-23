/**
 * Submit a payload to the configured provider and verify the returned EIP-191
 * receipt signature. Loads env from the repository root `.env`:
 *
 *   PAYLOAD_PROVIDER_URL  e.g. https://payload.atlas.arkiv-global.net
 *   INGRESS_BEARER_KEY    bearer token for POST /payloads (anti-DDoS)
 *
 * Usage:
 *   bun run examples/submit-and-verify.ts [text]
 *   bun run examples/submit-and-verify.ts        # defaults to "Hello Atlas"
 */

import {
  PayloadProviderClient,
  predictPayloadId,
  verifySignature,
} from "../src/index.ts";

const url = process.env.PAYLOAD_PROVIDER_URL;
const bearerKey = process.env.INGRESS_BEARER_KEY;

if (!url) {
  console.error("PAYLOAD_PROVIDER_URL is not set (check the repo root .env).");
  process.exit(1);
}

const text = process.argv[2] ?? "Hello Atlas";

const client = new PayloadProviderClient({ baseUrl: url, bearerKey });

console.log(`Provider: ${url}`);
const status = await client.status();
console.log(
  `signingEnabled=${status.signingEnabled} signerAddress=${status.signerAddress ?? "(none)"} ingressProtected=${status.ingressProtected}`,
);

// Predict the content address the server should assign, then confirm it matches.
const predicted = predictPayloadId("atlas.test", text);
console.log(`predicted id: ${predicted.id}`);

const response = await client.submit({
  namespace: "atlas.test",
  contentType: "text/plain",
  payload: text,
});

const meta = response.payload;
console.log(`\nsubmitted (created=${response.created})`);
console.log(`  id:         ${meta.id}`);
console.log(`  checksum:   ${meta.checksum}`);
console.log(`  sizeBytes:  ${meta.sizeBytes}`);

if (predicted.id !== meta.id) {
  console.error(`!! server id does not match predicted id`);
  process.exit(2);
}

if (!meta.signature) {
  console.error("!! server did not return a signature");
  process.exit(3);
}

const result = verifySignature(meta, meta.signature);
console.log(`\nsignature ${result.valid ? "VALID" : "INVALID"}`);
console.log(`  scheme:          ${meta.signature.scheme}`);
console.log(`  signer (claimed): ${result.claimedSigner}`);
console.log(`  signer (recovered): ${result.recoveredAddress}`);
console.log(`  messageHash:     ${result.messageHash}`);
console.log(`  v: ${meta.signature.v}`);
if (result.errors.length > 0) {
  for (const err of result.errors) console.error(`  - ${err}`);
  process.exit(4);
}

// Fetch the raw bytes back and confirm the round-trip.
const raw = await client.getRaw(meta.id);
const roundtrip = new TextDecoder().decode(raw.bytes);
console.log(`\nraw round-trip: ${JSON.stringify(roundtrip)} (${raw.contentType})`);
console.log(`\nOK`);
