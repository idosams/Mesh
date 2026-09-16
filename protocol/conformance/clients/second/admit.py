"""The per-message preconditions, transcribed by hand from `protocol/wire/v0/messages.json`
(task 01KZC2TBX5BGTQPXXX2DATM3TY).

`preconditions` in the published message set is an array of ENGLISH SENTENCES. There is no
machine-readable predicate anywhere in `protocol/**`, so every implementer turns the same prose
into the same code by hand, and this file is that transcription for the second client. Where the
prose names an error code the code below uses it; where it does not, the code is a GUESS and the
line says so with `# UNNAMED`. That distinction is finding SPEC-4 in
`protocol/conformance/reports/second-client-r14.md`: a receiver that answers `malformed message`
and one that answers `sequence gap` for the same frame are both defensible today.

Two published preconditions are not checkable here at all, and the client says so rather than
pretending:

  * AUTHENTICATE -- "HELLO came first on this session, and named the same actor" is session state,
    and `message.admit` is a question about one message. There is no session, and no adapter
    operation to carry one.
  * UPDATE_CANONICAL_HEAD -- "the receipt is verified where its bytes are" is a forward reference
    to a record type that has no schema and no vectors.
"""

MALFORMED = "malformed message"
UNSUPPORTED_VERSION = "unsupported version"
SEQUENCE_GAP = "sequence gap"
BATCH_TOO_LARGE = "batch too large"
OFFSET_PAST_END = "offset past end"

UINT64_MAX = (1 << 64) - 1


def _refuse(code, reason):
    return {"admitted": False, "error_code": code, "reason": reason}


ADMITTED = {"admitted": True, "error_code": None, "reason": "every published precondition holds"}


def _ascending(values, what, code=MALFORMED):
    for earlier, later in zip(values, values[1:]):
        if earlier == later:
            return _refuse(code, f"{what} repeats {earlier}")
        if earlier > later:
            return _refuse(code, f"{what} is not in ascending order at {later}")
    return None


def _sparse(entry):
    """The rule ADVERTISE_FRONTIER states and ACK_OPERATIONS cites: ascending, strictly beyond."""
    sparse = entry["sparse"]
    sequences = [item["sequence"] for item in sparse]
    problem = _ascending(sequences, "a sparse set", SEQUENCE_GAP)
    if problem:
        return problem
    for sequence in sequences:
        if sequence <= entry["contiguous_through"]:
            return _refuse(
                SEQUENCE_GAP,
                f"a sparse entry at sequence {sequence} is not strictly beyond the contiguous run "
                f"through {entry['contiguous_through']}",
            )
    return None


def _hello(value, wire):
    if value["protocol_version"] != wire["protocol_version"]:
        return _refuse(
            UNSUPPORTED_VERSION,
            f"this peer speaks protocol version {wire['protocol_version']} and there is no "
            f"negotiation; the frame names {value['protocol_version']}",
        )
    if value["encoding_profile"] != wire["record_encoding_profile"]:
        return _refuse(
            UNSUPPORTED_VERSION,
            f"the record encoding profile is not negotiated either; this peer speaks "
            f"{wire['record_encoding_profile']} and the frame names {value['encoding_profile']}",
        )
    return None


def _frontier(value, _wire):
    actors = [entry["actor"] for entry in value["heads"]]
    problem = _ascending(actors, "the heads sequence")  # UNNAMED: no code published for this one
    if problem:
        return problem
    for entry in value["heads"]:
        problem = _sparse(entry)
        if problem:
            return problem
    return None


def _operations_batch(value, wire):
    carried = value["changesets"]
    if len(carried) == 0:
        return _refuse(MALFORMED, "an OPERATIONS_BATCH carries at least one ChangeSet")  # UNNAMED
    limit = wire["limits"]["max_operations_per_batch"]
    if len(carried) > limit:
        return _refuse(BATCH_TOO_LARGE, f"{len(carried)} ChangeSets; the bound is {limit}")
    for item in carried:
        if item["sequence"] == 0:
            return _refuse(MALFORMED, "a carried ChangeSet is at actor sequence zero")
        if item["body"] == "":
            return _refuse(MALFORMED, "a carried ChangeSet has an empty body")
    return None


def _chunk_batch(value, wire):
    parts = value["parts"]
    if len(parts) == 0:
        return _refuse(MALFORMED, "a CHUNK_BATCH carries at least one part")  # UNNAMED
    limit = wire["limits"]["max_chunk_part_bytes"]
    for part in parts:
        size = len(part["bytes"]) // 2
        if size == 0:
            return _refuse(MALFORMED, "a part carries no bytes")
        if size > limit:
            return _refuse(MALFORMED, f"a part of {size} bytes; the bound is {limit}")  # UNNAMED
        if part["offset"] + size > UINT64_MAX:
            return _refuse(OFFSET_PAST_END, "a part ends past the largest representable offset")
    return None


def _anti_entropy(value, _wire):
    nodes = value["nodes"]
    previous = None
    for node in nodes:
        if node["first"] > node["last"]:
            return _refuse(MALFORMED, f"the run {node['first']}..{node['last']} is empty")
        if previous is not None and node["first"] != previous["last"] + 1:
            return _refuse(
                MALFORMED,  # UNNAMED
                f"a hole between {previous['last']} and {node['first']}; the runs are contiguous",
            )
        previous = node
    return None


RULES = {
    "HELLO": _hello,
    "AUTHENTICATE": lambda value, _wire: (
        _refuse(MALFORMED, "the signature is empty") if value["signature"] == "" else None
    ),
    "ADVERTISE_FRONTIER": _frontier,
    "REQUEST_OPERATIONS": lambda value, _wire: (
        _refuse(MALFORMED, "max_count is zero")
        if value["max_count"] == 0
        else _ascending(value["specific"], "the identifier list")  # UNNAMED
    ),
    "OPERATIONS_BATCH": _operations_batch,
    "ACK_OPERATIONS": lambda value, _wire: _sparse(value),
    "ADVERTISE_MANIFESTS": lambda value, _wire: _ascending(value["manifests"], "the manifest list"),
    "REQUEST_CHUNKS": lambda value, _wire: (
        _refuse(MALFORMED, "a REQUEST_CHUNKS asks for at least one chunk")  # UNNAMED
        if len(value["requests"]) == 0
        else _ascending([item["content"] for item in value["requests"]], "the request list")
        or next(
            (
                _refuse(MALFORMED, "a request allows zero bytes")
                for item in value["requests"]
                if item["max_bytes"] == 0
            ),
            None,
        )
    ),
    "CHUNK_BATCH": _chunk_batch,
    "ACK_CHUNKS": lambda value, _wire: _ascending(value["verified"], "the acknowledgement list"),
    "UPDATE_ACTOR_HEAD": lambda value, _wire: (
        _refuse(MALFORMED, "sequence zero: an actor with no ChangeSets has no head")  # UNNAMED
        if value["sequence"] == 0
        else None
    ),
    "UPDATE_CANONICAL_HEAD": lambda _value, _wire: None,
    "PRESENCE": lambda value, wire: (
        _refuse(MALFORMED, "presence that expires immediately is not presence")  # UNNAMED
        if value["expires_after_millis"] == 0
        else (
            None
            if value["state"] in {state["tag"] for state in wire["presence_states"]}
            else _refuse(MALFORMED, f"{value['state']} names no published presence state")
        )
    ),
    "REVIEW_BUNDLE": lambda value, _wire: (
        _refuse(MALFORMED, "the body is empty") if value["body"] == "" else None
    ),
    "VALIDATION_RECEIPT": lambda value, _wire: (
        _refuse(MALFORMED, "the body is empty") if value["body"] == "" else None
    ),
    "APPROVAL_ENVELOPE": lambda value, _wire: (
        _refuse(MALFORMED, "the body is empty") if value["body"] == "" else None
    ),
    "ANTI_ENTROPY_SUMMARY": _anti_entropy,
    "ERROR": lambda _value, _wire: None,
}


def admit(name, value, wire, encode):
    """Decide one message. `encode` is the structural check: a value that cannot be encoded under
    `mesh-cbor/0` is `malformed message` whatever else holds of it."""
    rule = RULES.get(name)
    if rule is None:
        return _refuse("unknown message", f"this version defines no message named {name}")
    try:
        verdict = rule(value, wire)
    except (KeyError, TypeError) as error:
        return _refuse(MALFORMED, f"the frame does not carry the published fields: {error}")
    if verdict is not None:
        return verdict
    problem = encode()
    if problem is not None:
        return _refuse(MALFORMED, problem)
    return ADMITTED
