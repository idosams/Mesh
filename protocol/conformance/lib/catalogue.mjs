// protocol/conformance/lib/catalogue.mjs — the whole case catalogue, built from the published
// material at load time (task 01KZC2P6SQ7E00ENWQ6F4YVXJJ).
//
// Nothing here is a literal list of cases. Publishing a new record type, a new message or a new
// error code adds its cases; a case list maintained by hand is a case list that falls behind the
// specification exactly when the specification changes, which is the moment conformance matters.

import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { profileControlCase, recordCases } from "./cases-records.mjs";
import { deliveryCases, headCases, messageCases, publicationCases } from "./cases-messages.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
export const PROTOCOL_ROOT = join(HERE, "..", "..");

const read = (...parts) => JSON.parse(readFileSync(join(PROTOCOL_ROOT, ...parts), "utf8"));

export function loadPublished() {
  const schema = read("schemas", "canonical-encoding-v0.json");
  const wire = read("wire", "v0", "messages.json");
  const index = read("test-vectors", "v0", "index.json");

  const files = readdirSync(join(PROTOCOL_ROOT, "test-vectors", "v0"))
    .filter((name) => name.endsWith(".json") && name !== "index.json")
    .sort();

  const indexed = new Set(index.files.map((entry) => entry.file));
  for (const name of files) {
    if (!indexed.has(name)) {
      throw new Error(
        `protocol/test-vectors/v0/${name} is not in index.json. The catalogue is built from the ` +
          "index, so an unindexed vector file would silently contribute no cases.",
      );
    }
  }

  const vectors = index.files.map((entry) => read("test-vectors", "v0", entry.file));
  return { schema, wire, index, vectors };
}

export function buildCatalogue(published) {
  const cases = [
    ...recordCases(published),
    profileControlCase(published.vectors),
    ...messageCases(published.wire),
    ...headCases(published.wire),
    ...deliveryCases(published.wire),
    ...publicationCases(),
  ];

  const seen = new Set();
  for (const item of cases) {
    if (seen.has(item.id)) throw new Error(`two conformance cases share the id ${item.id}`);
    seen.add(item.id);
  }
  return cases;
}

export const FAMILIES = {
  ENC: "record encoding — the bytes a signed record is",
  PROF: "the mesh-cbor/0 profile's exclusions, as refusals a decoder must make",
  ID: "record identity — the derivation of record_id_hex",
  MSG: "message exchange — the frame, the tags, the planes and the preconditions",
  ERR: "the closed error-code set and its retryability",
  HEAD: "head advancement",
  DELIV: "delivery semantics",
  PUB: "publication authority",
};
