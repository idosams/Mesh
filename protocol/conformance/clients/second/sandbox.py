"""The independence constraint, enforced at runtime rather than promised
(task 01KZC2TBX5BGTQPXXX2DATM3TY).

The task this client exists for requires that it be built "without access to the Rust source, and
that constraint is enforced by the run's setup". A comment saying "no Rust was read" is worth
whatever the author is worth; a process that CANNOT read `crates/**` is worth what the interpreter
is worth, and the second is checkable by somebody who was not there.

`sys.addaudithook` (PEP 578) installs a hook that cannot be removed for the life of the process.
This one refuses, with `PermissionError`:

  * any file open outside `protocol/` and the Python installation -- so `crates/**`, `docs/**`,
    `tests/**` and every other path in the repository is unreadable, including through a library;
  * `protocol/test-vectors/**`, which is inside `protocol/` and is the ANSWER KEY. The reference
    client loads the published documents and deletes every `vector` from them; this one cannot open
    the record vectors at all, so every `ENC-*` answer is computed from the schema. That is the one
    place where this client's setup is strictly stronger than the reference client's, and it is the
    reason the two agreeing means something;
  * every socket operation -- the client is offline by construction, so it cannot fetch an answer;
  * every subprocess and exec -- so it cannot shell out to something that would read for it.

What it does NOT establish, stated because the gap matters: it constrains the RUNNING process, not
the author. A human who read Rust and then typed the answer into this file leaves no trace here.
Reads at authoring time are disclosed in `protocol/conformance/reports/second-client-r14.md` §2,
and the honest form of the claim is "the artefact cannot consult the reference implementation, and
the author states which files were open" -- not "independence is proven".

`python3 client.py --self-audit` proves the hook fires, and exits non-zero if it does not.
"""

import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
PROTOCOL_ROOT = os.path.realpath(os.path.join(HERE, "..", "..", ".."))
REPO_ROOT = os.path.realpath(os.path.join(PROTOCOL_ROOT, ".."))

_READABLE = (
    PROTOCOL_ROOT + os.sep,
    os.path.realpath(sys.prefix) + os.sep,
    os.path.realpath(sys.base_prefix) + os.sep,
)

_FORBIDDEN = (os.path.join(PROTOCOL_ROOT, "test-vectors") + os.sep,)

_BLOCKED_EVENTS = (
    "socket.",
    "subprocess.",
    "os.exec",
    "os.posix_spawn",
    "os.system",
    "os.fork",
    "urllib.",
    "ftplib.",
    "http.",
)


class SandboxViolation(PermissionError):
    """The client reached for something the independence constraint forbids."""


def _readable(path):
    try:
        resolved = os.path.realpath(path)
    except (TypeError, ValueError):
        return False
    if any(resolved.startswith(prefix) for prefix in _FORBIDDEN):
        return False
    return any(resolved.startswith(prefix) for prefix in _READABLE)


def _hook(event, arguments):
    if any(event.startswith(prefix) for prefix in _BLOCKED_EVENTS):
        raise SandboxViolation(
            f"the second client may not use {event}: it answers from protocol/** and nothing else"
        )
    if event in ("open", "os.open") and arguments:
        target = arguments[0]
        if isinstance(target, (str, bytes, os.PathLike)) and not _readable(target):
            raise SandboxViolation(
                f"the second client may not read {target!r}: it is outside protocol/, and the "
                "point of this client is that it was built from the published material alone"
            )


def install():
    """Install the hook. Irreversible for the life of the process, which is the point."""
    sys.addaudithook(_hook)


def self_audit():
    """Prove the hook fires. Returns a report; `client.py --self-audit` prints it and exits on it."""
    probes = [
        ("crates/mesh-types/src/lib.rs", "the reference encoder"),
        ("crates/mesh-crypto/src/lib.rs", "the reference digest and signature code"),
        ("docs/protocol.md", "the specification prose, which the AUTHOR read and the CLIENT may not"),
        ("protocol/test-vectors/v0/file-manifest.json", "the answer key for the ENC family"),
    ]
    findings = []
    for relative, why in probes:
        path = os.path.join(REPO_ROOT, relative)
        try:
            with open(path, "rb"):
                pass
            findings.append({"path": relative, "about": why, "refused": False})
        except SandboxViolation:
            findings.append({"path": relative, "about": why, "refused": True})
        except OSError as error:
            findings.append(
                {"path": relative, "about": why, "refused": False, "note": f"{type(error).__name__}"}
            )

    allowed = os.path.join(PROTOCOL_ROOT, "schemas", "canonical-encoding-v0.json")
    try:
        with open(allowed, "rb"):
            published_readable = True
    except OSError:
        published_readable = False

    return {
        "protocol_root": PROTOCOL_ROOT,
        "probes": findings,
        "published_material_readable": published_readable,
        "holds": all(item["refused"] for item in findings) and published_readable,
    }
