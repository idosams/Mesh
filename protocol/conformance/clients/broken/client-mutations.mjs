// protocol/conformance/clients/broken/client-mutations.mjs — the injected violations, and the case
// that must catch each one (task 01KZC2P6SQ7E00ENWQ6F4YVXJJ).
//
// This table is the mutation test. `run.mjs --self-test` runs the broken client once per entry and
// asserts that the case named in `caught_by` FAILED — not merely that something failed, because
// "something failed" is satisfied by a suite that fails everything. Adding a mutation whose
// `caught_by` case does not exist, or does not fail, turns the self-test red.
//
// It lives apart from `client.mjs` so the runner can read it without spawning the client, and so
// the client can refuse to start when `MESH_CONFORMANCE_BREAK` names nothing here.

export const MUTATIONS = {
  "long-head": {
    violates: "every head is the shortest one that holds its value",
    caught_by: "ENC-mesh.v0.file-manifest/empty-file-bytes",
  },
  "accept-widened-head": {
    violates: "a conformant decoder rejects the longer spellings",
    caught_by: "ENC-mesh.v0.file-manifest/empty-file-reject-widened-head",
  },
  "unsorted-keys": {
    violates: "a keyed sequence ascends by the key's UTF-8 bytes",
    caught_by: "ENC-mesh.v0.directory-version/three-entries-sorted-by-utf8-bytes-key-order",
  },
  "bad-digest": {
    violates: "canonical_encoding_digest_hex is BLAKE3 of the canonical encoding",
    caught_by: "ENC-mesh.v0.file-manifest/empty-file-digest",
  },
  "wrong-plane": {
    violates: "only the three chunk messages are on the content plane",
    caught_by: "DELIV-content-plane-holds-exactly-the-chunk-messages",
  },
  "admit-oversized-batch": {
    violates: "at most MAX_OPERATIONS_PER_BATCH, else `batch too large`",
    caught_by: "DELIV-operations-batch-over-the-limit-is-refused",
  },
  "guess-unknown-tag": {
    violates: "a tag this version does not define is refused rather than guessed",
    caught_by: "MSG-unknown-tag-is-refused",
  },
  "accept-trailing-bytes": {
    violates: "a complete item followed by anything at all is a decode failure",
    caught_by: "PROF-trailing-byte",
  },
  "admit-zero-sequence-head": {
    violates: "an actor with no ChangeSets has no head",
    caught_by: "HEAD-actor-head-sequence-zero-is-refused",
  },
  "fabricate-record-id": {
    violates: "a value that cannot be derived from the published material is not invented",
    caught_by: "ID-mesh.v0.file-manifest/empty-file",
  },
  "refuse-everything": {
    violates: "a decoder that refuses everything is not conformant",
    caught_by: "PROF-control-accepts-the-published-bytes",
  },
  "wrong-error-tag": {
    violates: "ErrorCode is closed and each code's tag is fixed",
    caught_by: "ERR-busy",
  },
  "silent-death": {
    violates: "cwp-conformance-adapter/0: one response per request, in order",
    caught_by: "ENC-mesh.v0.file-manifest/single-chunk-bytes",
  },
};
