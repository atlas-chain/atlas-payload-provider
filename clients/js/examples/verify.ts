/**
 * Verify the receipt signature of an already-stored payload by ID.
 *
 * Usage:
 *   bun run examples/verify.ts <payloadId>
 *
 * Reads PAYLOAD_PROVIDER_URL and INGRESS_BEARER_KEY from the repo root .env.
 */

import { PayloadProviderClient, verifySignature } from "../src/index.ts";

const url = process.env.PAYLOAD_PROVIDER_URL;
const bearerKey = process.env.INGRESS_BEARER_KEY;

if (!url) {
  console.error("PAYLOAD_PROVIDER_URL is not set (check the repo root .env).");
  process.exit(1);
}

const id = process.argv[2];
if (!id) {
  console.error("usage: bun run examples/verify.ts <payloadId>");
  process.exit(1);
}

const client = new PayloadProviderClient({ baseUrl: url, bearerKey });

const meta = await client.get(id);
console.log(`payload ${meta.id}`);
console.log(`  namespace:  ${meta.namespace}`);
console.log(`  checksum:   ${meta.checksum}`);
console.log(`  sizeBytes:  ${meta.sizeBytes}`);

if (!meta.signature) {
  console.error("!! payload has no signature");
  process.exit(2);
}

const result = verifySignature(meta, meta.signature);
console.log(`\nsignature ${result.valid ? "VALID" : "INVALID"}`);
console.log(`  signer (claimed):   ${result.claimedSigner}`);
console.log(`  signer (recovered): ${result.recoveredAddress}`);
console.log(`  messageHash:        ${result.messageHash}`);
if (result.errors.length > 0) {
  for (const err of result.errors) console.error(`  - ${err}`);
  process.exit(3);
}
console.log(`\nOK`);
