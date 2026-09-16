#!/usr/bin/env node

import { constants } from "node:fs";
import { open } from "node:fs/promises";
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";

const EXPECTED_PRODUCER_VERSION = createRequire(import.meta.url)("../../package.json").version;

const TOP_KEYS = [
  "schema",
  "producer",
  "workspace_correlation",
  "included",
  "excluded",
  "crash-diagnostics",
];
const CRASH_KEYS = [
  "section",
  "serving",
  "severity",
  "saved_records",
  "boundary_bytes",
  "unfinished_bytes",
  "checkpoint_state_available",
  "meaningful_checkpoint_through",
  "recovery_preserved_through",
  "open_activity_from",
  "open_activity_through",
  "elapsed_ms",
  "sentence",
];
const EXCLUDED = [
  "configuration",
  "event-ledger",
  "file-content",
  "key-material",
  "raw-paths",
];
const MAX_BUNDLE_BYTES = 65_536;
const MAX_SUPPORT_JOURNAL_BYTES = 64 * 1024 * 1024;
// `mesh-store` frames every record with an 8-byte prefix, a 16-byte header checksum, and a
// 16-byte body checksum. Record bodies make real frames larger; this conservative lower bound is
// enough to reject a boundary that no sequence of verified frames could occupy.
const MIN_JOURNAL_FRAME_BYTES = 40;
const RECOVERY_BUDGET_MS = 5_000;

const NOTHING_READABLE =
  "Mesh found an unfinished save in that folder and nothing finished before it, so it has not " +
  "opened the folder and has changed nothing. This folder is not empty and Mesh will not treat " +
  "it as though it were. Nothing you were told had been saved privately is missing, because " +
  "nothing there had finished being saved. This folder needs attention before it can be used.";
const UNRECOVERABLE =
  "Mesh could not open your workspace and has changed nothing. Your saved work is still on this " +
  "device. Quit Mesh and start it again; if this message comes back, the workspace needs " +
  "attention before it can be used.";

function fail(reason) {
  throw new Error(reason);
}

function skipWhitespace(text, cursor) {
  while (cursor.index < text.length && /\s/.test(text[cursor.index])) cursor.index += 1;
}

function readJsonString(text, cursor) {
  const start = cursor.index;
  cursor.index += 1;
  while (cursor.index < text.length) {
    if (text[cursor.index] === "\\") {
      cursor.index += 2;
      continue;
    }
    if (text[cursor.index] === '"') {
      cursor.index += 1;
      return JSON.parse(text.slice(start, cursor.index));
    }
    cursor.index += 1;
  }
  fail("bundle is not JSON: unterminated string");
}

function inspectJsonValue(text, cursor) {
  skipWhitespace(text, cursor);
  const token = text[cursor.index];
  if (token === '"') {
    readJsonString(text, cursor);
    return;
  }
  if (token === "{") {
    cursor.index += 1;
    skipWhitespace(text, cursor);
    const keys = new Set();
    if (text[cursor.index] === "}") {
      cursor.index += 1;
      return;
    }
    while (cursor.index < text.length) {
      const key = readJsonString(text, cursor);
      if (keys.has(key)) fail(`duplicate JSON key ${JSON.stringify(key)}`);
      keys.add(key);
      skipWhitespace(text, cursor);
      cursor.index += 1; // JSON.parse already proved this byte is `:`.
      inspectJsonValue(text, cursor);
      skipWhitespace(text, cursor);
      if (text[cursor.index] === "}") {
        cursor.index += 1;
        return;
      }
      cursor.index += 1; // JSON.parse already proved this byte is `,`.
      skipWhitespace(text, cursor);
    }
    return;
  }
  if (token === "[") {
    cursor.index += 1;
    skipWhitespace(text, cursor);
    if (text[cursor.index] === "]") {
      cursor.index += 1;
      return;
    }
    while (cursor.index < text.length) {
      inspectJsonValue(text, cursor);
      skipWhitespace(text, cursor);
      if (text[cursor.index] === "]") {
        cursor.index += 1;
        return;
      }
      cursor.index += 1; // JSON.parse already proved this byte is `,`.
    }
    return;
  }
  while (cursor.index < text.length && !/[\s,\]}]/.test(text[cursor.index])) {
    cursor.index += 1;
  }
}

function parseStrictJson(text) {
  let bundle;
  try {
    bundle = JSON.parse(text);
  } catch (error) {
    fail(`bundle is not JSON: ${error.message}`);
  }
  const cursor = { index: 0 };
  inspectJsonValue(text, cursor);
  skipWhitespace(text, cursor);
  if (cursor.index !== text.length) fail("bundle is not one JSON value");
  const canonical = JSON.stringify(bundle);
  if (text !== canonical && text !== `${canonical}\n`) {
    fail("bundle must use the canonical JSON preview encoding");
  }
  return bundle;
}

async function readBundleText(path) {
  const flags =
    constants.O_RDONLY | (constants.O_NONBLOCK ?? 0) | (constants.O_NOFOLLOW ?? 0);
  let file;
  try {
    file = await open(path, flags);
  } catch {
    fail("bundle could not be opened as a regular file");
  }

  try {
    const metadata = await file.stat();
    if (!metadata.isFile()) fail("bundle must be a regular file");
    if (metadata.size > MAX_BUNDLE_BYTES) {
      fail(`bundle exceeds the ${MAX_BUNDLE_BYTES}-byte limit`);
    }

    const bytes = Buffer.allocUnsafe(MAX_BUNDLE_BYTES + 1);
    let total = 0;
    while (total < bytes.length) {
      const read = await file.read(bytes, total, bytes.length - total, null);
      if (read.bytesRead === 0) break;
      total += read.bytesRead;
    }
    if (total > MAX_BUNDLE_BYTES) {
      fail(`bundle exceeds the ${MAX_BUNDLE_BYTES}-byte limit`);
    }
    try {
      return new TextDecoder("utf-8", { fatal: true }).decode(bytes.subarray(0, total));
    } catch {
      fail("bundle must be valid UTF-8");
    }
  } finally {
    await file.close();
  }
}

function object(value, name) {
  if (value === null || Array.isArray(value) || typeof value !== "object") {
    fail(`${name} must be an object`);
  }
  return value;
}

function exactKeys(value, expected, name) {
  const found = Object.keys(object(value, name));
  if (found.length !== expected.length || found.some((key, index) => key !== expected[index])) {
    fail(`${name} keys must be exactly ${expected.join(", ")} in that order`);
  }
}

function unsigned(value, name) {
  if (!Number.isSafeInteger(value) || value < 0) fail(`${name} must be a safe unsigned integer`);
}

const U64_MAX = 18_446_744_073_709_551_615n;

function optionalPositiveDecimal(value, name) {
  if (value === null) return null;
  if (typeof value !== "string" || !/^[1-9][0-9]{0,19}$/.test(value)) {
    fail(`${name} must be a canonical positive u64 decimal string or null`);
  }
  const parsed = BigInt(value);
  if (parsed > U64_MAX) fail(`${name} exceeds u64`);
  return parsed;
}

function exactArray(value, expected, name) {
  if (!Array.isArray(value) || value.length !== expected.length) {
    fail(`${name} must contain exactly ${expected.join(", ")}`);
  }
  for (let index = 0; index < expected.length; index += 1) {
    if (value[index] !== expected[index]) fail(`${name} is not the scanner allowlist`);
  }
}

function safeSentence(crash) {
  const records = crash.saved_records;
  const clean = `Mesh started and your workspace is up to date. ${records} saved changes were read back.`;
  const interrupted =
    `Mesh started after an unexpected shutdown. Your workspace is up to date and ${records} ` +
    "saved changes were read back. One save had not finished when the shutdown happened and " +
    "was set aside — nothing you were told had been saved privately is affected.";
  if (![clean, interrupted, NOTHING_READABLE, UNRECOVERABLE].includes(crash.sentence)) {
    fail("crash-diagnostics.sentence is outside the fixed sanitized message set");
  }
  return { clean, interrupted };
}

function consistentCrashDiagnostic(crash) {
  if ((crash.saved_records === 0) !== (crash.boundary_bytes === 0)) {
    fail("crash-diagnostics durable boundary is inconsistent");
  }
  if (
    BigInt(crash.boundary_bytes) <
    BigInt(crash.saved_records) * BigInt(MIN_JOURNAL_FRAME_BYTES)
  ) {
    fail("crash-diagnostics durable boundary cannot contain the claimed records");
  }
  if (
    BigInt(crash.boundary_bytes) + BigInt(crash.unfinished_bytes) >
    BigInt(MAX_SUPPORT_JOURNAL_BYTES)
  ) {
    fail("crash-diagnostics exceeds the producer journal inspection limit");
  }

  const { clean, interrupted } = safeSentence(crash);
  if (crash.sentence === clean) {
    const expectedSeverity = crash.elapsed_ms < RECOVERY_BUDGET_MS ? "routine" : "notable";
    if (crash.severity !== expectedSeverity) {
      fail("crash-diagnostics recovery budget verdict is inconsistent");
    }
  }
  const outcomeIsConsistent =
    (crash.sentence === clean &&
      crash.serving &&
      crash.unfinished_bytes === 0 &&
      ["routine", "notable"].includes(crash.severity)) ||
    (crash.sentence === interrupted &&
      crash.serving &&
      crash.unfinished_bytes > 0 &&
      crash.severity === "notable") ||
    (crash.sentence === NOTHING_READABLE &&
      !crash.serving &&
      crash.saved_records === 0 &&
      crash.boundary_bytes === 0 &&
      crash.unfinished_bytes > 0 &&
      crash.severity === "blocking") ||
    (crash.sentence === UNRECOVERABLE &&
      !crash.serving &&
      crash.severity === "blocking");
  if (!outcomeIsConsistent) fail("crash-diagnostics has contradictory outcome fields");
  // Non-serving reports come from open failure or read-only runtime-layout refusal. Neither path
  // has a restored RecoverySnapshot. The layout-refusal path may still retain a verified journal
  // prefix and torn tail, which are independently constrained above.
  if (!crash.serving && crash.checkpoint_state_available) {
    fail("crash-diagnostics checkpoint state cannot be available for a non-serving report");
  }

  const from = optionalPositiveDecimal(crash.open_activity_from, "open_activity_from");
  const through = optionalPositiveDecimal(crash.open_activity_through, "open_activity_through");
  if ((from === null) !== (through === null) || (from !== null && from > through)) {
    fail("crash-diagnostics open activity window is inconsistent");
  }

  // Mirror mesh_store::RecoverySnapshot's durable relationships. The scanner must not approve a
  // field-wise valid document that the producer's state decoder would reject as impossible.
  const meaningful = optionalPositiveDecimal(
    crash.meaningful_checkpoint_through,
    "meaningful_checkpoint_through",
  );
  const recovery = optionalPositiveDecimal(
    crash.recovery_preserved_through,
    "recovery_preserved_through",
  );
  if (meaningful !== null && from !== null && meaningful >= from) {
    fail("crash-diagnostics meaningful checkpoint must precede the open activity window");
  }
  // mesh_store::validate_snapshot permits latest_recovery to describe either the current open
  // window or bytes retained beneath the last meaningful checkpoint. The latter naturally occurs
  // after a meaningful close followed by new activity: opening the next window does not discard
  // the prior verified recovery pointer.
  if (recovery !== null && from !== null) {
    const belongsToCurrentWindow = recovery >= from && recovery <= through;
    const belongsToPriorMeaningful = meaningful !== null && recovery <= meaningful;
    if (!belongsToCurrentWindow && !belongsToPriorMeaningful) {
      fail(
        "crash-diagnostics recovery prefix must belong to the current window or prior meaningful state",
      );
    }
  }
  if (recovery !== null && from === null && (meaningful === null || recovery > meaningful)) {
    fail("crash-diagnostics recovery prefix without an open window exceeds meaningful state");
  }
}

/** Verify the bundle through an allowlist. Unknown data is rejected, not heuristically redacted. */
export function scan(bundle) {
  exactKeys(bundle, TOP_KEYS, "bundle");
  if (bundle.schema !== "mesh-support-bundle/v1") fail("unsupported bundle schema");

  exactKeys(bundle.producer, ["component", "version"], "producer");
  if (bundle.producer.component !== "mesh-daemon") fail("unexpected producer");
  if (bundle.producer.version !== EXPECTED_PRODUCER_VERSION) {
    fail("producer.version must exactly match the scanner version");
  }
  if (!/^blake3:[0-9a-f]{64}$/.test(bundle.workspace_correlation)) {
    fail("workspace_correlation must be one BLAKE3 digest and no raw path");
  }
  exactArray(bundle.included, ["crash-diagnostics"], "included");
  exactArray(bundle.excluded, EXCLUDED, "excluded");

  const crash = bundle["crash-diagnostics"];
  exactKeys(crash, CRASH_KEYS, "crash-diagnostics");
  if (crash.section !== "crash-diagnostics") fail("wrong crash section name");
  if (typeof crash.serving !== "boolean") fail("crash-diagnostics.serving must be boolean");
  if (!["routine", "notable", "blocking"].includes(crash.severity)) {
    fail("unknown crash severity");
  }
  for (const name of ["saved_records", "boundary_bytes", "unfinished_bytes", "elapsed_ms"]) {
    unsigned(crash[name], `crash-diagnostics.${name}`);
  }
  if (typeof crash.checkpoint_state_available !== "boolean") {
    fail("crash-diagnostics.checkpoint_state_available must be boolean");
  }
  for (const name of [
    "meaningful_checkpoint_through",
    "recovery_preserved_through",
    "open_activity_from",
    "open_activity_through",
  ]) {
    optionalPositiveDecimal(crash[name], `crash-diagnostics.${name}`);
  }
  if (!crash.checkpoint_state_available && [
    crash.meaningful_checkpoint_through,
    crash.recovery_preserved_through,
    crash.open_activity_from,
    crash.open_activity_through,
  ].some((value) => value !== null)) {
    fail("checkpoint values cannot appear when checkpoint state is unavailable");
  }
  consistentCrashDiagnostic(crash);
  return bundle;
}

async function main(arguments_) {
  if (arguments_.length !== 2 || arguments_[0] !== "--bundle") {
    fail("usage: node tools/support-bundle/scan.mjs --bundle <path>");
  }
  const text = await readBundleText(arguments_[1]);
  const bundle = parseStrictJson(text);
  scan(bundle);
  process.stdout.write(`support-bundle scan passed: ${arguments_[1]}\n`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2)).catch((error) => {
    process.stderr.write(`support-bundle scan failed: ${error.message}\n`);
    process.exitCode = 1;
  });
}
