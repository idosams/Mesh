// protocol/conformance/clients/published/cbor.mjs — `mesh-cbor/0`, written from the published
// profile page (task 01KZC2P6SQ7E00ENWQ6F4YVXJJ).
//
// Sources, and nothing else: `protocol/test-vectors/README.md` §*The encoding, complete*,
// `protocol/schemas/canonical-encoding-v0.json` and `protocol/wire/README.md` §*The frame*. No Rust
// was read to write this file, which is the property the conformance suite exists to demonstrate.
//
// One thing here goes further than `protocol/verify-published.mjs`, which refuses a schema holding a
// nested `record` field because "the published schema does not name which record it is". It does not
// have to: the nested encoding carries its own domain tag as its first element, and the published
// schema is a registry keyed by exactly that tag. `readRecord` reads the tag and looks it up, so a
// ChangeSet round-trips its `operations` sequence here rather than being encode-checked only.

const MAJOR = { unsigned: 0, bytes: 2, text: 3, array: 4 };

/**
 * Encoder options. There is exactly one, it defaults to the conformant setting, and the published
 * client never passes anything else — it exists so `clients/broken/` can turn the keyed-sequence
 * ordering rule off from OUTSIDE this file. A mutation applied by editing the encoder would be a
 * mutation nobody could tell from a bug.
 */
export const ENCODER_DEFAULTS = Object.freeze({ sortKeyedSequences: true });

export class Writer {
  constructor() {
    this.out = [];
  }

  head(major, value) {
    const numeric = BigInt(value);
    if (numeric < 0n) throw new Error("mesh-cbor/0 admits no negative integer");
    const base = major << 5;
    if (numeric < 24n) {
      this.out.push(base | Number(numeric));
      return;
    }
    const width = numeric < 256n ? 1 : numeric < 65536n ? 2 : numeric < 4294967296n ? 4 : 8;
    this.out.push(base | { 1: 24, 2: 25, 4: 26, 8: 27 }[width]);
    for (let shift = BigInt(width - 1) * 8n; shift >= 0n; shift -= 8n) {
      this.out.push(Number((numeric >> shift) & 0xffn));
    }
  }

  unsigned(value) {
    this.head(MAJOR.unsigned, value);
  }

  byteString(bytes) {
    this.head(MAJOR.bytes, bytes.length);
    for (const byte of bytes) this.out.push(byte);
  }

  text(value) {
    const encoded = new TextEncoder().encode(value);
    this.head(MAJOR.text, encoded.length);
    for (const byte of encoded) this.out.push(byte);
  }

  array(length) {
    this.head(MAJOR.array, length);
  }

  boolean(value) {
    this.out.push(value ? 0xf5 : 0xf4);
  }

  raw(bytes) {
    for (const byte of bytes) this.out.push(byte);
  }

  hex() {
    return this.out.map((byte) => byte.toString(16).padStart(2, "0")).join("");
  }
}

export class Reader {
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
    if (first >> 5 !== major) {
      throw new Error(`expected major type ${major}, found major type ${first >> 5}`);
    }
    const info = first & 0x1f;
    if (info < 24) return BigInt(info);
    const width = { 24: 1, 25: 2, 26: 4, 27: 8 }[info];
    if (!width) throw new Error("an indefinite length or a reserved additional-information value");
    let value = 0n;
    for (let index = 0; index < width; index += 1) value = (value << 8n) | BigInt(this.byte());
    const needed =
      value < 24n ? 0 : value < 256n ? 1 : value < 65536n ? 2 : value < 4294967296n ? 4 : 8;
    if (needed !== width) throw new Error("a head is longer than the value it carries needs");
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
      throw new Error(`${this.bytes.length - this.at} byte(s) left after a complete item`);
    }
  }
}

export const unhex = (value) => {
  if (typeof value !== "string" || !/^([0-9a-f]{2})*$/.test(value)) {
    throw new Error(`not lowercase hex: ${value}`);
  }
  return value.length === 0 ? [] : value.match(/../g).map((pair) => parseInt(pair, 16));
};

export const hex = (bytes) =>
  Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");

/**
 * Whether a sequence of groups models a keyed collection, and if so which field is the key.
 * The published rule names one case today — `mesh.v0.directory-version`'s `entries`, keyed by
 * `name` — and the rule is stated in terms of "a key", so this reads the shape rather than the
 * record: a group whose first field is a text field is keyed by it.
 */
const keyFieldOf = (element) =>
  element?.type === "group" && element.fields?.[0]?.type === "text" ? element.fields[0].name : null;

const byUtf8Bytes = (left, right) => {
  const a = new TextEncoder().encode(left);
  const b = new TextEncoder().encode(right);
  const shared = Math.min(a.length, b.length);
  for (let index = 0; index < shared; index += 1) {
    if (a[index] !== b[index]) return a[index] - b[index];
  }
  return a.length - b.length;
};

export function writeField(writer, field, value, registry, options = ENCODER_DEFAULTS) {
  switch (field.type) {
    case "unsigned":
      if (!Number.isInteger(value) || value < 0) throw new Error(`${field.name} is not unsigned`);
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
        throw new Error(
          `${field.name} is ${bytes.length} bytes, the schema says ${field.byte_length}`,
        );
      }
      writer.byteString(bytes);
      return;
    }
    case "record":
      // A nested record is spliced in whole. It arrives either as its bytes or as
      // `{ domain_tag, fields }`, and both spellings must produce the same encoding.
      if (typeof value === "string") {
        writer.raw(unhex(value));
        return;
      }
      writer.raw(
        unhex(encodeRecord(value.domain_tag, value.record ?? value.fields, registry, options)),
      );
      return;
    case "sequence": {
      if (field.max_elements !== undefined && value.length > field.max_elements) {
        throw new Error(`${field.name} holds ${value.length}, at most ${field.max_elements}`);
      }
      const key = options.sortKeyedSequences ? keyFieldOf(field.element) : null;
      const elements = key
        ? [...value].sort((left, right) => byUtf8Bytes(left[key], right[key]))
        : value;
      writer.array(elements.length);
      for (const element of elements) writeField(writer, field.element, element, registry, options);
      return;
    }
    case "group":
      writer.array(field.fields.length);
      for (const inner of field.fields) {
        writeField(writer, inner, value[inner.name], registry, options);
      }
      return;
    default:
      throw new Error(`no such field type in mesh-record-schema/0: ${field.type}`);
  }
}

export function readField(reader, field, registry) {
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
        throw new Error(
          `${field.name} is ${bytes.length} bytes, the schema says ${field.byte_length}`,
        );
      }
      return hex(bytes);
    }
    case "record": {
      // A nested record is returned as the hex of the bytes it occupies, which is how the published
      // vectors spell it, so `decode(encode(r))` compares equal to `r` without a normalisation step.
      // The bytes are still fully parsed on the way past: a malformed operation is a decode failure.
      const start = reader.at;
      readRecord(reader, registry);
      return hex(reader.bytes.slice(start, reader.at));
    }
    case "sequence": {
      const count = reader.array();
      return Array.from({ length: count }, () => readField(reader, field.element, registry));
    }
    case "group": {
      reader.arrayOf(field.fields.length);
      const out = {};
      for (const inner of field.fields) out[inner.name] = readField(reader, inner, registry);
      return out;
    }
    default:
      throw new Error(`no such field type in mesh-record-schema/0: ${field.type}`);
  }
}

/** One complete record encoding: the domain tag, then the fields in schema order. */
export function encodeRecord(domainTag, record, registry, options = ENCODER_DEFAULTS) {
  const fields = registry.get(domainTag);
  if (!fields) throw new Error(`no schema published for domain tag ${domainTag}`);
  const writer = new Writer();
  writer.array(fields.length + 1);
  writer.text(domainTag);
  for (const field of fields) writeField(writer, field, record[field.name], registry, options);
  return writer.hex();
}

/** A record read from the current position, resolving its schema from its own domain tag. */
export function readRecord(reader, registry) {
  const arity = reader.array();
  const domainTag = reader.text();
  const fields = registry.get(domainTag);
  if (!fields) throw new Error(`no schema published for domain tag ${domainTag}`);
  if (arity !== fields.length + 1) {
    throw new Error(
      `${domainTag} encodes ${arity} elements, the schema says ${fields.length + 1}`,
    );
  }
  const record = {};
  for (const field of fields) record[field.name] = readField(reader, field, registry);
  return { domain_tag: domainTag, record };
}

export function decodeRecord(bytesHex, registry, expectedDomainTag) {
  const reader = new Reader(unhex(bytesHex));
  const decoded = readRecord(reader, registry);
  reader.done();
  if (expectedDomainTag !== undefined && decoded.domain_tag !== expectedDomainTag) {
    throw new Error(`domain tag ${decoded.domain_tag}, expected ${expectedDomainTag}`);
  }
  return decoded;
}

/** One complete message: the two-element frame, then the fields in the published order. */
export function encodeMessage(message, value, registry, options = ENCODER_DEFAULTS) {
  const writer = new Writer();
  writer.array(2);
  writer.unsigned(message.tag);
  writer.array(message.fields.length);
  for (const field of message.fields) {
    writeField(writer, field, value[field.name], registry, options);
  }
  return writer.hex();
}

export function decodeMessage(bytesHex, messagesByTag, registry) {
  const reader = new Reader(unhex(bytesHex));
  reader.arrayOf(2);
  const tag = reader.unsigned();
  const message = messagesByTag.get(tag);
  if (!message) throw new Error(`unknown message: no message carries wire tag ${tag}`);
  reader.arrayOf(message.fields.length);
  const value = {};
  for (const field of message.fields) value[field.name] = readField(reader, field, registry);
  reader.done();
  return { tag, name: message.name, value };
}
