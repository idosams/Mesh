#!/usr/bin/env node
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../../..");
const requirementsPath = resolve(here, "requirements.json");
const evidencePath = resolve(here, "evidence.json");

const parse = async (path) => JSON.parse(await readFile(path, "utf8"));
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

export function expectedKeys(requirements) {
  const keys = [];
  for (const sample of requirements.samples)
    for (const candidate of requirements.candidates)
      for (const size of requirements.sizes_bytes)
        for (const pattern of requirements.patterns)
          for (const version of requirements.versions)
            for (const actor of requirements.actors)
              keys.push(`${sample}/${candidate}/${size}/${pattern}/${version}/${actor}`);
  return keys;
}

function rowKey(row) {
  return `${row.sample}/${row.candidate}/${row.size_bytes}/${row.pattern}/${row.version}/${row.actor}`;
}

export function decide(requirements, rows) {
  const expected = expectedKeys(requirements);
  const observed = rows.map(rowKey);
  const failures = [];
  if (expected.length !== requirements.row_count) failures.push("requirements row_count drift");
  if (observed.length !== expected.length) failures.push(`expected ${expected.length} rows, observed ${observed.length}`);
  const duplicate = observed.find((key, index) => observed.indexOf(key) !== index);
  if (duplicate) failures.push(`duplicate row ${duplicate}`);
  const orderMismatch = observed.findIndex((key, index) => key !== expected[index]);
  if (orderMismatch !== -1) failures.push(`missing or reordered row at ordinal ${orderMismatch}`);
  for (const row of rows) {
    if (row.reconstruction_failures !== 0) failures.push(`reconstruction failure at ${rowKey(row)}`);
    if (row.actor === "identical-second" && row.novel_stored_manifest_bytes !== 0)
      failures.push(`second actor stored novel bytes at ${rowKey(row)}`);
  }

  // A C win is intentionally impossible unless all exact rows are present. The full producer is
  // future work; this boundary verifier's job is to prevent partial or adjacent evidence from
  // being promoted into a protocol decision.
  return {
    status: failures.length === 0 ? "complete" : "inconclusive",
    selected_boundary: failures.length === 0 ? "C-evaluation-required" : "B",
    candidate_c_authorized: false,
    failures
  };
}

async function verify() {
  const [requirements, evidence] = await Promise.all([parse(requirementsPath), parse(evidencePath)]);
  assert.equal(requirements.schema, "mesh.manifest-scaling-requirements/1");
  assert.equal(requirements.generator.name, "manifest-scaling-stream/1");
  assert.equal(requirements.generator.seed, "0x000000004d455348");
  assert.equal(expectedKeys(requirements).length, 6120);
  assert.deepEqual(requirements.thresholds, {
    c_publication_bytes_max_percent_of_b: 25,
    c_point_lookup_decoded_bytes_max_percent_of_a: 25,
    c_full_reconstruction_decoded_bytes_max_percent_of_a: 110,
    c_novel_storage_bytes_max_percent_of_b: 110
  });

  for (const source of Object.values(evidence.sources)) {
    const bytes = await readFile(resolve(root, source.path));
    assert.equal(sha256(bytes), source.sha256, `${source.path} drifted after the evidence pin`);
  }
  const contextLines = (await readFile(resolve(root, evidence.sources.chunk_policy_results.path), "utf8")).trimEnd().split("\n");
  const context = JSON.parse(contextLines[evidence.context_only_observation.source_line - 1]);
  for (const key of ["workload", "scale", "segment", "policy", "content_bytes", "chunk_refs", "transfer_bytes", "roundtrip_failures", "sample_count"])
    assert.deepEqual(context[key], evidence.context_only_observation[key], `context-only ${key} drifted`);

  const decision = decide(requirements, evidence.manifest_scaling_rows);
  assert.equal(decision.status, evidence.result.status);
  assert.equal(decision.selected_boundary, evidence.result.selected_boundary);
  assert.equal(decision.candidate_c_authorized, false);
  assert.equal(evidence.result.required_rows, requirements.row_count);
  assert.equal(evidence.result.observed_rows, evidence.manifest_scaling_rows.length);
  assert.equal(evidence.result.missing_rows, requirements.row_count - evidence.manifest_scaling_rows.length);
  assert.equal(evidence.result.logical_schema, "mesh.v0.file-manifest");
  assert.match(evidence.context_only_observation.why_not_selection_evidence, /not canonical publication/);
  console.log(`manifest-scaling: PASS (${evidence.result.observed_rows}/${evidence.result.required_rows} comparable rows; explicit ${evidence.result.status}; retain ${evidence.result.selected_boundary}/v0)`);
}

function runMutations() {
  const req = {
    samples: [1], candidates: ["A"], sizes_bytes: [1], patterns: ["overwrite-1k"],
    versions: [0], actors: ["primary", "identical-second"], row_count: 2
  };
  const row = (actor, extra = {}) => ({ sample: 1, candidate: "A", size_bytes: 1,
    pattern: "overwrite-1k", version: 0, actor, reconstruction_failures: 0,
    novel_stored_manifest_bytes: actor === "primary" ? 1 : 0, ...extra });
  const complete = [row("primary"), row("identical-second")];
  assert.equal(decide(req, complete).status, "complete");
  assert.equal(decide(req, complete.slice(0, 1)).selected_boundary, "B");
  assert.match(decide(req, [complete[1], complete[0]]).failures.join("\n"), /reordered/);
  assert.match(decide(req, [complete[0], complete[0]]).failures.join("\n"), /duplicate/);
  assert.match(decide(req, [row("primary", { reconstruction_failures: 1 }), complete[1]]).failures.join("\n"), /reconstruction/);
  assert.match(decide(req, [complete[0], row("identical-second", { novel_stored_manifest_bytes: 1 })]).failures.join("\n"), /second actor/);
  console.log("manifest-scaling mutations: PASS (5 planted evidence failures rejected)");
}

await verify();
if (process.argv.includes("--mutations")) runMutations();
