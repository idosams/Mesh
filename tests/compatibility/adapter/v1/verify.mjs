#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../../../..");
const read = (path) => readFileSync(join(root, path), "utf8");
const json = (path) => JSON.parse(read(path));
const clone = (value) => structuredClone(value);

const FIELDS = [
  "source_before.parent",
  "source_before.name",
  "source_before.object",
  "destination_before.state",
  "destination_before.object",
  "destination_after.parent",
  "destination_after.name",
  "destination_after.object",
];
const REASONS = ["Closed", "Synced", "RenamedIntoPlace"];
const PRODUCERS = [
  ["Closed", "mesh-fuse/last-modified-handle-closed"],
  ["Synced", "mesh-fuse/successful-fsync"],
  ["RenamedIntoPlace", "mesh-fuse/atomic-replace-with-binding-evidence"],
];

class Failure extends Error {
  constructor(code, message) {
    super(message);
    this.code = code;
  }
}
const fail = (code, message) => { throw new Failure(code, message); };
function equal(actual, expected, code, message) {
  try { assert.deepEqual(actual, expected); } catch { fail(code, message); }
}

function variants(source, enumeration) {
  const header = `pub enum ${enumeration} {`;
  const at = source.indexOf(header);
  if (at < 0) fail("rust-vocabulary", `${enumeration} is not a plain public enum`);
  const found = [];
  let depth = 0;
  for (const line of source.slice(at + header.length).split("\n")) {
    const trimmed = line.trim();
    if (depth === 0 && trimmed.startsWith("}")) break;
    if (depth === 0 && trimmed && !trimmed.startsWith("//") && !trimmed.startsWith("#")) {
      const name = trimmed.match(/^[A-Z][A-Za-z0-9_]*/)?.[0];
      if (name) found.push(name);
    }
    depth += (line.match(/{/g) ?? []).length;
    depth -= (line.match(/}/g) ?? []).length;
  }
  return found;
}

function validate(v1, compatibility, { source = true } = {}) {
  equal(Object.keys(v1), [
    "contract", "extends", "design", "task", "capabilities", "errors", "event_kinds",
    "rename_dispositions", "rename_evidence", "identity", "boundary_reasons",
    "boundary_observations", "producers", "ordering", "compatibility",
  ], "closed-document", "v1 vocabulary keys changed");
  equal(v1.contract, "mesh-workspace-adapter/1", "contract", "wrong v1 token");
  equal(v1.extends, "mesh-workspace-adapter/0", "contract", "wrong source contract");
  equal(v1.design, "01M0E2ZW2VPM66R1FZXQRR2CV5", "contract", "wrong design");
  equal(v1.task, "01KZM96X4Y849Y23CC2SDX212H", "contract", "wrong task");

  const v0 = json("tests/compatibility/adapter/v0/vocabulary.json");
  equal(v1.capabilities, v0.capabilities.map(({ name }) => name), "capabilities", "capability list drifted");
  equal(v1.errors, v0.errors.map(({ name }) => name), "errors", "error list drifted");
  equal(v1.event_kinds, v0.event_kinds, "events", "event list drifted");
  equal(v1.rename_dispositions, ["Fail", "Replace"], "dispositions", "rename dispositions changed");
  equal(v1.rename_evidence, {
    carrier: "separate-evidence-object",
    event_key: ["view", "sequence"],
    cardinality: "zero-or-one",
    fields: FIELDS,
    outcomes: ["Available", "Unsupported"],
    unsupported_code: "rename-binding-evidence-unavailable",
    guessing: "forbidden",
  }, "evidence-fields", "rename evidence shape changed");
  equal(v1.identity, {
    moved_object: ["Preserved", "Changed"],
    destination_binding: ["Created", "Preserved", "Replaced"],
  }, "identity", "identity outcomes changed");
  equal(v1.boundary_reasons, REASONS, "boundary-reasons", "boundary list changed");
  equal(v1.boundary_observations, ["None", "Candidate", "Unsupported"], "boundary-observations", "observation list changed");
  equal(v1.producers.map(({ reason, producer }) => [reason, producer]), PRODUCERS, "producers", "producer list changed");
  equal(v1.ordering, {
    view: "EventSequence-ascending",
    concurrent: ["lamport", "event_ulid", "content_hash"],
    wall_clock: "forbidden",
  }, "ordering", "ordering changed");
  equal(v1.compatibility, {
    adapter_0: "frozen-readable",
    adapter_1: "explicit-opt-in",
    unknown_contract: "refuse",
    silent_translation: "forbidden",
  }, "compatibility-policy", "compatibility policy changed");

  equal(compatibility, json("tests/compatibility/workspace-adapter-boundaries/v0/fixtures/compatibility.json").cases.map((entry) => ({
    id: entry.id,
    source_contract: entry.source_contract,
    input: entry.input.reason,
    result: entry.expected.result,
    ...(entry.expected.reason ? { output: entry.expected.reason } : {}),
    ...(entry.expected.code ? { code: entry.expected.code } : {}),
  })), "compatibility-cases", "compatibility fixtures disagree with the approved boundary contract");

  if (source) {
    const rust = read("crates/mesh-materializer/src/adapter.rs");
    equal(variants(rust, "AdapterCapability"), v1.capabilities, "rust-vocabulary", "Rust capabilities disagree");
    equal(variants(rust, "AdapterError"), v1.errors, "rust-vocabulary", "Rust errors disagree");
    equal(variants(rust, "FsEventKind"), v1.event_kinds, "rust-vocabulary", "Rust events disagree");
    equal(variants(rust, "RenameDisposition"), v1.rename_dispositions, "rust-vocabulary", "Rust dispositions disagree");
    equal(variants(rust, "RenameEvidenceField").map((name) => ({
      SourceParent: "source_before.parent", SourceName: "source_before.name", SourceObject: "source_before.object",
      DestinationBeforeState: "destination_before.state", DestinationBeforeObject: "destination_before.object",
      DestinationAfterParent: "destination_after.parent", DestinationAfterName: "destination_after.name",
      DestinationAfterObject: "destination_after.object",
    })[name]), FIELDS, "rust-vocabulary", "Rust evidence members disagree");
    equal(variants(rust, "BoundaryReasonV1"), REASONS, "rust-vocabulary", "Rust v1 reasons disagree");
    equal(variants(rust, "BoundaryObservationV1"), v1.boundary_observations, "rust-vocabulary", "Rust observations disagree");
    equal(variants(rust, "BoundaryReason"), ["Closed", "Synced", "RenamedIntoPlace", "MetadataSettled"], "v0-frozen", "Rust v0 reason history changed");
    for (const needle of [
      "rename_with_evidence", "move_entry_with_evidence", "WORKSPACE_ADAPTER_CONTRACT_V1",
      "RenameEvidenceUnavailable::all", "DestinationBindingOutcome::Replaced",
      "removed-boundary-reason",
    ]) if (!rust.includes(needle)) fail("rust-surface", `materializer omits ${needle}`);
    const fuse = read("crates/mesh-fuse/src/adapter.rs") + read("crates/mesh-fuse/src/tree.rs") + read("crates/mesh-fuse/src/view.rs");
    for (const needle of [
      "WORKSPACE_ADAPTER_CONTRACT_V1", "rename_with_evidence", "modified_handle_count",
      "DestinationBindingOutcome::Replaced", "RenameEvidenceUnavailable::all",
    ]) if (!fuse.includes(needle)) fail("fuse-surface", `FUSE omits ${needle}`);
  }
}

function mutations() {
  const fieldMutations = FIELDS.map((field) => ({
    name: `remove-${field.replaceAll(".", "-")}`,
    expected: "evidence-fields",
    apply(v1) { v1.rename_evidence.fields = v1.rename_evidence.fields.filter((candidate) => candidate !== field); },
  }));
  return [...fieldMutations,
    { name: "restore-metadata-settled", expected: "boundary-reasons", apply(v1) { v1.boundary_reasons.push("MetadataSettled"); } },
    ...REASONS.map((reason) => ({ name: `remove-${reason}-producer`, expected: "producers", apply(v1) { v1.producers = v1.producers.filter((entry) => entry.reason !== reason); } })),
    { name: "remove-replace-disposition", expected: "dispositions", apply(v1) { v1.rename_dispositions.pop(); } },
    { name: "use-wall-clock", expected: "ordering", apply(v1) { v1.ordering.wall_clock = "tie-break"; } },
    { name: "change-unsupported-code", expected: "evidence-fields", apply(v1) { v1.rename_evidence.unsupported_code = "guess"; } },
    { name: "unknown-contract", expected: "contract", apply(v1) { v1.contract = "mesh-workspace-adapter/2"; } },
    { name: "silent-metadata-translation", expected: "compatibility-cases", apply(_v1, compatibility) { const row = compatibility.find(({ id }) => id === "adapter-0-metadata-settled"); row.result = "valid"; row.output = "Synced"; delete row.code; } },
  ];
}

try {
  const vocabulary = json("tests/compatibility/adapter/v1/vocabulary.json");
  const compatibilityDocument = json("tests/compatibility/adapter/v1/compatibility.json");
  equal(Object.keys(compatibilityDocument), ["contract", "cases"], "closed-document", "compatibility keys changed");
  equal(compatibilityDocument.contract, "mesh-workspace-adapter/1/compatibility", "contract", "compatibility token changed");
  validate(vocabulary, compatibilityDocument.cases);
  if (process.argv.includes("--mutations")) {
    const rejected = [];
    for (const mutation of mutations()) {
      const v1 = clone(vocabulary);
      const compatibility = clone(compatibilityDocument.cases);
      mutation.apply(v1, compatibility);
      try {
        validate(v1, compatibility, { source: false });
        fail("mutation-survived", mutation.name);
      } catch (error) {
        if (!(error instanceof Failure) || error.code === "mutation-survived") throw error;
        if (error.code !== mutation.expected) fail("mutation-wrong-failure", `${mutation.name}: ${error.code} != ${mutation.expected}`);
        rejected.push(`${mutation.name}:${error.code}`);
      }
    }
    process.stdout.write(`adapter-v1 mutations: PASS — ${rejected.length} named mutations rejected\n`);
    for (const result of rejected) process.stdout.write(`- ${result}\n`);
  } else {
    process.stdout.write(`adapter-v1: PASS — ${FIELDS.length} evidence fields, ${REASONS.length} boundary reasons, ${PRODUCERS.length} real producers, 5 migration cases\n`);
  }
} catch (error) {
  const code = error instanceof Failure ? `${error.code}: ` : "";
  process.stderr.write(`adapter-v1: FAIL — ${code}${error.stack ?? error.message}\n`);
  process.exitCode = 1;
}
