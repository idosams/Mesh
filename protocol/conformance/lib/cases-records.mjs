// protocol/conformance/lib/cases-records.mjs — the record-encoding half of the catalogue
// (task 01KZC2P6SQ7E00ENWQ6F4YVXJJ).
//
// Every case here is generated from `protocol/test-vectors/v0/**` and
// `protocol/schemas/canonical-encoding-v0.json`, so a new record type gets its cases the moment its
// vectors are published and cannot be forgotten. `protocol/test-vectors/README.md` §*How to use
// these vectors* lists six steps; families `-bytes`, `-length`, `-digest`, `-decode` and
// `-reject-widened-head` are those six, and the `PROF-*` family is the profile's exclusion list
// turned into refusals a decoder must make.

import { single } from "./case.mjs";
import { deepEquals, equals, hexEquals, rejected } from "./expect.mjs";

const VECTOR_CITE = "protocol/test-vectors/README.md §The encoding, complete";
const PROFILE_CITE = "protocol/schemas/canonical-encoding-v0.json §encoding_profile";

/** Widen the leading array head by one byte: `0x84` becomes `0x98 0x04`. */
function widenLeadingHead(hex) {
  const first = parseInt(hex.slice(0, 2), 16);
  const info = first & 0x1f;
  if (info >= 24) throw new Error("the leading head is already long; widening it is not defined");
  const major = first >> 5;
  return `${((major << 5) | 24).toString(16).padStart(2, "0")}${info
    .toString(16)
    .padStart(2, "0")}${hex.slice(2)}`;
}

export function recordCases({ vectors, schema }) {
  const cases = [];

  for (const file of vectors) {
    for (const vector of file.vectors) {
      const at = `${file.record}/${vector.name}`;
      const encode = { op: "record.encode", domain_tag: file.record, record: vector.record };

      cases.push(
        single({
          id: `ENC-${at}-bytes`,
          family: "ENC",
          title: `${at} encodes to the published bytes`,
          rule: "a record is a definite-length array: the domain tag, then the fields in schema order",
          citation: VECTOR_CITE,
          requires: "record.encode",
          request: encode,
          check: (response) =>
            hexEquals(response.hex, vector.canonical_encoding_hex, "the canonical encoding"),
        }),
        single({
          id: `ENC-${at}-length`,
          family: "ENC",
          title: `${at} encodes to the published length`,
          rule: "canonical_encoding_length is the byte count of canonical_encoding_hex",
          citation: "protocol/test-vectors/README.md §The vector files",
          requires: "record.encode",
          request: encode,
          check: (response) =>
            equals(
              (response.hex ?? "").length / 2,
              vector.canonical_encoding_length,
              "the encoded byte count",
            ),
        }),
        single({
          id: `ENC-${at}-digest`,
          family: "ENC",
          title: `${at} digests to the published canonical_encoding_digest_hex`,
          rule: "canonical_encoding_digest_hex is BLAKE3 of the canonical encoding",
          citation: "protocol/test-vectors/README.md §Two digests, and they are not the same thing",
          requires: "record.digest",
          request: { op: "record.digest", hex: vector.canonical_encoding_hex },
          check: (response) =>
            hexEquals(
              response.digest_hex,
              vector.canonical_encoding_digest_hex,
              "BLAKE3 of the canonical encoding",
            ),
        }),
        single({
          id: `ENC-${at}-decode`,
          family: "ENC",
          title: `${at} decodes back to the published field values`,
          rule: "decoding the canonical encoding returns the record it was made from",
          citation: "protocol/test-vectors/README.md §How to use these vectors, step 5",
          requires: "record.decode",
          request: {
            op: "record.decode",
            domain_tag: file.record,
            hex: vector.canonical_encoding_hex,
          },
          check: (response) => deepEquals(response.record, vector.record, "the decoded record"),
        }),
        single({
          id: `ENC-${at}-reject-widened-head`,
          family: "ENC",
          title: `${at} with a widened leading head is refused`,
          rule: "every head is the shortest one that holds its value; a longer spelling is refused",
          citation: "protocol/test-vectors/README.md §How to use these vectors, step 6",
          requires: "bytes.reject",
          request: {
            op: "bytes.reject",
            as: "record",
            hex: widenLeadingHead(vector.canonical_encoding_hex),
          },
          check: (response) =>
            rejected(response, "the encoding with its leading array head widened by one byte"),
        }),
      );

      if (vector.record_id_hex !== undefined) {
        cases.push(
          single({
            id: `ID-${at}`,
            family: "ID",
            title: `${at} derives the published record_id_hex`,
            rule: "a content-derived name is computed from the record's content",
            citation: "docs/protocol.md §2.1, contradicted by §3.10's DigestWriter row",
            requires: "record.id",
            specGap: {
              tracking: "01KZCZDTVD0D36W5YRGX8CNE17",
              question:
                "docs/protocol.md §2.1 says a content-derived name is computed under the canonical " +
                "encoding; §3.10's DigestWriter row says the identity framing is NOT the canonical " +
                "encoding, and the identity framing is not published as a schema. An implementation " +
                "built from the published material can verify canonical_encoding_digest_hex and " +
                "cannot recompute record_id_hex. This case exists so that gap is counted rather " +
                "than hidden; it is never removed to make a report look complete.",
            },
            request: { op: "record.id", domain_tag: file.record, record: vector.record },
            check: (response) =>
              hexEquals(response.record_id_hex, vector.record_id_hex, "the record identifier"),
          }),
        );
      }
    }
  }

  cases.push(...keyOrderCases(vectors), ...profileCases(vectors, schema));
  return cases;
}

/**
 * The one ordering rule the profile states: a sequence that models a keyed collection is in
 * ascending byte-lexicographic order of the key's UTF-8 bytes. The published vector's `record`
 * already lists the entries sorted, so re-encoding it proves nothing about sorting — the case
 * hands the client the SAME entries in reverse and demands the same bytes back.
 */
function keyOrderCases(vectors) {
  const cases = [];
  for (const file of vectors) {
    const keyed = file.schema.fields.find(
      (field) =>
        field.type === "sequence" &&
        field.element?.type === "group" &&
        field.element.fields?.[0]?.type === "text",
    );
    if (!keyed) continue;

    for (const vector of file.vectors) {
      const entries = vector.record[keyed.name];
      if (!Array.isArray(entries) || entries.length < 2) continue;
      const reversed = { ...vector.record, [keyed.name]: [...entries].reverse() };
      cases.push(
        single({
          id: `ENC-${file.record}/${vector.name}-key-order`,
          family: "ENC",
          title: `${file.record}/${vector.name} sorts ${keyed.name} by the key's UTF-8 bytes`,
          rule:
            "a sequence modelling a keyed collection is in ascending byte-lexicographic order of " +
            "the key's UTF-8 bytes — not by locale, not by code point after normalization",
          citation: "protocol/test-vectors/README.md §Ordering inside a sequence",
          requires: "record.encode",
          request: { op: "record.encode", domain_tag: file.record, record: reversed },
          check: (response) =>
            hexEquals(
              response.hex,
              vector.canonical_encoding_hex,
              `the encoding of ${keyed.name} supplied in reverse order`,
            ),
        }),
      );
    }
  }
  return cases;
}

/**
 * The profile's exclusion list, as refusals. Each case is the `mesh.v0.file-manifest` empty-file
 * vector with one substitution, so the bytes are a real record in every respect except the one the
 * case is about — a decoder that refuses them for some unrelated reason is not what is being tested.
 */
function profileCases(vectors, schema) {
  const manifest = vectors.find((file) => file.record === "mesh.v0.file-manifest");
  const empty = manifest?.vectors.find((vector) => vector.name === "empty-file");
  if (!empty) throw new Error("the mesh.v0.file-manifest empty-file vector is not published");

  const HEAD = "84";
  const TAG = "756d6573682e76302e66696c652d6d616e6966657374";
  const LENGTH = "00";
  const HASH = "5820af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262";
  const CHUNKS = "80";
  const rebuilt = `${HEAD}${TAG}${LENGTH}${HASH}${CHUNKS}`;
  if (rebuilt !== empty.canonical_encoding_hex) {
    throw new Error(
      "the mesh.v0.file-manifest empty-file vector no longer has the shape the profile cases are " +
        `built from.\n  published: ${empty.canonical_encoding_hex}\n  built:     ${rebuilt}\n` +
        "Rebuild the substitutions in lib/cases-records.mjs rather than deleting the cases.",
    );
  }
  const substitute = (lengthField, hashField = HASH, chunksField = CHUNKS) =>
    `${HEAD}${TAG}${lengthField}${hashField}${chunksField}`;

  const excluded = schema.encoding_profile.excluded.join(", ");

  const refusals = [
    [
      "PROF-map",
      "a map is refused",
      "the profile admits five shapes and a map is not one of them",
      "a1616100",
    ],
    [
      "PROF-negative-integer",
      "a negative integer is refused",
      "the profile admits unsigned integers only",
      substitute("20"),
    ],
    [
      "PROF-float",
      "a floating-point number is refused",
      "the profile admits no floating point",
      substitute("fa3f800000"),
    ],
    [
      "PROF-null",
      "null is refused",
      "the profile has no absence marker; an absent field is an empty sequence",
      substitute("f6"),
    ],
    [
      "PROF-undefined",
      "a simple value other than true and false is refused",
      "the profile admits 0xf4 and 0xf5 and no other simple value",
      substitute("f7"),
    ],
    [
      "PROF-tag",
      "a CBOR tag is refused",
      "the profile admits no tags",
      `c0${empty.canonical_encoding_hex}`,
    ],
    [
      "PROF-indefinite-length",
      "an indefinite-length array is refused",
      "the profile admits definite lengths only",
      substitute(LENGTH, HASH, "9fff"),
    ],
    [
      "PROF-non-shortest-byte-string-head",
      "a byte string whose length head is longer than it needs is refused",
      "every head is the shortest one that holds its value",
      substitute(LENGTH, `590020${HASH.slice(4)}`),
    ],
    [
      "PROF-trailing-byte",
      "a complete record followed by another byte is refused",
      "a complete item followed by anything at all is a decode failure, not an item plus noise",
      `${empty.canonical_encoding_hex}00`,
    ],
    [
      "PROF-truncated",
      "a record whose last byte is missing is refused",
      "a length that exceeds the bytes that follow it is a decode failure",
      empty.canonical_encoding_hex.slice(0, -2),
    ],
    [
      "PROF-identifier-width",
      "a fixed-width byte field of the wrong width is refused",
      "when the schema carries byte_length, exactly that many bytes",
      substitute(LENGTH, `581f${HASH.slice(4, -2)}`),
    ],
    [
      "PROF-wrong-arity",
      "a record array whose element count disagrees with the schema is refused",
      "one array element per field, and the domain tag first",
      `83${TAG}${LENGTH}${HASH}`,
    ],
  ];

  return refusals.map(([id, title, rule, hex]) =>
    single({
      id,
      family: "PROF",
      title,
      rule,
      citation: `${PROFILE_CITE} — excluded: ${excluded}`,
      requires: "bytes.reject",
      request: { op: "bytes.reject", as: "record", hex },
      check: (response) => rejected(response, "the bytes"),
    }),
  );
}

/** A control the suite needs: the unmodified vector must still decode, or every refusal is trivial. */
export function profileControlCase(vectors) {
  const manifest = vectors.find((file) => file.record === "mesh.v0.file-manifest");
  const empty = manifest.vectors.find((vector) => vector.name === "empty-file");
  return single({
    id: "PROF-control-accepts-the-published-bytes",
    family: "PROF",
    title: "the unmodified published bytes are accepted",
    rule: "a decoder that refuses everything passes every refusal case and is not conformant",
    citation: "protocol/test-vectors/README.md §How to use these vectors",
    requires: "bytes.reject",
    request: { op: "bytes.reject", as: "record", hex: empty.canonical_encoding_hex },
    check: (response) =>
      response.rejected === false
        ? null
        : `the published bytes were refused: ${response.reason}`,
  });
}
