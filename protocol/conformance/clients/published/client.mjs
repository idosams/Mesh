#!/usr/bin/env node
// protocol/conformance/clients/published/client.mjs — the reference conformance client
// (task 01KZC2P6SQ7E00ENWQ6F4YVXJJ).
//
// A CWP client built from `protocol/**` alone: the profile page, the published schema, the message
// table and the versioning policy. It reads no Rust, and it is the worked example of the adapter
// contract in `protocol/conformance/README.md` §3 — an external implementer writes one of these in
// their own language and runs the same suite.
//
// **It loads the published documents and then deletes every `vector` from them.** A schema-driven
// client is the intended way to build one; a client that could see the expected bytes would be
// answering from the answer key. The deletion is at `loadSpec` and it is load-bearing.
//
// What it does NOT answer, and says so per request rather than by staying silent:
//  - `record.id` — `record_id_hex` cannot be recomputed from the published material at all.
//    `protocol/README.md` §6.1 and `protocol/test-vectors/README.md` disagree about which framing
//    names a record, and the identity framing is not published as a schema. Tracked as
//    01KZCZDTVD0D36W5YRGX8CNE17. The answer is `unsupported`, never a guess.
//  - anything session-scoped — no transport exists (`protocol/README.md` §4), so head advancement
//    across a session, delivery semantics under reordering and the whole publication sequence are
//    `unsupported` here. That is the honest answer and it is not a failure.

import { createInterface } from "node:readline";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { blake3Hex } from "./blake3.mjs";
import { admit } from "./preconditions.mjs";
import {
  decodeMessage,
  decodeRecord,
  encodeMessage,
  encodeRecord,
  unhex,
} from "./cbor.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const PROTOCOL_ROOT = join(HERE, "..", "..", "..");

export const CAPABILITIES = [
  "record.encode",
  "record.decode",
  "record.digest",
  "bytes.reject",
  "message.encode",
  "message.decode",
  "message.plane",
  "message.admit",
  "error.code",
];

export function loadSpec() {
  const schema = JSON.parse(
    readFileSync(join(PROTOCOL_ROOT, "schemas", "canonical-encoding-v0.json"), "utf8"),
  );
  const wire = JSON.parse(readFileSync(join(PROTOCOL_ROOT, "wire", "v0", "messages.json"), "utf8"));

  // The answer key is in the same files as the schema. Drop it before anything can read it.
  for (const message of wire.messages) delete message.vector;

  const registry = new Map(
    schema.records.map((entry) => [entry.schema.domain_tag, entry.schema.fields]),
  );
  const messagesByName = new Map(wire.messages.map((message) => [message.name, message]));
  const messagesByTag = new Map(wire.messages.map((message) => [message.tag, message]));
  const errorsByName = new Map(wire.error_codes.map((code) => [code.name, code]));

  return { schema, wire, registry, messagesByName, messagesByTag, errorsByName };
}

export const unsupported = (reason, tracking) => ({ ok: false, unsupported: true, reason, tracking });
export const failed = (reason) => ({ ok: false, unsupported: false, reason });

export function handle(request, spec) {
  const { registry, messagesByName, messagesByTag, errorsByName, wire } = spec;

  switch (request.op) {
    case "hello":
      return {
        ok: true,
        adapter: "cwp-conformance-adapter/0",
        client: "published",
        description: "A CWP client written from protocol/** alone, no Rust read.",
        record_encoding_profile: wire.record_encoding_profile,
        protocol_version: wire.protocol_version,
        capabilities: CAPABILITIES,
      };

    case "record.encode":
      return { ok: true, hex: encodeRecord(request.domain_tag, request.record, registry) };

    case "record.decode": {
      const decoded = decodeRecord(request.hex, registry, request.domain_tag);
      return { ok: true, domain_tag: decoded.domain_tag, record: decoded.record };
    }

    case "record.digest":
      return { ok: true, digest_hex: blake3Hex(unhex(request.hex)) };

    case "record.id":
      return unsupported(
        "record_id_hex comes from an identity framing that is not published as a schema, and " +
          "docs/protocol.md §2.1 and §3.10 disagree about whether it is the canonical encoding. " +
          "A client built from the published material cannot recompute it.",
        "01KZCZDTVD0D36W5YRGX8CNE17",
      );

    case "bytes.reject": {
      try {
        if (request.as === "message") decodeMessage(request.hex, messagesByTag, registry);
        else decodeRecord(request.hex, registry);
      } catch (error) {
        return { ok: true, rejected: true, reason: error.message };
      }
      return { ok: true, rejected: false, reason: "the bytes decoded without complaint" };
    }

    case "message.encode": {
      const message = messagesByName.get(request.name);
      if (!message) return failed(`no message named ${request.name}`);
      return { ok: true, hex: encodeMessage(message, request.value, registry) };
    }

    case "message.decode": {
      const decoded = decodeMessage(request.hex, messagesByTag, registry);
      return { ok: true, tag: decoded.tag, name: decoded.name, value: decoded.value };
    }

    case "message.plane": {
      const message = messagesByName.get(request.name);
      if (!message) return failed(`no message named ${request.name}`);
      return { ok: true, plane: message.plane };
    }

    case "message.admit": {
      const verdict = admit(request.name, request.value, wire);
      return { ok: true, ...verdict };
    }

    case "error.code": {
      const code = errorsByName.get(request.name);
      if (!code) return failed(`no error code named ${request.name}`);
      return { ok: true, tag: code.tag, retryable: code.retryable };
    }

    case "session.open":
    case "session.deliver":
    case "session.head":
    case "publication.attempt":
      return unsupported(
        "no transport exists: protocol/README.md §4 lists the transport as not implemented, and " +
          "nothing in Mesh can produce a signature, so no session can be opened or authenticated.",
        null,
      );

    default:
      return unsupported(`this client does not implement the op ${request.op}`, null);
  }
}

/**
 * The adapter loop: one JSON object per line in, one per line out, in order.
 * `answer` is the seam `clients/broken/` replaces; nothing else about serving differs.
 */
export async function serve(answer) {
  const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
  for await (const line of lines) {
    if (line.trim() === "") continue;
    let response;
    try {
      response = await answer(JSON.parse(line));
    } catch (error) {
      response = failed(error.message);
    }
    process.stdout.write(`${JSON.stringify(response)}\n`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const spec = loadSpec();
  await serve((request) => handle(request, spec));
}
