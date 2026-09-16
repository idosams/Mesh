"""The `mesh-cbor/0` profile in Python: encoder and decoder, driven by the published schema
(task 01KZC2TBX5BGTQPXXX2DATM3TY).

Written from `protocol/test-vectors/README.md` (the profile page), the `encoding_profile` block of
`protocol/schemas/canonical-encoding-v0.json`, and `protocol/wire/README.md` for the frame. No Rust
was read, and `sandbox.py` makes that a runtime property rather than a promise.

Records and messages share this file because they share the profile: `protocol/wire/README.md`
says the field vocabulary is the same one the record schema uses, so one schema-driven codec serves
both.

Two places where this file makes a choice the published material does not decide. Both are in
`protocol/conformance/reports/second-client-r14.md`, and both are marked HERE as well, because a
reader of the code is the person the ambiguity costs:

  * AMBIGUITY-1 -- which sequences are "keyed collections". The ordering rule is normative and the
    schema carries no marker for it, so `_is_keyed` is a HEURISTIC: a sequence of groups whose
    first field is `text`. Today that selects exactly `mesh.v0.directory-version.entries` and
    nothing else, which is the whole published population, so it is right and unprincipled at once.
  * AMBIGUITY-2 -- whether a DECODER must refuse a keyed sequence that arrives out of order. The
    profile says the encoding IS ordered; it does not say what a decoder does with bytes that are
    not. This decoder refuses them. A conformant-looking implementation that accepts them exists,
    and nothing in the published material or the conformance suite separates the two.
  * AMBIGUITY-3 -- what a receiver does with a ChangeSet carrying an operation whose domain tag it
    has no schema for. The operation vocabulary is NOT PUBLISHED (`protocol/README.md` §6.3), so
    this is the state every real ChangeSet will be in. `mesh-cbor/0` is self-delimiting, so a
    generic decoder can skip the item and carry it opaquely; a schema-driven one cannot find where
    it ends without the schema. This decoder refuses. Both readings pass the conformance suite.

`MESH_SECOND_READING` selects the other reading of each, so the claim above is a measurement:

    MESH_SECOND_READING=key-order,unknown-operation node protocol/conformance/run.mjs --client second

scores identically to the default while ANSWERING DIFFERENT QUESTIONS ABOUT THE SAME BYTES. Two
implementations that disagree in the field and agree on the suite is what an under-specified rule
looks like from the outside.
"""

from os import environ

LENIENT = {choice for choice in environ.get("MESH_SECOND_READING", "").split(",") if choice}


def lenient(choice):
    """True when the OTHER defensible reading of an under-specified rule is selected."""
    return choice in LENIENT

MAJOR_UNSIGNED = 0
MAJOR_BYTES = 2
MAJOR_TEXT = 3
MAJOR_ARRAY = 4
MAJOR_SIMPLE = 7

FALSE_BYTE = 0xF4
TRUE_BYTE = 0xF5

UINT64_MAX = (1 << 64) - 1


class Refused(Exception):
    """The bytes, or the value, are not admissible under `mesh-cbor/0`."""


class UnknownDomainTag(Refused):
    """The bytes are well formed and name a record type this client has no published schema for."""


def unhex(text):
    if not isinstance(text, str):
        raise Refused("a byte string must arrive as a lowercase hex string")
    if len(text) % 2 != 0:
        raise Refused("a hex string has an odd number of characters")
    try:
        return bytes.fromhex(text)
    except ValueError:
        raise Refused(f"not hexadecimal: {text[:32]}") from None


# ---------------------------------------------------------------------------
# heads
# ---------------------------------------------------------------------------


def encode_head(major, argument):
    if argument < 0 or argument > UINT64_MAX:
        raise Refused(f"{argument} is outside 0 .. 2^64 - 1")
    if argument < 24:
        return bytes([(major << 5) | argument])
    for info, width in ((24, 1), (25, 2), (26, 4), (27, 8)):
        if argument < (1 << (8 * width)):
            return bytes([(major << 5) | info]) + argument.to_bytes(width, "big")
    raise Refused("unreachable: the argument did not fit eight bytes")


def read_head(data, at):
    """`(major, argument, next)` with the shortest-head rule enforced."""
    if at >= len(data):
        raise Refused("the bytes end where an item head was expected")
    first = data[at]
    major = first >> 5
    info = first & 0x1F
    if info < 24:
        return major, info, at + 1
    if info == 31:
        raise Refused("an indefinite-length item: the profile admits definite lengths only")
    if info > 27:
        raise Refused(f"additional information {info} is reserved and not admitted")
    width = 1 << (info - 24)
    end = at + 1 + width
    if end > len(data):
        raise Refused("a head's argument runs past the end of the bytes")
    argument = int.from_bytes(data[at + 1 : end], "big")
    floor = 24 if width == 1 else 1 << (8 * (width // 2))
    if argument < floor:
        raise Refused(
            f"the head for {argument} is {width + 1} bytes; every head is the shortest one that "
            "holds its value"
        )
    return major, argument, end


def _expect(major, wanted, what):
    if major != wanted:
        raise Refused(f"expected {what} (major type {wanted}) and found major type {major}")


def skip_item(data, at):
    """The end of one complete `mesh-cbor/0` item, without any schema. The profile is
    self-delimiting, which is what makes the `unknown-operation` reading of AMBIGUITY-3 possible."""
    major, argument, at = read_head(data, at)
    if major == MAJOR_UNSIGNED:
        return at
    if major in (MAJOR_BYTES, MAJOR_TEXT):
        end = at + argument
        if end > len(data):
            raise Refused("a string's length runs past the end of the bytes")
        return end
    if major == MAJOR_ARRAY:
        for _ in range(argument):
            at = skip_item(data, at)
        return at
    if major == MAJOR_SIMPLE and argument in (20, 21):
        return at
    raise Refused(f"major type {major} is not admitted by the profile")


# ---------------------------------------------------------------------------
# the field vocabulary
# ---------------------------------------------------------------------------


def _is_keyed(element):
    """AMBIGUITY-1: the schema carries no keyed-collection marker. See the module docstring."""
    return (
        isinstance(element, dict)
        and element.get("type") == "group"
        and element.get("fields")
        and element["fields"][0].get("type") == "text"
    )


def _key_of(element, value):
    name = element["fields"][0]["name"]
    if not isinstance(value.get(name), str):
        raise Refused(f"a keyed element carries no text {name}")
    return value[name].encode("utf-8")


def encode_field(field, value, registry):
    kind = field["type"]

    if kind == "unsigned":
        if isinstance(value, bool) or not isinstance(value, int):
            raise Refused(f"{field['name']} is not an unsigned integer")
        return encode_head(MAJOR_UNSIGNED, value)

    if kind == "bool":
        if not isinstance(value, bool):
            raise Refused(f"{field['name']} is not a boolean")
        return bytes([TRUE_BYTE if value else FALSE_BYTE])

    if kind == "bytes":
        raw = unhex(value)
        width = field.get("byte_length")
        if width is not None and len(raw) != width:
            raise Refused(f"{field['name']} is {len(raw)} bytes, the schema fixes {width}")
        return encode_head(MAJOR_BYTES, len(raw)) + raw

    if kind == "text":
        if not isinstance(value, str):
            raise Refused(f"{field['name']} is not a text string")
        raw = value.encode("utf-8")
        return encode_head(MAJOR_TEXT, len(raw)) + raw

    if kind == "sequence":
        if not isinstance(value, list):
            raise Refused(f"{field['name']} is not a sequence")
        limit = field.get("max_elements")
        if limit is not None and len(value) > limit:
            raise Refused(f"{field['name']} holds {len(value)} elements, at most {limit}")
        element = field["element"]
        members = value
        if _is_keyed(element):
            members = sorted(value, key=lambda item: _key_of(element, item))
        return encode_head(MAJOR_ARRAY, len(members)) + b"".join(
            encode_field(element, item, registry) for item in members
        )

    if kind == "group":
        if not isinstance(value, dict):
            raise Refused(f"{field['name']} is not a group")
        names = [inner["name"] for inner in field["fields"]]
        if sorted(names) != sorted(value.keys()):
            raise Refused(
                f"{field['name']} carries {sorted(value.keys())}, the schema fixes {sorted(names)}"
            )
        return encode_head(MAJOR_ARRAY, len(names)) + b"".join(
            encode_field(inner, value[inner["name"]], registry) for inner in field["fields"]
        )

    if kind == "record":
        # A nested record travels as the hex of its own complete encoding. That representation is
        # AMBIGUITY-3 in the report: the vector files use it and no published sentence states it.
        raw = unhex(value)
        try:
            _tag, _record, end = decode_record_at(raw, 0, registry)  # not a record: refuse
        except UnknownDomainTag:
            if not lenient("unknown-operation"):
                raise
            end = skip_item(raw, 0)
        if end != len(raw):
            raise Refused("a nested record carries trailing bytes")
        return raw

    raise Refused(f"the schema names a field type this client does not know: {kind}")


def decode_field(field, data, at, registry):
    kind = field["type"]

    if kind == "unsigned":
        major, argument, at = read_head(data, at)
        _expect(major, MAJOR_UNSIGNED, "an unsigned integer")
        return argument, at

    if kind == "bool":
        if at >= len(data):
            raise Refused("the bytes end where a boolean was expected")
        byte = data[at]
        if byte not in (FALSE_BYTE, TRUE_BYTE):
            raise Refused(
                f"0x{byte:02x} is not a boolean; the profile admits 0xf4 and 0xf5 and no other "
                "simple value"
            )
        return byte == TRUE_BYTE, at + 1

    if kind == "bytes":
        major, argument, at = read_head(data, at)
        _expect(major, MAJOR_BYTES, "a byte string")
        end = at + argument
        if end > len(data):
            raise Refused("a byte string's length runs past the end of the bytes")
        width = field.get("byte_length")
        if width is not None and argument != width:
            raise Refused(f"{field['name']} is {argument} bytes, the schema fixes {width}")
        return data[at:end].hex(), end

    if kind == "text":
        major, argument, at = read_head(data, at)
        _expect(major, MAJOR_TEXT, "a text string")
        end = at + argument
        if end > len(data):
            raise Refused("a text string's length runs past the end of the bytes")
        try:
            return data[at:end].decode("utf-8"), end
        except UnicodeDecodeError:
            raise Refused(f"{field['name']} is not valid UTF-8") from None

    if kind == "sequence":
        major, count, at = read_head(data, at)
        _expect(major, MAJOR_ARRAY, "an array")
        limit = field.get("max_elements")
        if limit is not None and count > limit:
            raise Refused(f"{field['name']} holds {count} elements, at most {limit}")
        element = field["element"]
        members = []
        for _ in range(count):
            value, at = decode_field(element, data, at, registry)
            members.append(value)
        if _is_keyed(element) and not lenient("key-order"):
            keys = [_key_of(element, item) for item in members]
            if keys != sorted(keys):
                # AMBIGUITY-2: refusing here is this client's reading, not a published rule.
                raise Refused(f"{field['name']} is not in ascending order of its key's UTF-8 bytes")
        return members, at

    if kind == "group":
        major, count, at = read_head(data, at)
        _expect(major, MAJOR_ARRAY, "an array")
        if count != len(field["fields"]):
            raise Refused(
                f"{field['name']} is an array of {count}, the schema fixes {len(field['fields'])}"
            )
        value = {}
        for inner in field["fields"]:
            value[inner["name"]], at = decode_field(inner, data, at, registry)
        return value, at

    if kind == "record":
        start = at
        try:
            _tag, _record, at = decode_record_at(data, at, registry)
        except UnknownDomainTag:
            # AMBIGUITY-3: the operation vocabulary is unpublished, so this is the state every real
            # ChangeSet is in. Refusing and carrying it opaquely are both defensible today.
            if not lenient("unknown-operation"):
                raise
            at = skip_item(data, start)
        return data[start:at].hex(), at

    raise Refused(f"the schema names a field type this client does not know: {kind}")


# ---------------------------------------------------------------------------
# records
# ---------------------------------------------------------------------------


def encode_record(domain_tag, record, registry):
    fields = registry.get(domain_tag)
    if fields is None:
        raise Refused(f"no published schema for the domain tag {domain_tag}")
    names = [field["name"] for field in fields]
    if sorted(names) != sorted(record.keys()):
        raise Refused(f"{domain_tag} carries {sorted(record.keys())}, the schema fixes {sorted(names)}")
    body = b"".join(encode_field(field, record[field["name"]], registry) for field in fields)
    tag = domain_tag.encode("utf-8")
    return (
        encode_head(MAJOR_ARRAY, len(fields) + 1)
        + encode_head(MAJOR_TEXT, len(tag))
        + tag
        + body
    ).hex()


def decode_record_at(data, at, registry):
    major, count, at = read_head(data, at)
    _expect(major, MAJOR_ARRAY, "an array")
    if count < 1:
        raise Refused("a record is an array whose first element is its domain tag")
    tag, at = decode_field({"name": "domain_tag", "type": "text"}, data, at, registry)
    fields = registry.get(tag)
    if fields is None:
        raise UnknownDomainTag(f"no published schema for the domain tag {tag}")
    if count != len(fields) + 1:
        raise Refused(
            f"{tag} is an array of {count}; the schema fixes {len(fields) + 1} "
            "(the domain tag and one element per field)"
        )
    record = {}
    for field in fields:
        record[field["name"]], at = decode_field(field, data, at, registry)
    return tag, record, at


def decode_record(hex_text, registry, expected_tag=None):
    data = unhex(hex_text)
    tag, record, at = decode_record_at(data, 0, registry)
    if at != len(data):
        raise Refused(f"{len(data) - at} byte(s) follow a complete record; trailing bytes are refused")
    if expected_tag is not None and tag != expected_tag:
        raise Refused(f"the bytes carry {tag}, {expected_tag} was expected")
    return {"domain_tag": tag, "record": record}


# ---------------------------------------------------------------------------
# messages: the same profile, framed
# ---------------------------------------------------------------------------


def encode_message(message, value, registry):
    fields = message["fields"]
    names = [field["name"] for field in fields]
    if sorted(names) != sorted(value.keys()):
        raise Refused(
            f"{message['name']} carries {sorted(value.keys())}, the published set is {sorted(names)}"
        )
    body = b"".join(encode_field(field, value[field["name"]], registry) for field in fields)
    return (
        encode_head(MAJOR_ARRAY, 2)
        + encode_head(MAJOR_UNSIGNED, message["tag"])
        + encode_head(MAJOR_ARRAY, len(fields))
        + body
    ).hex()


def decode_message(hex_text, messages_by_tag, registry):
    data = unhex(hex_text)
    major, count, at = read_head(data, 0)
    _expect(major, MAJOR_ARRAY, "an array")
    if count != 2:
        raise Refused(f"a frame is a two-element array and this one holds {count}")
    major, tag, at = read_head(data, at)
    _expect(major, MAJOR_UNSIGNED, "a wire tag")
    message = messages_by_tag.get(tag)
    if message is None:
        raise Refused(f"unknown message: this version defines no wire tag {tag}")
    major, arity, at = read_head(data, at)
    _expect(major, MAJOR_ARRAY, "the body array")
    if arity != len(message["fields"]):
        raise Refused(
            f"{message['name']}'s body holds {arity} elements; the published set fixes "
            f"{len(message['fields'])}"
        )
    value = {}
    for field in message["fields"]:
        value[field["name"]], at = decode_field(field, data, at, registry)
    if at != len(data):
        raise Refused(f"{len(data) - at} byte(s) follow a complete message; trailing bytes are refused")
    return {"tag": tag, "name": message["name"], "value": value}
