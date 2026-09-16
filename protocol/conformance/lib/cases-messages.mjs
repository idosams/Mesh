// protocol/conformance/lib/cases-messages.mjs — the message half of the catalogue: message
// exchange, head advancement, delivery semantics and publication authority
// (task 01KZC2P6SQ7E00ENWQ6F4YVXJJ).
//
// `MSG-*` and `ERR-*` are generated from `protocol/wire/v0/messages.json`, so a new message or a new
// error code gets its cases the moment it is published. `HEAD-*`, `DELIV-*` and `PUB-*` are written
// against `docs/protocol.md` §2 and §7.3 and name the invariant they are about — SG, OG and TG
// identifiers — because the point of the family is that the implementer can look the rule up.
//
// The `PUB-*` family and the session-scoped members of `HEAD-*` and `DELIV-*` cannot be answered by
// anything today: no transport exists, nothing can produce a signature, and the approval envelope
// has no record schema. They are in the catalogue anyway. A conformance suite whose coverage grows
// only as the implementation grows is a suite that always says the implementation is complete.

import { several, single } from "./case.mjs";
import { admitted, deepEquals, equals, hexEquals, refused, rejected } from "./expect.mjs";

const WIRE_CITE = "protocol/wire/README.md and protocol/wire/v0/messages.json";
const SET_CITE = "docs/protocol.md §7.3 The message set";

const H32 = (byte) => byte.repeat(64);

const NOT_IMPLEMENTED = {
  tracking: null,
  question:
    "protocol/README.md §4 lists this as specified and not implemented: review bundles, approval " +
    "envelopes, publication receipts and heads-as-records have no record schema and no vectors, no " +
    "transport carries a session, and nothing in Mesh can produce a signature. No client can pass " +
    "this case from the published material, including Mesh's own. The case is published so the " +
    "gap is counted.",
};

export function messageCases(wire) {
  const cases = [];

  for (const message of wire.messages) {
    const vector = message.vector;
    cases.push(
      single({
        id: `MSG-${message.name}-encode`,
        family: "MSG",
        title: `${message.name} encodes to the published bytes`,
        rule: "a message is a two-element array: the wire tag, then the fields in published order",
        citation: `${WIRE_CITE} §The frame`,
        requires: "message.encode",
        request: { op: "message.encode", name: message.name, value: vector.value },
        check: (response) => hexEquals(response.hex, vector.bytes_hex, `the ${message.name} frame`),
      }),
      single({
        id: `MSG-${message.name}-decode`,
        family: "MSG",
        title: `${message.name} decodes back to the published field values`,
        rule: "a decoder reads the tag, and from the tag it knows the arity the body must have",
        citation: `${WIRE_CITE} §The frame`,
        requires: "message.decode",
        request: { op: "message.decode", hex: vector.bytes_hex },
        check: (response) =>
          deepEquals(
            { tag: response.tag, name: response.name, value: response.value },
            { tag: message.tag, name: message.name, value: vector.value },
            `the decoded ${message.name}`,
          ),
      }),
      single({
        id: `MSG-${message.name}-plane`,
        family: "MSG",
        title: `${message.name} is on the ${message.plane} plane`,
        rule: "a plane is a delivery guarantee; a wire tag and its plane are assigned once",
        citation: `${SET_CITE} — Three planes`,
        requires: "message.plane",
        request: { op: "message.plane", name: message.name },
        check: (response) => equals(response.plane, message.plane, `${message.name}'s plane`),
      }),
      single({
        id: `MSG-${message.name}-example-is-admissible`,
        family: "MSG",
        title: `${message.name}'s published example satisfies its own preconditions`,
        rule: `preconditions: ${message.preconditions.join(" ")}`,
        citation: `${SET_CITE} — Preconditions, checked by both sides`,
        requires: "message.admit",
        request: { op: "message.admit", name: message.name, value: vector.value },
        check: (response) => admitted(response, `${message.name}'s published example`),
      }),
    );
  }

  for (const code of wire.error_codes) {
    cases.push(
      single({
        id: `ERR-${code.name.replace(/ /g, "-")}`,
        family: "ERR",
        title: `the error code ${JSON.stringify(code.name)} carries tag ${code.tag} and is ${
          code.retryable ? "retryable" : "not retryable"
        }`,
        rule:
          "ErrorCode is closed; a sender decides what to do from the code alone, and only `busy` " +
          "and `unknown policy epoch` are retryable",
        citation: `${SET_CITE} — Error cases`,
        requires: "error.code",
        request: { op: "error.code", name: code.name },
        check: (response) =>
          equals(response.tag, code.tag, `${code.name}'s tag`) ??
          equals(response.retryable, code.retryable, `${code.name}'s retryability`),
      }),
    );
  }

  const ackChunks = wire.messages.find((message) => message.name === "ACK_CHUNKS");
  const hello = wire.messages.find((message) => message.name === "HELLO");

  cases.push(
    single({
      id: "MSG-unknown-tag-is-refused",
      family: "MSG",
      title: "a wire tag this version does not define is refused rather than guessed",
      rule: "a tag this version does not define is answered `unknown message` and never guessed",
      citation: `${SET_CITE} — The message set`,
      requires: "bytes.reject",
      request: { op: "bytes.reject", as: "message", hex: "82186380" },
      check: (response) => rejected(response, "a frame carrying wire tag 99"),
    }),
    single({
      id: "MSG-trailing-byte-is-refused",
      family: "MSG",
      title: "a complete message followed by another byte is refused",
      rule: "no trailing bytes: a complete message followed by anything at all is a decode failure",
      citation: `${WIRE_CITE} §Rules that are easy to miss`,
      requires: "bytes.reject",
      request: { op: "bytes.reject", as: "message", hex: `${ackChunks.vector.bytes_hex}00` },
      check: (response) => rejected(response, "ACK_CHUNKS followed by one extra byte"),
    }),
    single({
      id: "MSG-frame-arity-is-two",
      family: "MSG",
      title: "a frame that is not a two-element array is refused",
      rule: "a message is a two-element array: element 0 the wire tag, element 1 the body",
      citation: `${WIRE_CITE} §The frame`,
      requires: "bytes.reject",
      request: { op: "bytes.reject", as: "message", hex: `83${ackChunks.vector.bytes_hex.slice(2)}00` },
      check: (response) => rejected(response, "a three-element frame"),
    }),
    single({
      id: "MSG-identifier-width-is-thirty-two",
      family: "MSG",
      title: "an identifier of any width but thirty-two bytes is refused",
      rule: "identifiers are exactly thirty-two bytes; any other width is a decode failure",
      citation: `${WIRE_CITE} §Rules that are easy to miss`,
      requires: "bytes.reject",
      request: {
        op: "bytes.reject",
        as: "message",
        hex: hello.vector.bytes_hex.replace(`5820${H32("1")}`, `581f${H32("1").slice(0, 62)}`),
      },
      check: (response) => rejected(response, "a HELLO whose actor is thirty-one bytes"),
    }),
    single({
      id: "MSG-control-accepts-the-published-frame",
      family: "MSG",
      title: "the unmodified published frame is accepted",
      rule: "a decoder that refuses everything passes every refusal case and is not conformant",
      citation: `${WIRE_CITE} §The frame`,
      requires: "bytes.reject",
      request: { op: "bytes.reject", as: "message", hex: ackChunks.vector.bytes_hex },
      check: (response) =>
        response.rejected === false ? null : `the published frame was refused: ${response.reason}`,
    }),
    single({
      id: "MSG-hello-version-must-match",
      family: "MSG",
      title: "a HELLO naming another protocol version is refused as `unsupported version`",
      rule: "there is no negotiation: one version exists, and another is answered `unsupported version`",
      citation: "protocol/VERSIONING.md §1 The version identifiers",
      requires: "message.admit",
      request: {
        op: "message.admit",
        name: "HELLO",
        value: { ...hello.vector.value, protocol_version: wire.protocol_version + 1 },
      },
      check: (response) => refused(response, "a HELLO naming a later protocol version", "unsupported version"),
    }),
    single({
      id: "MSG-hello-profile-must-match",
      family: "MSG",
      title: "a HELLO naming another encoding profile is refused as `unsupported version`",
      rule: "the record encoding profile is not negotiated either",
      citation: "protocol/VERSIONING.md §1 The version identifiers",
      requires: "message.admit",
      request: {
        op: "message.admit",
        name: "HELLO",
        value: { ...hello.vector.value, encoding_profile: "mesh-cbor/1" },
      },
      check: (response) => refused(response, "a HELLO naming mesh-cbor/1", "unsupported version"),
    }),
    single({
      id: "MSG-authenticate-signature-non-empty",
      family: "MSG",
      title: "an AUTHENTICATE carrying no signature is refused as `malformed message`",
      rule: "signature is non-empty, else `malformed message`",
      citation: `${SET_CITE} — Preconditions`,
      requires: "message.admit",
      request: {
        op: "message.admit",
        name: "AUTHENTICATE",
        value: { actor: H32("1"), challenge: H32("2"), signature: "" },
      },
      check: (response) => refused(response, "an AUTHENTICATE with an empty signature", "malformed message"),
    }),
  );

  return cases;
}

export function headCases(wire) {
  const frontier = wire.messages.find((message) => message.name === "ADVERTISE_FRONTIER");
  const head = (actor, contiguous, sparse) => ({
    actor,
    head: H32("4"),
    contiguous_through: contiguous,
    sparse,
  });

  return [
    single({
      id: "HEAD-actor-head-sequence-zero-is-refused",
      family: "HEAD",
      title: "UPDATE_ACTOR_HEAD at sequence zero is refused",
      rule: "the sequence is not zero: an actor with no ChangeSets has no head",
      citation: `${SET_CITE} — UPDATE_ACTOR_HEAD`,
      requires: "message.admit",
      request: {
        op: "message.admit",
        name: "UPDATE_ACTOR_HEAD",
        value: { actor: H32("1"), head: H32("4"), sequence: 0 },
      },
      check: (response) => refused(response, "an actor head at sequence zero"),
    }),
    single({
      id: "HEAD-actor-head-sequence-one-is-admitted",
      family: "HEAD",
      title: "UPDATE_ACTOR_HEAD at sequence one is admitted",
      rule: "actor sequence numbers start at one and are strictly increasing (OG-2)",
      citation: "docs/protocol.md §2.2 OG-2",
      requires: "message.admit",
      request: {
        op: "message.admit",
        name: "UPDATE_ACTOR_HEAD",
        value: { actor: H32("1"), head: H32("4"), sequence: 1 },
      },
      check: (response) => admitted(response, "an actor head at sequence one"),
    }),
    single({
      id: "HEAD-frontier-entries-ascend-by-actor",
      family: "HEAD",
      title: "ADVERTISE_FRONTIER entries out of actor order are refused",
      rule: "heads ascend by actor, so two peers holding the same set send the same bytes",
      citation: `${SET_CITE} — ADVERTISE_FRONTIER`,
      requires: "message.admit",
      request: {
        op: "message.admit",
        name: "ADVERTISE_FRONTIER",
        value: {
          heads: [head(H32("6"), 1, []), head(H32("1"), 3, [])],
          canonical_head: [],
          policy_epoch: 2,
        },
      },
      check: (response) => refused(response, "a frontier whose entries descend by actor"),
    }),
    single({
      id: "HEAD-sparse-entry-below-contiguous-is-a-sequence-gap",
      family: "HEAD",
      title: "a sparse entry at or below contiguous_through is refused `sequence gap`",
      rule: "a sparse entry at or below the contiguous sequence is refused: no store can be in that state",
      citation: `${SET_CITE} — What each peer tracks about every other peer`,
      requires: "message.admit",
      request: {
        op: "message.admit",
        name: "ADVERTISE_FRONTIER",
        value: {
          heads: [head(H32("1"), 3, [{ sequence: 3, id: H32("5") }])],
          canonical_head: [],
          policy_epoch: 2,
        },
      },
      check: (response) => refused(response, "a sparse entry at the contiguous sequence", "sequence gap"),
    }),
    single({
      id: "HEAD-sparse-set-ascends",
      family: "HEAD",
      title: "a sparse set that does not ascend is refused `sequence gap`",
      rule: "each sparse set ascends by actor sequence number and lies strictly beyond the contiguous run",
      citation: `${SET_CITE} — ADVERTISE_FRONTIER`,
      requires: "message.admit",
      request: {
        op: "message.admit",
        name: "ADVERTISE_FRONTIER",
        value: {
          heads: [
            head(H32("1"), 3, [
              { sequence: 9, id: H32("5") },
              { sequence: 5, id: H32("6") },
            ]),
          ],
          canonical_head: [],
          policy_epoch: 2,
        },
      },
      check: (response) => refused(response, "a sparse set in descending order", "sequence gap"),
    }),
    several({
      id: "HEAD-absent-canonical-head-is-the-empty-sequence",
      family: "HEAD",
      title: "an absent canonical head is the empty sequence, and round-trips as one",
      rule: "the profile has no absence marker; a field that may be absent is a sequence of at most one element",
      citation: `${WIRE_CITE} §Rules that are easy to miss`,
      requires: ["message.encode", "message.decode"],
      requests: [
        {
          op: "message.encode",
          name: "ADVERTISE_FRONTIER",
          value: { ...frontier.vector.value, canonical_head: [] },
        },
        // The follow-up asks about the bytes the client just produced, which is the only way to
        // check a shape the published examples do not carry: no published frontier omits its head.
        ([encoded]) => ({ op: "message.decode", hex: encoded.hex }),
      ],
      check: ([encoded, decoded]) =>
        (encoded.hex === frontier.vector.bytes_hex
          ? "an absent canonical head encoded to the same bytes as a present one"
          : null) ??
        deepEquals(
          decoded.value,
          { ...frontier.vector.value, canonical_head: [] },
          "the frontier with no canonical head, decoded back",
        ),
    }),
    single({
      id: "HEAD-canonical-sequence-is-totally-ordered",
      family: "HEAD",
      title: "each canonical state has exactly one canonical predecessor",
      rule: "SG-7: the canonical head sequence is totally ordered",
      citation: "docs/protocol.md §2.1 SG-7",
      requires: "session",
      specGap: NOT_IMPLEMENTED,
      request: { op: "session.head", ask: "canonical-predecessor-count" },
      check: (response) => equals(response.predecessors, 1, "the canonical predecessor count"),
    }),
    single({
      id: "HEAD-resulting-head-is-checked-on-receipt",
      family: "HEAD",
      title: "a ChangeSet's resulting head is recomputed on receipt, never trusted from its author",
      rule: "OG-9: checked on construction and on receipt — never asserted by its author",
      citation: "docs/protocol.md §2.2 OG-9",
      requires: "session",
      specGap: NOT_IMPLEMENTED,
      request: { op: "session.deliver", ask: "fabricated-resulting-head" },
      check: (response) => equals(response.accepted, false, "acceptance of a fabricated resulting head"),
    }),
    single({
      id: "HEAD-advancement-never-consults-wall-clock",
      family: "HEAD",
      title: "head advancement does not consult wall-clock time",
      rule: "OG-6: hybrid logical time is metadata and a total-order tiebreak, never evidence of causality",
      citation: "docs/protocol.md §2.2 OG-6",
      requires: "session",
      specGap: NOT_IMPLEMENTED,
      request: { op: "session.deliver", ask: "backdated-hybrid-logical-time" },
      check: (response) => equals(response.head_changed, false, "the head under a backdated timestamp"),
    }),
  ];
}

export function deliveryCases(wire) {
  const carried = (sequence, body) => ({
    id: H32("a"),
    author: H32("1"),
    sequence,
    parents: [],
    base_head: H32("b"),
    resulting_head: H32("c"),
    policy_epoch: 1,
    body,
  });
  const body = "81776d6573682e76302e656d7074792d6f7065726174696f6e";
  const limit = wire.limits.max_operations_per_batch;
  const contentPlane = wire.messages
    .filter((message) => message.plane === "content")
    .map((message) => message.name);

  return [
    several({
      id: "DELIV-content-plane-holds-exactly-the-chunk-messages",
      family: "DELIV",
      title: "only REQUEST_CHUNKS, CHUNK_BATCH and ACK_CHUNKS are on the content plane",
      rule:
        "a peer learns that an object changed before the bytes of that object arrive: nothing that " +
        "carries visibility may be queued behind chunk bytes",
      citation: `${SET_CITE} — Three planes`,
      requires: "message.plane",
      requests: wire.messages.map((message) => ({ op: "message.plane", name: message.name })),
      check: (responses) => {
        const found = wire.messages
          .filter((_, index) => responses[index].plane === "content")
          .map((message) => message.name);
        return deepEquals(found.sort(), [...contentPlane].sort(), "the content plane's membership");
      },
    }),
    single({
      id: "DELIV-operations-batch-over-the-limit-is-refused",
      family: "DELIV",
      title: `an OPERATIONS_BATCH of ${limit + 1} ChangeSets is refused \`batch too large\``,
      rule: `at most MAX_OPERATIONS_PER_BATCH (${limit}), else \`batch too large\``,
      citation: `${SET_CITE} — OPERATIONS_BATCH`,
      requires: "message.admit",
      request: {
        op: "message.admit",
        name: "OPERATIONS_BATCH",
        value: {
          changesets: Array.from({ length: limit + 1 }, (_, index) => carried(index + 1, body)),
        },
      },
      check: (response) => refused(response, `a batch of ${limit + 1}`, "batch too large"),
    }),
    single({
      id: "DELIV-operations-batch-at-the-limit-is-admitted",
      family: "DELIV",
      title: `an OPERATIONS_BATCH of exactly ${limit} ChangeSets is admitted`,
      rule: `the bound is inclusive: ${limit} is admitted and ${limit + 1} is not`,
      citation: `${SET_CITE} — OPERATIONS_BATCH`,
      requires: "message.admit",
      request: {
        op: "message.admit",
        name: "OPERATIONS_BATCH",
        value: {
          changesets: Array.from({ length: limit }, (_, index) => carried(index + 1, body)),
        },
      },
      check: (response) => admitted(response, `a batch of ${limit}`),
    }),
    single({
      id: "DELIV-operations-batch-empty-is-refused",
      family: "DELIV",
      title: "an empty OPERATIONS_BATCH is refused",
      rule: "non-empty",
      citation: `${SET_CITE} — OPERATIONS_BATCH`,
      requires: "message.admit",
      request: { op: "message.admit", name: "OPERATIONS_BATCH", value: { changesets: [] } },
      check: (response) => refused(response, "an empty batch"),
    }),
    single({
      id: "DELIV-operations-batch-sequence-zero-is-refused",
      family: "DELIV",
      title: "a carried ChangeSet at actor sequence zero is refused",
      rule: "no sequence zero — per actor, sequence numbers are strictly increasing and never reused (OG-2)",
      citation: "docs/protocol.md §2.2 OG-2",
      requires: "message.admit",
      request: {
        op: "message.admit",
        name: "OPERATIONS_BATCH",
        value: { changesets: [carried(0, body)] },
      },
      check: (response) => refused(response, "a carried ChangeSet at sequence zero"),
    }),
    single({
      id: "DELIV-operations-batch-empty-body-is-refused",
      family: "DELIV",
      title: "a carried ChangeSet with an empty body is refused",
      rule: "every body non-empty: a ChangeSet travels as the bytes its author signed",
      citation: `${SET_CITE} — OPERATIONS_BATCH`,
      requires: "message.admit",
      request: {
        op: "message.admit",
        name: "OPERATIONS_BATCH",
        value: { changesets: [carried(1, "")] },
      },
      check: (response) => refused(response, "a carried ChangeSet with no body"),
    }),
    single({
      id: "DELIV-request-operations-zero-max-is-refused",
      family: "DELIV",
      title: "a REQUEST_OPERATIONS with a zero maximum is refused",
      rule: "a non-zero maximum",
      citation: `${SET_CITE} — REQUEST_OPERATIONS`,
      requires: "message.admit",
      request: {
        op: "message.admit",
        name: "REQUEST_OPERATIONS",
        value: { actor: H32("1"), from_sequence: 0, specific: [], max_count: 0 },
      },
      check: (response) => refused(response, "a request with max_count zero"),
    }),
    single({
      id: "DELIV-request-operations-duplicate-identifier-is-refused",
      family: "DELIV",
      title: "a REQUEST_OPERATIONS whose identifier list repeats is refused",
      rule: "the identifier list ascends and holds no duplicate",
      citation: `${SET_CITE} — REQUEST_OPERATIONS`,
      requires: "message.admit",
      request: {
        op: "message.admit",
        name: "REQUEST_OPERATIONS",
        value: { actor: H32("1"), from_sequence: 0, specific: [H32("a"), H32("a")], max_count: 8 },
      },
      check: (response) => refused(response, "a request naming the same ChangeSet twice"),
    }),
    single({
      id: "DELIV-ack-chunks-duplicate-is-refused",
      family: "DELIV",
      title: "an ACK_CHUNKS repeating a chunk is refused",
      rule: "ascending, no duplicate",
      citation: `${SET_CITE} — ACK_CHUNKS`,
      requires: "message.admit",
      request: { op: "message.admit", name: "ACK_CHUNKS", value: { verified: [H32("b"), H32("b")] } },
      check: (response) => refused(response, "an acknowledgement repeating a chunk"),
    }),
    single({
      id: "DELIV-advertise-manifests-descending-is-refused",
      family: "DELIV",
      title: "an ADVERTISE_MANIFESTS in descending order is refused",
      rule: "ascending, no duplicate",
      citation: `${SET_CITE} — ADVERTISE_MANIFESTS`,
      requires: "message.admit",
      request: {
        op: "message.admit",
        name: "ADVERTISE_MANIFESTS",
        value: { manifests: [H32("c"), H32("b")] },
      },
      check: (response) => refused(response, "manifests in descending order"),
    }),
    single({
      id: "DELIV-chunk-part-empty-is-refused",
      family: "DELIV",
      title: "a CHUNK_BATCH part carrying no bytes is refused",
      rule: "every part non-empty and at most MAX_CHUNK_PART_BYTES",
      citation: `${SET_CITE} — CHUNK_BATCH`,
      requires: "message.admit",
      request: {
        op: "message.admit",
        name: "CHUNK_BATCH",
        value: { parts: [{ content: H32("b"), offset: 0, bytes: "", is_final: true }] },
      },
      check: (response) => refused(response, "a part carrying no bytes"),
    }),
    single({
      id: "DELIV-chunk-part-over-the-limit-is-refused",
      family: "DELIV",
      title: `a CHUNK_BATCH part of ${wire.limits.max_chunk_part_bytes + 1} bytes is refused`,
      rule: `at most MAX_CHUNK_PART_BYTES (${wire.limits.max_chunk_part_bytes})`,
      citation: `${SET_CITE} — CHUNK_BATCH`,
      requires: "message.admit",
      request: {
        op: "message.admit",
        name: "CHUNK_BATCH",
        value: {
          parts: [
            {
              content: H32("b"),
              offset: 0,
              bytes: "00".repeat(wire.limits.max_chunk_part_bytes + 1),
              is_final: true,
            },
          ],
        },
      },
      check: (response) => refused(response, "a part one byte past the bound"),
    }),
    single({
      id: "DELIV-presence-zero-ttl-is-refused",
      family: "DELIV",
      title: "a PRESENCE with a zero time to live is refused",
      rule: "a non-zero time to live — presence is ephemeral and expires, and is not knowledge",
      citation: `${SET_CITE} — PRESENCE`,
      requires: "message.admit",
      request: {
        op: "message.admit",
        name: "PRESENCE",
        value: { actor: H32("1"), state: 1, expires_after_millis: 0 },
      },
      check: (response) => refused(response, "a presence state that expires immediately"),
    }),
    single({
      id: "DELIV-anti-entropy-summary-with-a-hole-is-refused",
      family: "DELIV",
      title: "an ANTI_ENTROPY_SUMMARY with a hole between runs is refused",
      rule: "the summary's runs are non-empty, ascending and contiguous",
      citation: `${SET_CITE} — Merkle summaries for large histories`,
      requires: "message.admit",
      request: {
        op: "message.admit",
        name: "ANTI_ENTROPY_SUMMARY",
        value: {
          actor: H32("1"),
          nodes: [
            { first: 0, last: 3, digest: H32("d") },
            { first: 5, last: 7, digest: H32("e") },
          ],
        },
      },
      check: (response) => refused(response, "a summary with a missing range"),
    }),
    single({
      id: "DELIV-anti-entropy-summary-contiguous-is-admitted",
      family: "DELIV",
      title: "an ANTI_ENTROPY_SUMMARY whose runs are contiguous is admitted",
      rule: "the depth is not fixed by the protocol; contiguity is",
      citation: `${SET_CITE} — Merkle summaries for large histories`,
      requires: "message.admit",
      request: {
        op: "message.admit",
        name: "ANTI_ENTROPY_SUMMARY",
        value: {
          actor: H32("1"),
          nodes: [
            { first: 0, last: 3, digest: H32("d") },
            { first: 4, last: 7, digest: H32("e") },
          ],
        },
      },
      check: (response) => admitted(response, "a contiguous summary"),
    }),
    single({
      id: "DELIV-reapplying-a-known-changeset-changes-nothing",
      family: "DELIV",
      title: "applying a known ChangeSet again has no additional effect",
      rule: "OG-4: verified by state hash",
      citation: "docs/protocol.md §2.2 OG-4",
      requires: "session",
      specGap: NOT_IMPLEMENTED,
      request: { op: "session.deliver", ask: "duplicate-delivery" },
      check: (response) => equals(response.state_hash_changed, false, "the state hash after a replay"),
    }),
    single({
      id: "DELIV-a-child-arriving-first-is-buffered",
      family: "DELIV",
      title: "a ChangeSet whose causal parent is missing is buffered, never dropped",
      rule: "OG-3: buffered as a known-missing dependency and never dropped or collected as invalid",
      citation: "docs/protocol.md §2.2 OG-3",
      requires: "session",
      specGap: NOT_IMPLEMENTED,
      request: { op: "session.deliver", ask: "child-before-parent" },
      check: (response) => equals(response.buffered, true, "the orphaned child"),
    }),
    single({
      id: "DELIV-any-causal-order-reaches-the-same-head",
      family: "DELIV",
      title: "two delivery orders that respect causality reach the same head",
      rule: "OG-5: convergence",
      citation: "docs/protocol.md §2.2 OG-5",
      requires: "session",
      specGap: NOT_IMPLEMENTED,
      request: { op: "session.deliver", ask: "two-causal-orders" },
      check: (response) => equals(response.heads_agree, true, "the heads from two delivery orders"),
    }),
  ];
}

export function publicationCases() {
  const declare = (id, title, rule, citation, ask, expectation) =>
    single({
      id,
      family: "PUB",
      title,
      rule,
      citation,
      requires: "publication",
      specGap: NOT_IMPLEMENTED,
      request: { op: "publication.attempt", ask },
      check: expectation,
    });

  return [
    declare(
      "PUB-approval-envelope-binds-nine-fields",
      "modifying any of the nine bound fields invalidates the approval envelope's signature",
      "TG-4: the signature covers exactly nine bound fields",
      "docs/protocol.md §2.4 TG-4 and §3.5 `approval envelope`",
      "mutate-each-bound-field",
      (response) => equals(response.invalidated, 9, "the number of bound fields whose mutation invalidated the signature"),
    ),
    declare(
      "PUB-agent-key-cannot-approve",
      "no agent-scoped key can produce a valid approval envelope",
      "TG-3: the capability is unrepresentable, not merely denied",
      "docs/protocol.md §2.4 TG-3",
      "approve-with-agent-key",
      (response) => equals(response.accepted, false, "an approval signed by an agent key"),
    ),
    declare(
      "PUB-compare-and-swap-admits-exactly-one",
      "of N concurrent attempts against one expected head, exactly one succeeds",
      "TG-9: the rest fail as stale and name the current head",
      "docs/protocol.md §2.4 TG-9",
      "concurrent-transitions",
      (response) => equals(response.winners, 1, "the number of admitted concurrent transitions"),
    ),
    declare(
      "PUB-envelope-is-single-use",
      "a replayed approval envelope is rejected",
      "TG-10: an approval envelope is single-use",
      "docs/protocol.md §2.4 TG-10",
      "replay-envelope",
      (response) => equals(response.accepted, false, "a replayed envelope"),
    ),
    declare(
      "PUB-receipt-verifies-without-mesh",
      "a publication receipt verifies from the public key and the encoding specification alone",
      "TG-6: without Mesh's own code path",
      "docs/protocol.md §2.4 TG-6",
      "verify-receipt-independently",
      (response) => equals(response.verified, true, "an independently verified receipt"),
    ),
    declare(
      "PUB-hard-guard-cannot-be-disabled",
      "a hard guard blocks publication regardless of human intent and of configuration",
      "TG-8: it cannot be disabled by configuration",
      "docs/protocol.md §2.4 TG-8",
      "disable-hard-guard",
      (response) => equals(response.disabled, false, "the hard guard after a configuration change"),
    ),
    declare(
      "PUB-policy-epochs-strictly-increase",
      "authority granted under a prior policy epoch is not valid in a later one",
      "TG-7: rotation requires no peer to be online",
      "docs/protocol.md §2.4 TG-7",
      "use-prior-epoch-capability",
      (response) => equals(response.accepted, false, "a capability from a prior epoch"),
    ),
    declare(
      "PUB-self-reported-validation-never-satisfies",
      "a result produced by the actor that produced the change never satisfies a requirement",
      "TG-5: a requirement with no independent result is reported as unvalidated, never as passing",
      "docs/protocol.md §2.4 TG-5",
      "self-reported-validation",
      (response) => equals(response.satisfied, false, "a self-reported validation result"),
    ),
  ];
}
