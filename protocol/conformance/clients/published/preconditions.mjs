// protocol/conformance/clients/published/preconditions.mjs — the per-message admission rules of
// `docs/protocol.md` §7.3, implemented from the published table (task 01KZC2P6SQ7E00ENWQ6F4YVXJJ).
//
// Every rule below is one row of `messages[].preconditions` in `protocol/wire/v0/messages.json`.
// Where that table names the error code a refusal carries, this returns it; where it does not, the
// refusal carries `error_code: null` rather than a guess. Inventing a code here would be inventing
// protocol, and a conformance client is the last place that may happen.
//
// Session-scoped preconditions — "HELLO came first on this session, and named the same actor" — are
// NOT implemented and are not silently treated as satisfied. They need a session, no transport
// exists (`protocol/README.md` §4), and the suite asks about them through cases that require the
// `session` capability, which this client does not declare.

const refuse = (reason, errorCode = null) => ({
  admitted: false,
  error_code: errorCode,
  reason,
});

const ADMIT = { admitted: true, error_code: null, reason: null };

const byteLexAscending = (values, what) => {
  for (let index = 1; index < values.length; index += 1) {
    if (!(values[index - 1] < values[index])) {
      return refuse(`${what} at position ${index} does not ascend strictly past position ${index - 1}`);
    }
  }
  return null;
};

const sparseRule = (contiguousThrough, sparse) => {
  for (let index = 0; index < sparse.length; index += 1) {
    const entry = sparse[index];
    if (entry.sequence <= contiguousThrough) {
      return refuse(
        `a sparse entry at sequence ${entry.sequence} is at or below contiguous_through ` +
          `${contiguousThrough}; no store can be in that state`,
        "sequence gap",
      );
    }
    if (index > 0 && entry.sequence <= sparse[index - 1].sequence) {
      return refuse(
        `a sparse set does not ascend: ${sparse[index - 1].sequence} then ${entry.sequence}`,
        "sequence gap",
      );
    }
  }
  return null;
};

const hexByteLength = (value) => value.length / 2;

/**
 * The admission verdict for one message, from its published field values alone.
 * `spec` is the published `messages.json` document: its `protocol_version`,
 * `record_encoding_profile` and `limits` are the constants these rules are written against.
 */
export function admit(name, value, spec) {
  const limits = spec.limits;

  switch (name) {
    case "HELLO":
      if (value.protocol_version !== spec.protocol_version) {
        return refuse(
          `protocol version ${value.protocol_version}, this build speaks ${spec.protocol_version}`,
          "unsupported version",
        );
      }
      if (value.encoding_profile !== spec.record_encoding_profile) {
        return refuse(
          `encoding profile ${value.encoding_profile}, this build speaks ` +
            `${spec.record_encoding_profile}`,
          "unsupported version",
        );
      }
      return ADMIT;

    case "AUTHENTICATE":
      if (hexByteLength(value.signature) === 0) {
        return refuse("the signature is empty", "malformed message");
      }
      return ADMIT;

    case "ADVERTISE_FRONTIER": {
      const ascending = byteLexAscending(
        value.heads.map((entry) => entry.actor),
        "heads ascend by actor, and the entry",
      );
      if (ascending) return ascending;
      for (const entry of value.heads) {
        const sparse = sparseRule(entry.contiguous_through, entry.sparse);
        if (sparse) return sparse;
      }
      if (value.canonical_head.length > 1) {
        return refuse("canonical_head holds more than one element; absent is the empty sequence");
      }
      return ADMIT;
    }

    case "REQUEST_OPERATIONS": {
      if (value.max_count === 0) return refuse("max_count is zero, so no reply could satisfy it");
      const ascending = byteLexAscending(value.specific, "the specific identifier");
      if (ascending) return ascending;
      return ADMIT;
    }

    case "OPERATIONS_BATCH": {
      if (value.changesets.length === 0) return refuse("the batch is empty");
      if (value.changesets.length > limits.max_operations_per_batch) {
        return refuse(
          `the batch holds ${value.changesets.length} ChangeSets, at most ` +
            `${limits.max_operations_per_batch}`,
          "batch too large",
        );
      }
      for (const carried of value.changesets) {
        if (carried.sequence === 0) {
          return refuse("a carried ChangeSet is at actor sequence zero; sequences start at one");
        }
        if (hexByteLength(carried.body) === 0) {
          return refuse("a carried ChangeSet has an empty body");
        }
      }
      return ADMIT;
    }

    case "ACK_OPERATIONS":
      return sparseRule(value.contiguous_through, value.sparse) ?? ADMIT;

    case "ADVERTISE_MANIFESTS":
      return byteLexAscending(value.manifests, "the advertised manifest") ?? ADMIT;

    case "REQUEST_CHUNKS": {
      if (value.requests.length === 0) return refuse("the request is empty");
      const ascending = byteLexAscending(
        value.requests.map((request) => request.content),
        "the chunk request",
      );
      if (ascending) return ascending;
      for (const request of value.requests) {
        if (request.max_bytes === 0) return refuse("a chunk request has a zero byte bound");
      }
      return ADMIT;
    }

    case "CHUNK_BATCH": {
      if (value.parts.length === 0) return refuse("the batch is empty");
      for (const part of value.parts) {
        const length = hexByteLength(part.bytes);
        if (length === 0) return refuse("a chunk part carries no bytes");
        if (length > limits.max_chunk_part_bytes) {
          return refuse(
            `a chunk part carries ${length} bytes, at most ${limits.max_chunk_part_bytes}`,
          );
        }
        if (BigInt(part.offset) + BigInt(length) > 0xffffffffffffffffn) {
          return refuse("a chunk part ends past the largest representable offset");
        }
      }
      return ADMIT;
    }

    case "ACK_CHUNKS":
      return byteLexAscending(value.verified, "the verified chunk") ?? ADMIT;

    case "UPDATE_ACTOR_HEAD":
      if (value.sequence === 0) {
        return refuse("the sequence is zero; an actor with no ChangeSets has no head");
      }
      return ADMIT;

    case "UPDATE_CANONICAL_HEAD":
      return ADMIT;

    case "PRESENCE":
      if (value.expires_after_millis === 0) {
        return refuse("the time to live is zero, so the state expires before it is read");
      }
      return ADMIT;

    case "REVIEW_BUNDLE":
    case "VALIDATION_RECEIPT":
    case "APPROVAL_ENVELOPE":
      if (hexByteLength(value.body) === 0) return refuse("the body is empty");
      return ADMIT;

    case "ANTI_ENTROPY_SUMMARY": {
      const nodes = value.nodes;
      for (let index = 0; index < nodes.length; index += 1) {
        const node = nodes[index];
        if (node.last < node.first) {
          return refuse(`a summary run ends (${node.last}) before it starts (${node.first})`);
        }
        if (index > 0 && nodes[index - 1].last + 1 !== node.first) {
          return refuse(
            `a summary has a hole: the run through ${nodes[index - 1].last} is followed by one ` +
              `starting at ${node.first}, and a missing range would read as a range that agrees`,
          );
        }
      }
      return ADMIT;
    }

    case "ERROR":
      // "none: a refusal must always be sendable."
      return ADMIT;

    default:
      throw new Error(`unknown message: ${name}`);
  }
}
