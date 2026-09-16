#!/usr/bin/env node
// protocol/verify-published.mjs — the published protocol held against the implementation and
// against itself (task 01KZC2JP6ABV1J5RJ7A6ESKE8M).
//
// This directory claims that an outside party can build a client from it alone. That claim is only
// worth something if something checks it, so this file is a second `mesh-cbor/0` implementation —
// written against `test-vectors/README.md` and `wire/README.md`, not against the Rust — that
// re-encodes every published example and compares bytes.
//
// Contract, deliberately:
//  - Zero network, zero dependencies, no build step, well under a second.
//  - Exit code is the verdict. Every finding names the check and the item it is about.
//  - `--self-test` breaks each check on purpose and asserts it fires. A check only ever observed to
//    pass is not evidence that it can fail.
//
// What it does NOT do, stated rather than left to be discovered:
//  - It does not verify `canonical_encoding_digest_hex`. That needs BLAKE3 and this file carries no
//    dependencies. Use any BLAKE3 library for that step.
//  - It does not verify `record_id_hex`, which cannot be recomputed from the published material at
//    all — README.md 6.1, tracked as 01KZCZDTVD0D36W5YRGX8CNE17.
//  - Its readers of the Rust sources are lints over text, not a parser. They read the exact shapes
//    those files use today; a refactor that changes those shapes makes a check fail loudly rather
//    than silently pass, which is the direction it is better to be wrong in.

import { readFileSync, readdirSync, mkdirSync, writeFileSync, cpSync, rmSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(HERE, "..");

const CHECKS = [
  ["PV-1", "every message the implementation declares is published, with its tag and its plane"],
  ["PV-2", "every published example's bytes equal the bytes the implementation pins"],
  ["PV-3", "every published message round-trips through a second mesh-cbor/0 implementation"],
  ["PV-4", "every error code the implementation declares is published, with its tag and retryability"],
  ["PV-5", "every record type in the schema has published vectors, and the index agrees"],
  ["PV-6", "every published record vector round-trips through that same second implementation"],
  ["PV-7", "the coverage map in README.md names every record type and every message"],
  ["PV-8", "the versioning policy names every version identifier in the tree"],
];

// ---------------------------------------------------------------------------
// mesh-cbor/0, written from the published profile page
// ---------------------------------------------------------------------------

const MAJOR = { unsigned: 0, bytes: 2, text: 3, array: 4 };

class Writer {
  constructor() {
    this.out = [];
  }

  head(major, value) {
    const v = BigInt(value);
    const base = major << 5;
    if (v < 24n) {
      this.out.push(base | Number(v));
      return;
    }
    const width = v < 256n ? 1 : v < 65536n ? 2 : v < 4294967296n ? 4 : 8;
    this.out.push(base | { 1: 24, 2: 25, 4: 26, 8: 27 }[width]);
    for (let shift = BigInt(width - 1) * 8n; shift >= 0n; shift -= 8n) {
      this.out.push(Number((v >> shift) & 0xffn));
    }
  }

  unsigned(value) {
    this.head(MAJOR.unsigned, value);
  }

  byteString(bytes) {
    this.head(MAJOR.bytes, bytes.length);
    this.out.push(...bytes);
  }

  text(value) {
    const encoded = new TextEncoder().encode(value);
    this.head(MAJOR.text, encoded.length);
    this.out.push(...encoded);
  }

  array(length) {
    this.head(MAJOR.array, length);
  }

  boolean(value) {
    this.out.push(value ? 0xf5 : 0xf4);
  }

  raw(bytes) {
    this.out.push(...bytes);
  }

  hex() {
    return this.out.map((byte) => byte.toString(16).padStart(2, "0")).join("");
  }
}

class Reader {
  constructor(bytes) {
    this.bytes = bytes;
    this.at = 0;
  }

  byte() {
    if (this.at >= this.bytes.length) throw new Error("the bytes ended inside an item");
    this.at += 1;
    return this.bytes[this.at - 1];
  }

  head(major) {
    const first = this.byte();
    if (first >> 5 !== major) throw new Error(`expected major type ${major}, found ${first >> 5}`);
    const info = first & 0x1f;
    if (info < 24) return BigInt(info);
    const width = { 24: 1, 25: 2, 26: 4, 27: 8 }[info];
    if (!width) throw new Error("an indefinite length or a reserved head");
    let value = 0n;
    for (let index = 0; index < width; index += 1) value = (value << 8n) | BigInt(this.byte());
    const needed = value < 24n ? 0 : value < 256n ? 1 : value < 65536n ? 2 : value < 4294967296n ? 4 : 8;
    if (needed !== width) throw new Error("a head is longer than the value needs");
    return value;
  }

  unsigned() {
    return Number(this.head(MAJOR.unsigned));
  }

  byteString() {
    const length = Number(this.head(MAJOR.bytes));
    const out = this.bytes.slice(this.at, this.at + length);
    if (out.length !== length) throw new Error("the bytes ended inside a byte string");
    this.at += length;
    return out;
  }

  text() {
    const length = Number(this.head(MAJOR.text));
    const out = this.bytes.slice(this.at, this.at + length);
    if (out.length !== length) throw new Error("the bytes ended inside a text string");
    this.at += length;
    return new TextDecoder("utf-8", { fatal: true }).decode(Uint8Array.from(out));
  }

  array() {
    return Number(this.head(MAJOR.array));
  }

  arrayOf(expected) {
    const found = this.array();
    if (found !== expected) throw new Error(`expected ${expected} elements, found ${found}`);
  }

  boolean() {
    const byte = this.byte();
    if (byte === 0xf5) return true;
    if (byte === 0xf4) return false;
    throw new Error("a simple value other than true or false");
  }

  done() {
    if (this.at !== this.bytes.length) {
      throw new Error(`${this.bytes.length - this.at} bytes left after a complete item`);
    }
  }
}

const unhex = (value) => {
  if (!/^([0-9a-f]{2})*$/.test(value)) throw new Error(`not lowercase hex: ${value}`);
  return value.length === 0 ? [] : value.match(/../g).map((pair) => parseInt(pair, 16));
};
const hex = (bytes) => bytes.map((byte) => byte.toString(16).padStart(2, "0")).join("");

/** Write one field of a published schema. `record` splices a complete nested encoding in whole. */
function writeField(writer, field, value) {
  switch (field.type) {
    case "unsigned":
      writer.unsigned(value);
      return;
    case "bool":
      writer.boolean(value);
      return;
    case "text":
      writer.text(value);
      return;
    case "bytes": {
      const bytes = unhex(value);
      if (field.byte_length !== undefined && bytes.length !== field.byte_length) {
        throw new Error(`${field.name} is ${bytes.length} bytes, schema says ${field.byte_length}`);
      }
      writer.byteString(bytes);
      return;
    }
    case "record":
      writer.raw(unhex(value));
      return;
    case "sequence":
      if (field.max_elements !== undefined && value.length > field.max_elements) {
        throw new Error(`${field.name} holds ${value.length}, at most ${field.max_elements}`);
      }
      writer.array(value.length);
      for (const element of value) writeField(writer, field.element, element);
      return;
    case "group":
      writer.array(field.fields.length);
      for (const inner of field.fields) writeField(writer, inner, value[inner.name]);
      return;
    default:
      throw new Error(`no such field type: ${field.type}`);
  }
}

function readField(reader, field) {
  switch (field.type) {
    case "unsigned":
      return reader.unsigned();
    case "bool":
      return reader.boolean();
    case "text":
      return reader.text();
    case "bytes": {
      const bytes = reader.byteString();
      if (field.byte_length !== undefined && bytes.length !== field.byte_length) {
        throw new Error(`${field.name} is ${bytes.length} bytes, schema says ${field.byte_length}`);
      }
      return hex(bytes);
    }
    case "record":
      // A nested record carries its own domain tag and its own schema, and the parent schema does
      // not name which. Decoding one needs a registry of record schemas keyed by domain tag, which
      // the published schema does not yet offer — so records holding one are encode-checked only,
      // and `decodableRecord` below is what keeps that from being silent.
      throw new Error("a nested record has no schema at this position");
    case "sequence": {
      const count = reader.array();
      return Array.from({ length: count }, () => readField(reader, field.element));
    }
    case "group": {
      reader.arrayOf(field.fields.length);
      const out = {};
      for (const inner of field.fields) out[inner.name] = readField(reader, inner);
      return out;
    }
    default:
      throw new Error(`no such field type: ${field.type}`);
  }
}

/** The complete encoding of one message: the two-element frame, then the fields in schema order. */
function encodeMessage(message, value) {
  const writer = new Writer();
  writer.array(2);
  writer.unsigned(message.tag);
  writer.array(message.fields.length);
  for (const field of message.fields) writeField(writer, field, value[field.name]);
  return writer.hex();
}

function decodeMessage(message, bytesHex) {
  const reader = new Reader(unhex(bytesHex));
  reader.arrayOf(2);
  const tag = reader.unsigned();
  if (tag !== message.tag) throw new Error(`tag ${tag}, expected ${message.tag}`);
  reader.arrayOf(message.fields.length);
  const value = {};
  for (const field of message.fields) value[field.name] = readField(reader, field);
  reader.done();
  return value;
}

/** The complete encoding of one record: the domain tag, then the fields in schema order. */
function encodeRecord(domainTag, fields, record) {
  const writer = new Writer();
  writer.array(fields.length + 1);
  writer.text(domainTag);
  for (const field of fields) writeField(writer, field, record[field.name]);
  return writer.hex();
}

/** The record a domain tag and a field list encode, or a throw for a schema this file cannot read. */
function decodeRecord(domainTag, fields, bytesHex) {
  const reader = new Reader(unhex(bytesHex));
  reader.arrayOf(fields.length + 1);
  const tag = reader.text();
  if (tag !== domainTag) throw new Error(`domain tag ${tag}, expected ${domainTag}`);
  const record = {};
  for (const field of fields) record[field.name] = readField(reader, field);
  reader.done();
  return record;
}

/** Whether a field list is free of nested records, and so can be decoded positionally. */
const decodableRecord = (fields) =>
  fields.every((field) => {
    if (field.type === "record") return false;
    if (field.type === "sequence") return decodableRecord([field.element]);
    if (field.type === "group") return decodableRecord(field.fields);
    return true;
  });

/** The same message with its wire tag's head widened by one byte — bytes no encoder produces. */
function widenTag(bytesHex) {
  const bytes = unhex(bytesHex);
  if (bytes[1] >= 24) return null;
  return hex([bytes[0], 0x18, ...bytes.slice(1)]);
}

const canonicalJson = (value) => {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
};

// ---------------------------------------------------------------------------
// Readers over the implementation. Lints over text, not a parser.
// ---------------------------------------------------------------------------

const read = (root, path) => readFileSync(join(root, path), "utf8");

/** The body of the first function whose signature contains `needle`, braces balanced. */
function functionBody(source, needle) {
  const start = source.indexOf(needle);
  if (start < 0) throw new Error(`no function matching ${needle}`);
  const open = source.indexOf("{", start);
  let depth = 0;
  for (let at = open; at < source.length; at += 1) {
    if (source[at] === "{") depth += 1;
    if (source[at] === "}") {
      depth -= 1;
      if (depth === 0) return source.slice(open, at + 1);
    }
  }
  throw new Error(`unbalanced braces after ${needle}`);
}

const pairs = (body, pattern) => new Map(Array.from(body.matchAll(pattern), (m) => [m[1], m[2]]));

/** Every message the implementation declares: name, wire tag, plane. */
function declaredMessages(root) {
  // Sliced to `impl MessageKind` first: `MessagePlane` in the same file has an `as_str` of its own,
  // and reading that one instead is a mistake this file made until the check was run.
  const source = functionBody(read(root, "crates/mesh-sync-protocol/src/plane.rs"), "impl MessageKind");
  const names = pairs(functionBody(source, "fn as_str(self) -> &'static str"), /Self::(\w+) => "([A-Z_]+)"/g);
  const tags = pairs(functionBody(source, "fn tag(self) -> u64"), /Self::(\w+) => (\d+)/g);
  const planeBody = functionBody(source, "fn plane(self) -> MessagePlane");
  const planes = new Map();
  for (const arm of planeBody.matchAll(/((?:\s*\|?\s*Self::\w+)+)\s*=>\s*MessagePlane::(\w+)/g)) {
    for (const [, variant] of arm[1].matchAll(/Self::(\w+)/g)) planes.set(variant, arm[2].toLowerCase());
  }
  return Array.from(names, ([variant, name]) => ({
    name,
    tag: Number(tags.get(variant)),
    plane: planes.get(variant),
  }));
}

/** Every error code the implementation declares: name, tag, retryability. */
function declaredErrorCodes(root) {
  const source = functionBody(read(root, "crates/mesh-sync-protocol/src/error.rs"), "impl ErrorCode");
  const names = pairs(functionBody(source, "fn as_str(self) -> &'static str"), /Self::(\w+) => "([^"]+)"/g);
  const tags = pairs(functionBody(source, "fn tag(self) -> u64"), /Self::(\w+) => (\d+)/g);
  const retryableBody = functionBody(source, "fn is_retryable(self) -> bool");
  const retryable = new Set(Array.from(retryableBody.matchAll(/Self::(\w+)/g), (m) => m[1]));
  return Array.from(names, ([variant, name]) => ({
    name,
    tag: Number(tags.get(variant)),
    retryable: retryable.has(variant),
  }));
}

/** The wire bytes the implementation pins, by message name. */
function pinnedWireBytes(root) {
  const source = read(root, "crates/mesh-sync-protocol/tests/protocol-vectors.rs");
  return new Map(Array.from(source.matchAll(/\(\s*"([A-Z_]+)"\s*,\s*"([0-9a-f]*)"\s*\)/g), (m) => [m[1], m[2]]));
}

/** Every version identifier declared in the tree, as the literal a document has to name. */
function versionIdentifiers(root) {
  const sources = [
    "crates/mesh-types/src/cbor.rs",
    "crates/mesh-types/src/canonical.rs",
    "crates/mesh-types/src/vectors.rs",
    "crates/mesh-sync-protocol/src/wire.rs",
    "crates/mesh-sync-protocol/src/lib.rs",
    "crates/mesh-sync-protocol/src/summary.rs",
  ];
  const found = new Set(["PROTOCOL_VERSION"]);
  for (const path of sources) {
    for (const [, , value] of read(root, path).matchAll(/pub const (\w+): &str = "([^"]+)"/g)) {
      found.add(value);
    }
  }
  return Array.from(found).sort();
}

// ---------------------------------------------------------------------------
// The checks
// ---------------------------------------------------------------------------

const vectorFiles = (root) =>
  readdirSync(join(root, "protocol/test-vectors/v0"))
    .filter((name) => name.endsWith(".json") && name !== "index.json")
    .sort();

const loadJson = (root, path) => JSON.parse(read(root, path));

function checkMessageCoverage(published, root, report) {
  const declared = declaredMessages(root);
  const byName = new Map(published.messages.map((message) => [message.name, message]));
  for (const message of declared) {
    const found = byName.get(message.name);
    if (!found) {
      report("PV-1", `${message.name} is declared by the implementation and published nowhere`);
      continue;
    }
    if (found.tag !== message.tag) report("PV-1", `${message.name}: published tag ${found.tag}, implementation ${message.tag}`);
    if (found.plane !== message.plane) report("PV-1", `${message.name}: published plane ${found.plane}, implementation ${message.plane}`);
  }
  const declaredNames = new Set(declared.map((message) => message.name));
  for (const message of published.messages) {
    if (!declaredNames.has(message.name)) report("PV-1", `${message.name} is published and the implementation declares no such message`);
  }
}

function checkPinnedBytes(published, root, report) {
  const pinned = pinnedWireBytes(root);
  for (const message of published.messages) {
    const expected = pinned.get(message.name);
    if (expected === undefined) {
      report("PV-2", `${message.name} has no pinned bytes in the implementation's vector table`);
    } else if (expected !== message.vector.bytes_hex) {
      report("PV-2", `${message.name}: the published bytes and the pinned bytes disagree`);
    }
  }
}

function checkMessageRoundTrip(published, report) {
  for (const message of published.messages) {
    const { value, bytes_hex: bytesHex, byte_length: byteLength } = message.vector;
    try {
      const encoded = encodeMessage(message, value);
      if (encoded !== bytesHex) report("PV-3", `${message.name}: re-encoding its published value gives different bytes`);
      if (bytesHex.length / 2 !== byteLength) report("PV-3", `${message.name}: byte_length ${byteLength} is not the length of bytes_hex`);
      const decoded = decodeMessage(message, bytesHex);
      if (canonicalJson(decoded) !== canonicalJson(value)) report("PV-3", `${message.name}: decoding its published bytes gives a different value`);
    } catch (error) {
      report("PV-3", `${message.name}: ${error.message}`);
      continue;
    }
    const widened = widenTag(bytesHex);
    if (widened === null) continue;
    try {
      decodeMessage(message, widened);
      report("PV-3", `${message.name}: a widened head decoded, and no conformant encoder produces one`);
    } catch {
      // Refusing it is the point.
    }
  }
}

function checkErrorCodes(published, root, report) {
  const declared = declaredErrorCodes(root);
  const byName = new Map(published.error_codes.map((code) => [code.name, code]));
  for (const code of declared) {
    const found = byName.get(code.name);
    if (!found) {
      report("PV-4", `error code "${code.name}" is declared by the implementation and published nowhere`);
      continue;
    }
    if (found.tag !== code.tag) report("PV-4", `error code "${code.name}": published tag ${found.tag}, implementation ${code.tag}`);
    if (found.retryable !== code.retryable) report("PV-4", `error code "${code.name}": published retryable ${found.retryable}, implementation ${code.retryable}`);
  }
  const declaredNames = new Set(declared.map((code) => code.name));
  for (const code of published.error_codes) {
    if (!declaredNames.has(code.name)) report("PV-4", `error code "${code.name}" is published and the implementation declares no such code`);
  }
}

function checkRecordCoverage(schema, index, root, report) {
  const indexed = new Map(index.files.map((entry) => [entry.record, entry]));
  const onDisk = new Set(vectorFiles(root));
  for (const record of schema.records) {
    const entry = indexed.get(record.record);
    if (!entry) {
      report("PV-5", `${record.record} is in the schema and not in the vector index`);
      continue;
    }
    if (!onDisk.has(entry.file)) {
      report("PV-5", `${record.record} is indexed as ${entry.file}, which is not on disk`);
      continue;
    }
    const file = loadJson(root, `protocol/test-vectors/v0/${entry.file}`);
    if (file.record !== record.record) report("PV-5", `${entry.file} says it is about ${file.record}, the index says ${record.record}`);
    if (file.vectors.length !== entry.vector_count) report("PV-5", `${entry.file} holds ${file.vectors.length} vectors, the index says ${entry.vector_count}`);
    if (file.signed_record !== entry.signed_record) report("PV-5", `${entry.file} and the index disagree about whether ${record.record} is signed`);
  }
  const schemaTags = new Set(schema.records.map((record) => record.record));
  for (const entry of index.files) {
    if (!schemaTags.has(entry.record)) report("PV-5", `${entry.record} is indexed and the schema declares no such record`);
  }
}

function checkRecordRoundTrip(root, report) {
  let count = 0;
  for (const name of vectorFiles(root)) {
    const file = loadJson(root, `protocol/test-vectors/v0/${name}`);
    for (const vector of file.vectors) {
      count += 1;
      const label = `${file.record}/${vector.name}`;
      try {
        const encoded = encodeRecord(file.schema.domain_tag, file.schema.fields, vector.record);
        if (encoded !== vector.canonical_encoding_hex) report("PV-6", `${label}: re-encoding its published record gives different bytes`);
        if (vector.canonical_encoding_hex.length / 2 !== vector.canonical_encoding_length) {
          report("PV-6", `${label}: canonical_encoding_length is not the length of canonical_encoding_hex`);
        }
        if (decodableRecord(file.schema.fields)) {
          const decoded = decodeRecord(file.schema.domain_tag, file.schema.fields, vector.canonical_encoding_hex);
          if (canonicalJson(decoded) !== canonicalJson(vector.record)) {
            report("PV-6", `${label}: decoding its published bytes gives a different record`);
          }
        }
      } catch (error) {
        report("PV-6", `${label}: ${error.message}`);
      }
    }
  }
  return count;
}

function checkDraftCoverage(published, schema, root, report) {
  const draft = read(root, "protocol/README.md");
  for (const record of schema.records) {
    if (!draft.includes(record.record)) report("PV-7", `README.md's coverage map does not name ${record.record}`);
  }
  for (const message of published.messages) {
    if (!draft.includes(message.name)) report("PV-7", `README.md's coverage map does not name ${message.name}`);
  }
}

function checkVersioningPolicy(root, report) {
  const policy = read(root, "protocol/VERSIONING.md");
  for (const identifier of versionIdentifiers(root)) {
    if (!policy.includes(identifier)) report("PV-8", `VERSIONING.md does not name the version identifier ${identifier}`);
  }
}

/** Every finding, in check order. An empty array is the verdict "published material is sound". */
function collectFindings(root) {
  const findings = [];
  const report = (check, detail) => findings.push({ check, detail });

  const published = loadJson(root, "protocol/wire/v0/messages.json");
  const schema = loadJson(root, "protocol/schemas/canonical-encoding-v0.json");
  const index = loadJson(root, "protocol/test-vectors/v0/index.json");

  checkMessageCoverage(published, root, report);
  checkPinnedBytes(published, root, report);
  checkMessageRoundTrip(published, report);
  checkErrorCodes(published, root, report);
  checkRecordCoverage(schema, index, root, report);
  const vectorCount = checkRecordRoundTrip(root, report);
  checkDraftCoverage(published, schema, root, report);
  checkVersioningPolicy(root, report);

  return {
    findings,
    counts: {
      messages: published.messages.length,
      records: schema.records.length,
      vectors: vectorCount,
      errorCodes: published.error_codes.length,
    },
  };
}

// ---------------------------------------------------------------------------
// The self-test: every check broken on purpose
// ---------------------------------------------------------------------------

const FIXTURE_PATHS = [
  "protocol/README.md",
  "protocol/VERSIONING.md",
  "protocol/wire/v0/messages.json",
  "protocol/schemas/canonical-encoding-v0.json",
  "protocol/test-vectors/v0",
  "crates/mesh-types/src/cbor.rs",
  "crates/mesh-types/src/canonical.rs",
  "crates/mesh-types/src/vectors.rs",
  "crates/mesh-sync-protocol/src/plane.rs",
  "crates/mesh-sync-protocol/src/error.rs",
  "crates/mesh-sync-protocol/src/wire.rs",
  "crates/mesh-sync-protocol/src/lib.rs",
  "crates/mesh-sync-protocol/src/summary.rs",
  "crates/mesh-sync-protocol/tests/protocol-vectors.rs",
];

function makeFixture() {
  const root = join(tmpdir(), `mesh-verify-published-${process.pid}-${Math.random().toString(36).slice(2)}`);
  for (const path of FIXTURE_PATHS) {
    mkdirSync(join(root, dirname(path)), { recursive: true });
    cpSync(join(REPO_ROOT, path), join(root, path), { recursive: true });
  }
  return root;
}

const editJson = (root, path, mutate) => {
  const value = loadJson(root, path);
  mutate(value);
  writeFileSync(join(root, path), `${JSON.stringify(value, null, 2)}\n`);
};

const editText = (root, path, from, to) => {
  const text = read(root, path);
  if (!text.includes(from)) throw new Error(`the mutation's anchor is gone from ${path}: ${from}`);
  writeFileSync(join(root, path), text.replace(from, to));
};

const MUTATIONS = [
  {
    check: "PV-1",
    title: "a message the implementation declares is not published",
    apply: (root) => editJson(root, "protocol/wire/v0/messages.json", (doc) => {
      doc.messages = doc.messages.filter((message) => message.name !== "PRESENCE");
    }),
  },
  {
    check: "PV-2",
    title: "the implementation's pinned bytes move and the publication does not",
    apply: (root) => editText(root, "crates/mesh-sync-protocol/tests/protocol-vectors.rs", '("ACK_CHUNKS", "820a', '("ACK_CHUNKS", "820b'),
  },
  {
    check: "PV-3",
    title: "a published field value stops matching its published bytes",
    apply: (root) => editJson(root, "protocol/wire/v0/messages.json", (doc) => {
      const presence = doc.messages.find((message) => message.name === "PRESENCE");
      presence.vector.value.expires_after_millis += 1;
    }),
  },
  {
    check: "PV-4",
    title: "a published error code carries the wrong tag",
    apply: (root) => editJson(root, "protocol/wire/v0/messages.json", (doc) => {
      doc.error_codes.find((code) => code.name === "busy").tag = 99;
    }),
  },
  {
    check: "PV-5",
    title: "a record type in the schema is dropped from the vector index",
    apply: (root) => editJson(root, "protocol/test-vectors/v0/index.json", (doc) => {
      doc.files = doc.files.filter((entry) => entry.record !== "mesh.v0.changeset");
    }),
  },
  {
    check: "PV-6",
    title: "a published record stops matching its published encoding",
    apply: (root) => editJson(root, "protocol/test-vectors/v0/file-manifest.json", (doc) => {
      doc.vectors[0].record.byte_length += 1;
    }),
  },
  {
    check: "PV-7",
    title: "a message drops out of the coverage map",
    apply: (root) => editText(root, "protocol/README.md", "`ANTI_ENTROPY_SUMMARY`", "`ANTI-ENTROPY-SUMMARY`"),
  },
  {
    check: "PV-8",
    title: "the versioning policy stops naming the wire format",
    apply: (root) => editText(root, "protocol/VERSIONING.md", "mesh-cwp-wire/0", "mesh-cwp-wire/nought"),
  },
];

function selfTest() {
  const lines = [];
  let failed = false;
  const clean = makeFixture();
  try {
    const { findings } = collectFindings(clean);
    if (findings.length > 0) {
      failed = true;
      lines.push(`  FAIL baseline  the clean fixture produced ${findings.length} finding(s)`);
      for (const finding of findings) lines.push(`       ${finding.check}  ${finding.detail}`);
    } else {
      lines.push("  ok   baseline  clean fixture produces no findings");
    }
  } finally {
    rmSync(clean, { recursive: true, force: true });
  }

  for (const mutation of MUTATIONS) {
    const root = makeFixture();
    try {
      mutation.apply(root);
      const { findings } = collectFindings(root);
      const fired = findings.some((finding) => finding.check === mutation.check);
      lines.push(`  ${fired ? "ok  " : "FAIL"} ${mutation.check}      ${mutation.title}`);
      if (!fired) failed = true;
    } catch (error) {
      failed = true;
      lines.push(`  FAIL ${mutation.check}      the mutation itself failed: ${error.message}`);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }

  const covered = new Set(MUTATIONS.map((mutation) => mutation.check));
  for (const [id] of CHECKS) {
    if (!covered.has(id)) {
      failed = true;
      lines.push(`  FAIL ${id}      the check has no mutation, so nothing proves it can fail`);
    }
  }

  console.log(lines.join("\n"));
  console.log(`self-test: ${failed ? "FAIL" : "pass"} (${MUTATIONS.length} mutations over ${CHECKS.length} checks)`);
  return failed ? 1 : 0;
}

// ---------------------------------------------------------------------------

function main() {
  const argv = process.argv.slice(2);
  if (argv.includes("--help")) {
    console.log("usage: node protocol/verify-published.mjs [--self-test]");
    for (const [id, title] of CHECKS) console.log(`  ${id}  ${title}`);
    return 0;
  }
  if (argv.includes("--self-test")) return selfTest();

  const { findings, counts } = collectFindings(REPO_ROOT);
  if (findings.length > 0) {
    for (const finding of findings) console.log(`  FAIL ${finding.check}  ${finding.detail}`);
    console.log(`\nverify-published: ${findings.length} finding(s)`);
    return 1;
  }
  for (const [id, title] of CHECKS) console.log(`  ok   ${id}      ${title}`);
  console.log("\nverify-published: clean");
  console.log(
    `  ${counts.messages} messages · ${counts.records} record types · ${counts.vectors} vectors · ` +
      `${counts.errorCodes} error codes · ${CHECKS.length} checks`,
  );
  return 0;
}

process.exit(main());
