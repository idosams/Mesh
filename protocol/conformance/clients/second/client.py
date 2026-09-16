#!/usr/bin/env python3
"""The SECOND CWP client: a `cwp-conformance-adapter/0` adapter in Python
(task 01KZC2TBX5BGTQPXXX2DATM3TY, research item R14).

`protocol/README.md` §1 says an outside party should be able to build a client that interoperates
with Mesh with no access to the Rust source. This client is the test of that sentence: it speaks
the adapter protocol of `protocol/conformance/README.md` §3 and answers every case in the suite
from `protocol/schemas/canonical-encoding-v0.json` and `protocol/wire/v0/messages.json` alone.

    node protocol/conformance/run.mjs --client second
    python3 protocol/conformance/clients/second/client.py --self-audit

Three things distinguish it from `clients/published/`, and they are what make running both worth
more than running either:

  1. A DIFFERENT LANGUAGE and a different author. Two implementations that share a runtime share
     its habits; an integer that JavaScript rounds, Python does not.
  2. IT CANNOT READ THE ANSWER KEY. `sandbox.py` installs an audit hook that refuses every file
     outside `protocol/` -- so `crates/**` is unreadable -- and also refuses
     `protocol/test-vectors/**`, which is where the expected bytes live. The reference client loads
     the vectors and deletes them; this one is never able to open them.
  3. IT WROTE ITS OWN BLAKE3. `blake3.py` is the algorithm from the BLAKE3 specification, in pure
     Python, so `record.digest` is an independent computation rather than a shared library call.

What it answers `unsupported` to, and why, is the finding this client exists to produce:
`record.id`, because `record_id_hex` cannot be recomputed from the published material at all
(01KZCZDTVD0D36W5YRGX8CNE17), and everything session-scoped, because no transport exists and
nothing in Mesh can produce a signature. Neither is a guess and neither is silence.
"""

import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.dont_write_bytecode = True  # leave no __pycache__ beside the source

import admit as preconditions  # noqa: E402
import meshcbor  # noqa: E402
import sandbox  # noqa: E402
from blake3 import blake3_hex  # noqa: E402

PROTOCOL_ROOT = sandbox.PROTOCOL_ROOT

IDENTITY_GAP = (
    "record_id_hex cannot be recomputed from protocol/**. docs/protocol.md §2.1 says a "
    "content-derived name is computed under the canonical encoding; §3.10's DigestWriter row says "
    "the identity framing is NOT the canonical encoding, and derive_id uses the framing. The "
    "framing is not published as a schema and has no vectors, so this client can verify "
    "canonical_encoding_digest_hex and cannot recompute record_id_hex. It answers `unsupported` "
    "rather than guessing: a fabricated identifier would be graded, and would be wrong."
)

NO_SESSION = (
    "no session exists to answer this. protocol/README.md §4 lists the transport as not "
    "implemented and signature production as not implemented anywhere, so this client cannot open "
    "a session, authenticate a peer, deliver a ChangeSet or attempt a publication. Mesh's own "
    "implementation cannot answer this case either."
)

CAPABILITIES = [
    "record.encode",
    "record.decode",
    "record.digest",
    "record.id",
    "bytes.reject",
    "message.encode",
    "message.decode",
    "message.plane",
    "message.admit",
    "error.code",
]


def load_published():
    """The two published documents this client is built from. Nothing else is opened, ever."""
    with open(os.path.join(PROTOCOL_ROOT, "schemas", "canonical-encoding-v0.json"), "rb") as handle:
        schema = json.loads(handle.read().decode("utf-8"))
    with open(os.path.join(PROTOCOL_ROOT, "wire", "v0", "messages.json"), "rb") as handle:
        wire = json.loads(handle.read().decode("utf-8"))

    # `messages[].vector` is an answer key that happens to live in the file the message set is
    # published in. The record vectors are unreadable by construction; this one is dropped by hand.
    for message in wire["messages"]:
        message.pop("vector", None)

    return {
        "wire": wire,
        "registry": {entry["schema"]["domain_tag"]: entry["schema"]["fields"] for entry in schema["records"]},
        "by_name": {message["name"]: message for message in wire["messages"]},
        "by_tag": {message["tag"]: message for message in wire["messages"]},
        "errors": {code["name"]: code for code in wire["error_codes"]},
    }


def unsupported(reason, tracking=None):
    return {"ok": False, "unsupported": True, "reason": reason, "tracking": tracking}


def failed(reason):
    return {"ok": False, "unsupported": False, "reason": reason}


def handle(request, spec):
    operation = request.get("op")
    registry = spec["registry"]

    if operation == "hello":
        return {
            "ok": True,
            "adapter": "cwp-conformance-adapter/0",
            "client": "second",
            "description": (
                "A second CWP client, in Python, from protocol/** alone: no Rust read, and the "
                "record vectors are unreadable to the process (sandbox.py)."
            ),
            "record_encoding_profile": spec["wire"]["record_encoding_profile"],
            "protocol_version": spec["wire"]["protocol_version"],
            "capabilities": CAPABILITIES,
        }

    if operation == "record.encode":
        return {"ok": True, "hex": meshcbor.encode_record(request["domain_tag"], request["record"], registry)}

    if operation == "record.decode":
        decoded = meshcbor.decode_record(request["hex"], registry, request.get("domain_tag"))
        return {"ok": True, "domain_tag": decoded["domain_tag"], "record": decoded["record"]}

    if operation == "record.digest":
        return {"ok": True, "digest_hex": blake3_hex(meshcbor.unhex(request["hex"]))}

    if operation == "record.id":
        return unsupported(IDENTITY_GAP, "01KZCZDTVD0D36W5YRGX8CNE17")

    if operation == "bytes.reject":
        try:
            if request.get("as") == "message":
                meshcbor.decode_message(request["hex"], spec["by_tag"], registry)
            else:
                meshcbor.decode_record(request["hex"], registry)
        except meshcbor.Refused as error:
            return {"ok": True, "rejected": True, "reason": str(error)}
        return {"ok": True, "rejected": False, "reason": "the bytes decoded without complaint"}

    if operation == "message.encode":
        message = spec["by_name"].get(request["name"])
        if message is None:
            return failed(f"no published message named {request['name']}")
        return {"ok": True, "hex": meshcbor.encode_message(message, request["value"], registry)}

    if operation == "message.decode":
        return {"ok": True, **meshcbor.decode_message(request["hex"], spec["by_tag"], registry)}

    if operation == "message.plane":
        message = spec["by_name"].get(request["name"])
        if message is None:
            return failed(f"no published message named {request['name']}")
        return {"ok": True, "plane": message["plane"]}

    if operation == "message.admit":
        message = spec["by_name"].get(request["name"])
        if message is None:
            return failed(f"no published message named {request['name']}")

        def structural():
            try:
                meshcbor.encode_message(message, request["value"], registry)
            except meshcbor.Refused as error:
                return str(error)
            return None

        return {"ok": True, **preconditions.admit(request["name"], request["value"], spec["wire"], structural)}

    if operation == "error.code":
        code = spec["errors"].get(request["name"])
        if code is None:
            return failed(f"no published error code named {request['name']}")
        return {"ok": True, "tag": code["tag"], "retryable": code["retryable"]}

    if operation in ("session.open", "session.deliver", "session.head", "publication.attempt"):
        return unsupported(NO_SESSION)

    return unsupported(f"this client does not implement the op {operation}")


def serve(spec):
    """One JSON object per line in, one per line out, in order. Never both on one line."""
    for line in sys.stdin:
        if line.strip() == "":
            continue
        try:
            response = handle(json.loads(line), spec)
        except meshcbor.Refused as error:
            response = failed(str(error))
        except Exception as error:  # an adapter that dies mid-conversation is a FAIL, not silence
            response = failed(f"{type(error).__name__}: {error}")
        sys.stdout.write(json.dumps(response) + "\n")
        sys.stdout.flush()


def main(argv):
    spec = load_published()  # read the published material, THEN close the door
    sandbox.install()

    if "--self-audit" in argv:
        report = sandbox.self_audit()
        sys.stdout.write(json.dumps(report, indent=2) + "\n")
        return 0 if report["holds"] else 1

    serve(spec)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
