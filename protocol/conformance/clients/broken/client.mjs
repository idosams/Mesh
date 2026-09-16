#!/usr/bin/env node
// protocol/conformance/clients/broken/client.mjs — the deliberately broken client
// (task 01KZC2P6SQ7E00ENWQ6F4YVXJJ).
//
// A conformance case only ever observed to pass is not evidence that it can fail. This client is the
// reference client with exactly one protocol violation injected, named by `MESH_CONFORMANCE_BREAK`,
// and `run.mjs --self-test` runs every mutation in `MUTATIONS` and asserts that the case each one
// is supposed to break DID fail, and that the failure named the rule.
//
// Every mutation is a violation of a published rule, not a crash. A suite that only catches clients
// that fall over catches nothing an implementer would ship.
//
//   MESH_CONFORMANCE_BREAK=long-head node protocol/conformance/clients/broken/client.mjs
//
// `run.mjs --client broken --break <name>` sets the variable for you.

import { blake3Hex } from "../published/blake3.mjs";
import { encodeRecord, unhex } from "../published/cbor.mjs";
import { CAPABILITIES, failed, handle, loadSpec, serve } from "../published/client.mjs";
import { MUTATIONS } from "./client-mutations.mjs";


const BREAK = process.env.MESH_CONFORMANCE_BREAK ?? "";
if (!Object.prototype.hasOwnProperty.call(MUTATIONS, BREAK)) {
  process.stderr.write(
    `MESH_CONFORMANCE_BREAK must name one of: ${Object.keys(MUTATIONS).join(", ")}\n`,
  );
  process.exit(2);
}

const spec = loadSpec();
let answered = 0;

const widenLeadingHead = (hex) => {
  const first = parseInt(hex.slice(0, 2), 16);
  const info = first & 0x1f;
  if (info >= 24) return hex;
  return `${((first >> 5) << 5 | 24).toString(16).padStart(2, "0")}${info
    .toString(16)
    .padStart(2, "0")}${hex.slice(2)}`;
};

const flipLastByte = (hex) =>
  `${hex.slice(0, -2)}${((parseInt(hex.slice(-2), 16) ^ 0x01) & 0xff).toString(16).padStart(2, "0")}`;

function broken(request) {
  answered += 1;
  // Not a returned value: the process leaves mid-conversation, which is what a real adapter bug
  // looks like and is graded FAIL rather than `unsupported`.
  if (BREAK === "silent-death" && answered > 3) process.exit(3);

  if (request.op === "hello") {
    const hello = handle(request, spec);
    return {
      ...hello,
      client: `broken:${BREAK}`,
      description: `The reference client with one injected violation: ${MUTATIONS[BREAK].violates}.`,
      // It declares `record.id`, which the reference client does not. A client that claims a
      // capability it cannot honour is graded on the answer it gives, not on the claim.
      capabilities: [...CAPABILITIES, "record.id"],
    };
  }

  switch (BREAK) {
    case "long-head":
      if (request.op === "record.encode" || request.op === "message.encode") {
        const answer = handle(request, spec);
        return answer.ok ? { ...answer, hex: widenLeadingHead(answer.hex) } : answer;
      }
      break;

    case "accept-widened-head":
      if (request.op === "bytes.reject") {
        const answer = handle(request, spec);
        if (answer.ok && answer.rejected && /head is longer/.test(answer.reason)) {
          return { ok: true, rejected: false, reason: "a longer head is just a longer head" };
        }
        return answer;
      }
      break;

    case "unsorted-keys":
      if (request.op === "record.encode") {
        return {
          ok: true,
          hex: encodeRecord(request.domain_tag, request.record, spec.registry, {
            sortKeyedSequences: false,
          }),
        };
      }
      break;

    case "bad-digest":
      if (request.op === "record.digest") {
        return { ok: true, digest_hex: flipLastByte(blake3Hex(unhex(request.hex))) };
      }
      break;

    case "wrong-plane":
      if (request.op === "message.plane" && request.name === "ACK_CHUNKS") {
        return { ok: true, plane: "metadata" };
      }
      break;

    case "admit-oversized-batch":
      if (request.op === "message.admit" && request.name === "OPERATIONS_BATCH") {
        return { ok: true, admitted: true, error_code: null, reason: null };
      }
      break;

    case "guess-unknown-tag":
      if (request.op === "bytes.reject" && request.as === "message") {
        const answer = handle(request, spec);
        if (answer.ok && answer.rejected && /unknown message/.test(answer.reason)) {
          return { ok: true, rejected: false, reason: "guessed it was a HELLO" };
        }
        return answer;
      }
      break;

    case "accept-trailing-bytes":
      if (request.op === "bytes.reject") {
        const answer = handle(request, spec);
        if (answer.ok && answer.rejected && /left after a complete item/.test(answer.reason)) {
          return { ok: true, rejected: false, reason: "the extra bytes were ignored" };
        }
        return answer;
      }
      break;

    case "admit-zero-sequence-head":
      if (request.op === "message.admit" && request.name === "UPDATE_ACTOR_HEAD") {
        return { ok: true, admitted: true, error_code: null, reason: null };
      }
      break;

    case "fabricate-record-id":
      if (request.op === "record.id") {
        return {
          ok: true,
          record_id_hex: blake3Hex(
            unhex(encodeRecord(request.domain_tag, request.record, spec.registry)),
          ),
        };
      }
      break;

    case "refuse-everything":
      if (request.op === "bytes.reject") {
        return { ok: true, rejected: true, reason: "no" };
      }
      break;

    case "wrong-error-tag":
      if (request.op === "error.code") {
        const answer = handle(request, spec);
        return answer.ok ? { ...answer, tag: answer.tag + 1 } : answer;
      }
      break;

    default:
      break;
  }

  return handle(request, spec);
}

if (process.env.MESH_CONFORMANCE_SERVE !== "0") {
  await serve((request) => {
    try {
      return broken(request);
    } catch (error) {
      return failed(error.message);
    }
  });
}
