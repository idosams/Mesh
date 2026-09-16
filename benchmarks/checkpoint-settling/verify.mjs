#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

import { buildArtifacts } from "./analyze.mjs";
import { FIXTURE, fixtureDigest, fixtureEntries } from "./fixture.mjs";
import { fixtureSemanticOutcome } from "./semantic.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, "..", "..");
const full = process.argv.includes("--full") || process.argv.includes("--mutations");
const mutations = process.argv.includes("--mutations");
const manifestPath = join(here, "manifest.json");
const manifestBytes = readFileSync(manifestPath);
const manifest = JSON.parse(manifestBytes);
const verificationFailed = Symbol("verification failed");

try {
  run();
} catch (error) {
  if (error !== verificationFailed) throw error;
  process.exitCode = 1;
}

function run() {
  const designErrors = [...validateDesign(manifest), ...validateSemanticOracle()];
  if (designErrors.length) finish(designErrors);

  if (!full) {
    process.stdout.write(`checkpoint-settling design: PASS — fixture ${fixtureDigest()}, rule ${manifest.selection_rule_digest}\n`);
    return;
  }

  for (const path of [manifest.accepted_captures.macos.path, manifest.accepted_captures.linux.path, ...manifest.tool_acquisition.captures.map((capture) => capture.path), "results.jsonl", "report.md"]) {
    if (!existsSync(join(here, path))) finish([`${path}: missing full-run artifact`]);
  }

  const cataloguePath = join(repo, "tests", "compatibility", "save-patterns", "v0", "catalogue.json");
  const state = {
    manifest,
    manifest_digest: digest(manifestBytes),
    catalogue: JSON.parse(readFileSync(cataloguePath)),
    catalogue_digest: digest(readFileSync(cataloguePath)),
    captures: {
      macos: JSON.parse(readFileSync(join(here, manifest.accepted_captures.macos.path))),
      linux: JSON.parse(readFileSync(join(here, manifest.accepted_captures.linux.path))),
    },
    capture_file_digests: {
      macos: digest(readFileSync(join(here, manifest.accepted_captures.macos.path))),
      linux: digest(readFileSync(join(here, manifest.accepted_captures.linux.path))),
    },
    tool_captures: manifest.tool_acquisition.captures.map((configured) => JSON.parse(readFileSync(join(here, configured.path)))),
    tool_capture_file_digests: manifest.tool_acquisition.captures.map((configured) => digest(readFileSync(join(here, configured.path)))),
    rows: readFileSync(join(here, "results.jsonl"), "utf8").trimEnd().split("\n").map((line) => JSON.parse(line)),
    report: readFileSync(join(here, "report.md"), "utf8"),
  };

  const built = buildArtifacts();
  const exactErrors = [];
  if (`${state.rows.map((row) => JSON.stringify(row)).join("\n")}\n` !== built.results) exactErrors.push("results.jsonl is not the deterministic analysis output");
  if (state.report !== built.report) exactErrors.push("report.md is not the deterministic analysis output");
  exactErrors.push(...validateFull(state));
  if (exactErrors.length) finish(exactErrors);

  let killed = 0;
  if (mutations) {
    const cases = mutationCases(state);
    for (const mutation of cases) {
      const mutant = structuredClone(state);
      mutation.change(mutant);
      if (validateFull(mutant).length === 0) finish([`mutation survived: ${mutation.name}`]);
      killed += 1;
    }
  }

  process.stdout.write(`checkpoint-settling full: PASS — ${state.rows.length} rows${mutations ? `, ${killed}/${killed} mutations rejected` : ""}\n`);
}

function validateDesign(value) {
  const errors = [];
  equal(value.contract, "mesh-checkpoint-settling-manifest/1", "manifest contract", errors);
  equal(value.task, "01KZM78X6WQ0YACD52KBVDPVRJ", "task", errors);
  equal(value.successor_fixture.seed, FIXTURE.seed, "fixture seed", errors);
  equal(value.successor_fixture.files, FIXTURE.files, "fixture files", errors);
  equal(value.successor_fixture.bytes_per_file, FIXTURE.bytes_per_file, "fixture bytes_per_file", errors);
  equal(value.successor_fixture.input_digest, fixtureDigest(), "fixture digest", errors);
  equal(value.historical_provenance.selection_input, false, "historical aggregate selection fence", errors);
  equal(value.required_save_pattern_cells, 12, "required matrix cells", errors);
  equal(value.selection_rule_digest, digest(JSON.stringify(value.selection_rule)), "frozen selection rule digest", errors);
  const linux = value.acquisition?.linux;
  for (const token of ["docker build", "Dockerfile.linux", "mesh-settling-exact:"]) if (!linux?.build_recipe?.includes(token)) errors.push(`Linux build recipe missing ${token}`);
  for (const token of ["docker image inspect", "IMAGE_ID=", "--network none", "--mount type=bind", "MESH_SETTLING_IMAGE=\"$IMAGE_ID\"", "\"$IMAGE_ID\" --platform", "--fixture-digest", "--out /out/", value.successor_fixture.input_digest, value.accepted_captures.instrument_commit]) {
    if (!linux?.recipe?.includes(token)) errors.push(`Linux acquisition recipe missing ${token}`);
  }
  for (const [name, values] of Object.entries(value.candidate_grid ?? {})) {
    if (!Array.isArray(values) || values.length < 2 || values.some((candidate, index) => !Number.isInteger(candidate) || candidate <= 0 || (index > 0 && candidate <= values[index - 1]))) errors.push(`${name}: candidates must be positive ascending integers`);
  }
  for (const name of ["idle_interval_ms", "maximum_uncheckpointed_bytes", "maximum_uncheckpointed_time_ms"]) if (!Array.isArray(value.candidate_grid?.[name])) errors.push(`${name}: candidate dimension absent`);
  for (const field of ["precondition", "idle_interval_ms", "maximum_uncheckpointed_bytes", "maximum_uncheckpointed_time_ms", "platform_disagreement", "aggregation"]) {
    if (typeof value.selection_rule?.[field] !== "string" || value.selection_rule[field].length < 20) errors.push(`selection_rule.${field}: missing`);
  }
  if (!value.selection_rule?.precondition.includes("12 save-pattern cells")) errors.push("selection precondition does not bind the full matrix");
  if (!value.selection_rule?.aggregation.includes("never combined")) errors.push("selection rule permits an unweighted composite");
  const tools = value.tool_acquisition;
  equal(tools?.revision, 2, "tool acquisition revision", errors);
  equal(tools?.capture_script, "capture-tools.mjs", "tool capture script", errors);
  equal(tools?.current_capture_script_digest, digest(readFileSync(join(here, "capture-tools.mjs"))), "current tool capture script digest", errors);
  equal(tools?.seed, 347002, "tool acquisition seed", errors);
  equal(tools?.samples_per_arm, 5, "tool samples per arm", errors);
  equal(tools?.captures?.length, 7, "tool capture files", errors);
  equal(tools?.acquired_cells?.length, 10, "acquired tool cells", errors);
  equal(Object.keys(tools?.unmet_cells ?? {}).length, 0, "unmet tool cells", errors);
  if (tools?.macos_recipes?.length !== 3 || tools.macos_recipes.some((recipe) => !recipe.includes("--samples 5"))) errors.push("macOS tool recipes are incomplete");
  if (tools?.linux_recipes?.length !== 4 || tools.linux_recipes.some((recipe) => !recipe.includes("--network none") || !recipe.includes("sha256:") || !recipe.includes("--samples 5"))) errors.push("Linux tool recipes are not pinned and network-disabled");
  equal(tools?.macos_jetbrains_artifact?.sha256, "2146b7f5e33d8eab68d9147a15b6df11b3c07b3e6130f351c0b16069c2e6faed", "macOS JetBrains artifact digest", errors);
  for (const configured of tools?.captures ?? []) {
    if (!sha(configured.digest)) errors.push(`${configured.path}: capture digest absent`);
    if (!sha(configured.corpus_digest)) errors.push(`${configured.path}: corpus digest absent`);
    if (!sha(configured.capture_script_digest)) errors.push(`${configured.path}: script digest absent`);
    if (!/^[0-9a-f]{40}$/.test(configured.instrument_commit ?? "")) errors.push(`${configured.path}: instrument commit absent`);
  }
  return errors;
}

function validateFull(value) {
  const errors = validateDesign(value.manifest);
  const provenance = only(value.rows, (row) => row.kind === "provenance", "provenance", errors);
  if (provenance) {
    equal(provenance.task, value.manifest.task, "results task", errors);
    equal(provenance.manifest_digest, value.manifest_digest, "results manifest digest", errors);
    equal(provenance.catalogue_digest, value.catalogue_digest, "results catalogue digest", errors);
    equal(provenance.historical_selection_input, false, "results historical selection fence", errors);
  }

  for (const name of ["macos", "linux"]) validateCapture(value, name, errors);
  validateToolCaptures(value, errors);

  const expectedPatterns = new Map(value.catalogue.patterns.map((entry) => [entry.pattern, entry]));
  const patternRows = value.rows.filter((row) => row.kind === "pattern_replay");
  equal(patternRows.length, expectedPatterns.size, "pattern replay count", errors);
  for (const [pattern] of expectedPatterns) {
    const row = patternRows.find((candidate) => candidate.pattern === pattern);
    if (!row) errors.push(`pattern replay absent: ${pattern}`);
    else {
      equal(row.replay, "pass", `${pattern} replay`, errors);
      if (!sha(row.source_digest)) errors.push(`${pattern}: source digest absent`);
    }
  }

  const expectedCells = [];
  for (const [family, platforms] of Object.entries(value.catalogue.coverage.cells)) for (const [platform, cell] of Object.entries(platforms)) expectedCells.push({ family, platform, pattern: cell.pattern });
  const cells = value.rows.filter((row) => row.kind === "matrix_cell");
  equal(cells.length, value.manifest.required_save_pattern_cells, "matrix row count", errors);
  for (const expected of expectedCells) {
    const matches = cells.filter((row) => row.family === expected.family && row.platform === expected.platform);
    equal(matches.length, 1, `matrix ${expected.family}/${expected.platform}`, errors);
    const row = matches[0];
    if (!row) continue;
    equal(row.pattern, expected.pattern, `${expected.family}/${expected.platform} pattern`, errors);
    equal(row.semantic_replay, "pass", `${expected.family}/${expected.platform} replay`, errors);
    const cellName = `${expected.family}/${expected.platform}`;
    const shouldBeTimed = expected.family === "a package manager install" || value.manifest.tool_acquisition.acquired_cells.includes(cellName);
    equal(row.timed, shouldBeTimed, `${expected.family}/${expected.platform} timed status`, errors);
    const expectedSource = expected.family === "a package manager install"
      ? value.manifest.accepted_captures[expected.platform === "macOS" ? "macos" : "linux"].path
      : toolTimingSources(value).get(cellName) ?? null;
    equal(row.timing_source, shouldBeTimed ? expectedSource : null, `${expected.family}/${expected.platform} timing source`, errors);
  }

  for (const name of ["macos", "linux"]) {
    const capture = value.captures[name];
    const observations = value.rows.filter((row) => row.kind === "successor_observation" && row.platform === name);
    equal(observations.length, capture?.observations?.length, `${name} normalized observation count`, errors);
    const summary = only(value.rows, (row) => row.kind === "successor_capture" && row.platform === name, `${name} successor summary`, errors);
    if (summary && capture) {
      equal(summary.source_digest, value.capture_file_digests[name], `${name} summary digest`, errors);
      equal(summary.repository_commit, capture.repository_commit, `${name} summary commit`, errors);
      equal(summary.observations, capture.observations.length, `${name} summary observations`, errors);
      equal(summary.changes, capture.observations.reduce((sum, row) => sum + row.changes.length, 0), `${name} summary changes`, errors);
      equal(summary.semantic?.verified, true, `${name} summary semantics`, errors);
      if (summary.changes === value.manifest.historical_provenance.changes) errors.push(`${name}: historical aggregate spliced into successor summary`);
    }
    for (const [parameter, candidates] of Object.entries(value.manifest.candidate_grid)) {
      const rows = value.rows.filter((row) => row.kind === "candidate" && row.platform === name && row.parameter === parameter);
      equal(rows.length, candidates.length, `${name}/${parameter} candidate count`, errors);
      for (const candidate of candidates) equal(rows.filter((row) => row.candidate === candidate).length, 1, `${name}/${parameter}/${candidate}`, errors);
    }
  }

  const decision = only(value.rows, (row) => row.kind === "decision", "decision", errors);
  const missing = expectedCells
    .filter((cell) => cell.family !== "a package manager install" && !value.manifest.tool_acquisition.acquired_cells.includes(`${cell.family}/${cell.platform}`))
    .map((cell) => `${cell.family}/${cell.platform}`);
  if (decision) {
    const expectedSelected = recomputeSelection(value.rows, value.manifest.candidate_grid, missing);
    const expectedStatus = missing.length === 0 && Object.values(expectedSelected).every((selected) => selected !== null) ? "selected" : missing.length > 0 ? "unresolved-incomplete-matrix" : "unresolved-no-eligible-candidate";
    equal(decision.status, expectedStatus, "decision status", errors);
    equal(decision.required_timed_cells, 12, "decision required cells", errors);
    equal(decision.timed_cells, 12 - missing.length, "decision timed cells", errors);
    equal(JSON.stringify(decision.missing_timed_cells), JSON.stringify(missing), "decision missing cells", errors);
    equal(JSON.stringify(decision.selected), JSON.stringify(expectedSelected), "decision selected values", errors);
    for (const row of value.rows.filter((candidate) => candidate.kind === "candidate" || candidate.kind === "tool_candidate")) {
      const eligible = expectedSelected[row.parameter] !== null && row.candidate === expectedSelected[row.parameter];
      equal(row.eligible, eligible, `${row.kind}/${row.parameter}/${row.candidate} eligibility`, errors);
      equal(row.ineligible_reason, eligible ? null : missing.length > 0 ? "full timed matrix absent" : "not selected by frozen rule", `${row.kind}/${row.parameter}/${row.candidate} reason`, errors);
    }
  }
  if (!value.report.includes("Selected from the complete twelve-cell timed matrix") || !value.report.includes("timed cells: 12/12")) errors.push("report does not publish the complete selection");
  return errors;
}

function validateToolCaptures(value, errors) {
  const configuredRows = value.manifest.tool_acquisition.captures;
  equal(value.tool_captures?.length, configuredRows.length, "loaded tool captures", errors);
  const seenCells = new Set();
  for (let index = 0; index < configuredRows.length; index += 1) {
    const configured = configuredRows[index];
    const capture = value.tool_captures?.[index];
    if (!capture) {
      errors.push(`${configured.path}: tool capture absent`);
      continue;
    }
    equal(value.tool_capture_file_digests?.[index], configured.digest, `${configured.path} file digest`, errors);
    equal(capture.contract, "mesh-checkpoint-settling-tool-capture/1", `${configured.path} contract`, errors);
    equal(capture.task, value.manifest.task, `${configured.path} task`, errors);
    equal(capture.repository_commit, configured.instrument_commit, `${configured.path} commit`, errors);
    equal(capture.capture_script_digest, configured.capture_script_digest, `${configured.path} script`, errors);
    equal(capture.seed, value.manifest.tool_acquisition.seed, `${configured.path} seed`, errors);
    equal(capture.sample_count_per_arm, value.manifest.tool_acquisition.samples_per_arm, `${configured.path} sample count`, errors);
    equal(capture.corpus_digest, configured.corpus_digest, `${configured.path} configured corpus`, errors);
    equal(capture.corpus_digest, digest(Buffer.from(canonical(capture.arms))), `${configured.path} recomputed corpus`, errors);
    if (!Array.isArray(capture.invocation) || capture.invocation.length < 3) errors.push(`${configured.path}: invocation absent`);
    const platform = capture.platform?.os === "darwin" ? "macOS" : capture.platform?.os === "linux" ? "Linux" : null;
    if (!platform) errors.push(`${configured.path}: unsupported platform`);
    const required = {
      "platform.tag": capture.platform?.tag,
      "platform.os_version": capture.platform?.os_version,
      "platform.arch": capture.platform?.arch,
      "platform.filesystem": capture.platform?.filesystem,
      "platform.node": capture.platform?.node,
      "hardware.cpu_model": capture.platform?.hardware?.cpu_model,
      "hardware.cpu_vendor": capture.platform?.hardware?.cpu_vendor,
      "hardware.logical_cores": capture.platform?.hardware?.logical_cores,
      "hardware.physical_cores": capture.platform?.hardware?.physical_cores,
      "hardware.memory_bytes": capture.platform?.hardware?.memory_bytes,
      network_evidence: capture.network_evidence,
      cache_state: capture.cache_state,
    };
    for (const [field, actual] of Object.entries(required)) if (actual === null || actual === undefined || actual === "" || actual === "unknown") errors.push(`${configured.path} ${field}: missing`);
    equal(capture.network_evidence, platform === "Linux" ? "docker-network-none" : "host-network-not-used", `${configured.path} network evidence`, errors);
    if (platform === "Linux" && !sha(capture.platform?.docker_image)) errors.push(`${configured.path}: Linux image digest absent`);
    if (platform === "macOS" && capture.platform?.docker_image !== null) errors.push(`${configured.path}: macOS capture names an image`);
    for (const arm of capture.arms ?? []) {
      const cell = `${arm.family}/${platform}`;
      if (seenCells.has(cell)) errors.push(`${cell}: duplicate tool capture`);
      seenCells.add(cell);
      if (!value.manifest.tool_acquisition.acquired_cells.includes(cell)) errors.push(`${cell}: not declared acquired`);
      if (!sha(arm.input_digest)) errors.push(`${cell}: input digest absent`);
      equal(arm.sample_count, value.manifest.tool_acquisition.samples_per_arm, `${cell} sample count`, errors);
      equal(arm.samples?.length, arm.sample_count, `${cell} samples`, errors);
      for (const sample of arm.samples ?? []) {
        if (!Number.isInteger(sample.seed) || !Number.isInteger(sample.sample)) errors.push(`${cell}: sample identity absent`);
        if (!sha(sample.initial_tree_digest) || !sha(sample.final_tree_digest)) errors.push(`${cell}/${sample.sample}: tree digest absent`);
        if (!Array.isArray(sample.invocation) || sample.invocation.length < 2) errors.push(`${cell}/${sample.sample}: invocation absent`);
        if (!(sample.polls > 0) || !(sample.elapsed_ms > 0) || !Array.isArray(sample.observations) || sample.observations.length === 0) errors.push(`${cell}/${sample.sample}: empty timing sample`);
        equal(sample.semantic?.verified, true, `${cell}/${sample.sample} semantic outcome`, errors);
        const sampleRows = value.rows.filter((row) => row.kind === "tool_sample" && row.family === arm.family && row.platform === platform && row.sample === sample.sample);
        equal(sampleRows.length, 1, `${cell}/${sample.sample} normalized sample`, errors);
        const observationRows = value.rows.filter((row) => row.kind === "tool_observation" && row.family === arm.family && row.platform === platform && row.sample === sample.sample);
        equal(observationRows.length, sample.observations.length, `${cell}/${sample.sample} normalized observations`, errors);
        for (const [parameter, candidates] of Object.entries(value.manifest.candidate_grid)) {
          const candidateRows = value.rows.filter((row) => row.kind === "tool_candidate" && row.family === arm.family && row.platform === platform && row.sample === sample.sample && row.parameter === parameter);
          equal(candidateRows.length, candidates.length, `${cell}/${sample.sample}/${parameter} candidates`, errors);
          for (const candidate of candidates) equal(candidateRows.filter((row) => row.candidate === candidate).length, 1, `${cell}/${sample.sample}/${parameter}/${candidate}`, errors);
        }
      }
    }
  }
  equal(seenCells.size, value.manifest.tool_acquisition.acquired_cells.length, "unique acquired tool cells", errors);
  for (const cell of value.manifest.tool_acquisition.acquired_cells) if (!seenCells.has(cell)) errors.push(`${cell}: declared acquisition absent`);
}

function toolTimingSources(value) {
  const sources = new Map();
  for (let index = 0; index < value.manifest.tool_acquisition.captures.length; index += 1) {
    const configured = value.manifest.tool_acquisition.captures[index];
    const capture = value.tool_captures?.[index];
    const platform = capture?.platform?.os === "darwin" ? "macOS" : "Linux";
    for (const arm of capture?.arms ?? []) sources.set(`${arm.family}/${platform}`, configured.path);
  }
  return sources;
}

function recomputeSelection(rows, grid, missing) {
  const selected = {
    idle_interval_ms: null,
    maximum_uncheckpointed_bytes: null,
    maximum_uncheckpointed_time_ms: null,
  };
  if (missing.length > 0) return selected;
  const candidates = rows.filter((row) => row.kind === "candidate" || row.kind === "tool_candidate");
  selected.idle_interval_ms = grid.idle_interval_ms.find((candidate) =>
    candidates.filter((row) => row.parameter === "idle_interval_ms" && row.candidate === candidate)
      .every((row) => row.false_meaningful_cuts === 0)) ?? null;
  for (const parameter of ["maximum_uncheckpointed_bytes", "maximum_uncheckpointed_time_ms"]) {
    selected[parameter] = grid[parameter].find((candidate) => {
      const matching = candidates.filter((row) => row.parameter === parameter && row.candidate === candidate);
      const platformP95Passes = ["macOS", "Linux"].every((platform) => {
        const counts = matching
          .filter((row) => (row.platform === "macos" ? "macOS" : row.platform === "linux" ? "Linux" : row.platform) === platform)
          .map((row) => row.recovery_checkpoints)
          .sort((a, b) => a - b);
        const p95 = counts[Math.min(counts.length - 1, Math.ceil(counts.length * 0.95) - 1)];
        return counts.length > 0 && p95 <= 32;
      });
      return platformP95Passes && matching.filter((row) => row.kind === "candidate").every((row) => row.preserved_intermediate);
    }) ?? null;
  }
  return selected;
}

function validateCapture(value, name, errors) {
  const capture = value.captures[name];
  if (!capture) {
    errors.push(`${name} capture absent`);
    return;
  }
  const configured = value.manifest.accepted_captures?.[name];
  if (!configured) {
    errors.push(`${name} manifest capture absent`);
    return;
  }
  equal(configured.digest, value.capture_file_digests[name], `${name} capture digest`, errors);
  equal(capture.contract, "mesh-checkpoint-settling-capture/1", `${name} contract`, errors);
  equal(capture.capture_revision, 3, `${name} capture revision`, errors);
  equal(capture.task, value.manifest.task, `${name} task`, errors);
  equal(capture.repository_commit, value.manifest.accepted_captures.instrument_commit, `${name} commit`, errors);
  equal(capture.capture_script_digest, value.manifest.accepted_captures.capture_script_digest, `${name} script`, errors);
  equal(capture.fixture?.digest, value.manifest.successor_fixture.input_digest, `${name} fixture`, errors);
  equal(capture.tarball_digest, value.manifest.accepted_captures.tarball_digest, `${name} tarball`, errors);
  equal(capture.semantic?.verified, true, `${name} semantic outcome`, errors);
  equal(capture.semantic?.installed_fixture_files, FIXTURE.files, `${name} installed fixture files`, errors);
  equal(capture.semantic?.expected_fixture_files, FIXTURE.files, `${name} expected fixture files`, errors);
  equal(capture.semantic?.missing_fixture_files, 0, `${name} missing fixture files`, errors);
  equal(capture.semantic?.unexpected_fixture_files, 0, `${name} unexpected fixture files`, errors);
  equal(capture.semantic?.mismatched_fixture_files, 0, `${name} mismatched fixture files`, errors);
  equal(capture.semantic?.fixture_bytes_verified, true, `${name} fixture byte verification`, errors);
  equal(capture.semantic?.installed_fixture_tree_digest, capture.semantic?.expected_fixture_tree_digest, `${name} fixture tree digest`, errors);
  if (!sha(capture.semantic?.installed_fixture_tree_digest)) errors.push(`${name}: invalid installed fixture tree digest`);
  equal(capture.polling?.polls, configured.polls, `${name} polls`, errors);
  equal(capture.observations?.length, configured.observations, `${name} observation count`, errors);
  equal(capture.observations?.reduce((sum, row) => sum + row.changes.length, 0), configured.changes, `${name} change count`, errors);
  equal(capture.elapsed_ms, configured.elapsed_ms, `${name} elapsed`, errors);
  const required = {
    "platform.tag": capture.platform?.tag,
    "platform.os": capture.platform?.os,
    "platform.os_version": capture.platform?.os_version,
    "platform.arch": capture.platform?.arch,
    "platform.filesystem": capture.platform?.filesystem,
    "platform.node": capture.platform?.node,
    "platform.npm": capture.platform?.npm,
    "hardware.cpu_model": capture.platform?.hardware?.cpu_model,
    "hardware.cpu_vendor": capture.platform?.hardware?.cpu_vendor,
    "hardware.logical_cores": capture.platform?.hardware?.logical_cores,
    "hardware.physical_cores": capture.platform?.hardware?.physical_cores,
    "hardware.memory_bytes": capture.platform?.hardware?.memory_bytes,
    network_evidence: capture.network_evidence,
    cache_state: capture.cache_state,
    final_tree_digest: capture.final_tree_digest,
  };
  for (const [field, actual] of Object.entries(required)) if (actual === null || actual === undefined || actual === "" || actual === "unknown") errors.push(`${name} ${field}: missing`);
  if (!sha(capture.final_tree_digest)) errors.push(`${name}: invalid final tree digest`);
  equal(capture.network_evidence, name === "linux" ? "docker-network-none" : "npm-offline-native", `${name} network evidence`, errors);
  if (name === "linux") {
    if (!sha(capture.platform?.docker_image)) errors.push("linux image digest absent");
    equal(capture.platform?.docker_image, value.manifest.acquisition.linux.built_image_digest, "linux accepted image digest", errors);
  }
  if (name === "macos" && capture.platform?.docker_image !== null) errors.push("macOS capture falsely names a Docker image");
}

function mutationCases(base) {
  const cases = [
    { name: "historical aggregate labelled selection input", change: (x) => { x.manifest.historical_provenance.selection_input = true; } },
    { name: "selection rule changed", change: (x) => { x.manifest.selection_rule.aggregation += " changed"; } },
    { name: "Linux output mount absent", change: (x) => { x.manifest.acquisition.linux.recipe = x.manifest.acquisition.linux.recipe.replace("--mount type=bind", "--volume-missing"); } },
    { name: "Linux image digest absent from process", change: (x) => { x.manifest.acquisition.linux.recipe = x.manifest.acquisition.linux.recipe.replace("MESH_SETTLING_IMAGE=", "IMAGE_DIGEST_MISSING="); } },
    { name: "Linux mutable tag executed instead of inspected image", change: (x) => { x.manifest.acquisition.linux.recipe = x.manifest.acquisition.linux.recipe.replace("\"$IMAGE_ID\" --platform", "mesh-settling-exact:b64a7284 --platform"); } },
    { name: "Linux accepted capture relabelled with another valid image digest", change: (x) => { x.captures.linux.platform.docker_image = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"; } },
    { name: "Linux fixture digest absent", change: (x) => { x.manifest.acquisition.linux.recipe = x.manifest.acquisition.linux.recipe.replace("--fixture-digest", "--input-digest"); } },
    { name: "macOS arm absent", change: (x) => { delete x.captures.macos; } },
    { name: "Linux arm absent", change: (x) => { delete x.captures.linux; } },
    { name: "tool capture absent", change: (x) => { x.tool_captures.splice(0, 1); x.tool_capture_file_digests.splice(0, 1); } },
    { name: "tool capture file digest changed", change: (x) => { x.tool_capture_file_digests[0] = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"; } },
    { name: "tool corpus digest changed", change: (x) => { x.tool_captures[0].corpus_digest = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"; } },
    { name: "tool semantic outcome false", change: (x) => { x.tool_captures[0].arms[0].samples[0].semantic.verified = false; } },
    { name: "tool input digest absent", change: (x) => { x.tool_captures[0].arms[0].input_digest = null; } },
    { name: "tool environment absent", change: (x) => { x.tool_captures[0].platform.hardware.cpu_vendor = null; } },
    { name: "tool sample absent", change: (x) => { x.tool_captures[0].arms[0].samples.pop(); } },
    { name: "acquired matrix cell falsely untimed", change: (x) => { const row = x.rows.find((candidate) => candidate.kind === "matrix_cell" && candidate.family === "a formatter" && candidate.platform === "macOS"); row.timed = false; row.timing_source = null; } },
    { name: "historical stream spliced", change: (x) => { x.rows.find((row) => row.kind === "successor_capture").changes = x.manifest.historical_provenance.changes; } },
    { name: "selected decision relabelled unresolved", change: (x) => { x.rows.find((row) => row.kind === "decision").status = "unresolved-incomplete-matrix"; } },
    { name: "selected value absent", change: (x) => { x.rows.find((row) => row.kind === "decision").selected.idle_interval_ms = 123456; } },
    { name: "semantic outcome false", change: (x) => { x.captures.macos.semantic.verified = false; } },
    { name: "installed fixture byte corrupted", change: (x) => { x.captures.macos.semantic.installed_fixture_tree_digest = "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"; } },
    { name: "pattern replay absent", change: (x) => { x.rows.splice(x.rows.findIndex((row) => row.kind === "pattern_replay"), 1); } },
    { name: "pattern digest absent", change: (x) => { x.rows.find((row) => row.kind === "pattern_replay").source_digest = null; } },
  ];
  const captureFields = [
    ["repository_commit"], ["capture_script_digest"], ["fixture", "digest"], ["tarball_digest"], ["final_tree_digest"],
    ["platform", "tag"], ["platform", "os"], ["platform", "os_version"], ["platform", "arch"], ["platform", "filesystem"],
    ["platform", "node"], ["platform", "npm"], ["platform", "hardware", "cpu_model"], ["platform", "hardware", "cpu_vendor"],
    ["platform", "hardware", "logical_cores"], ["platform", "hardware", "physical_cores"], ["platform", "hardware", "memory_bytes"],
    ["network_evidence"], ["cache_state"],
  ];
  for (const path of captureFields) cases.push({ name: `capture metadata ${path.join(".")} absent`, change: (x) => setPath(x.captures.macos, path, null) });
  for (const [family, platforms] of Object.entries(base.catalogue.coverage.cells)) {
    for (const platform of Object.keys(platforms)) cases.push({
      name: `matrix cell ${family}/${platform} absent`,
      change: (x) => { x.rows.splice(x.rows.findIndex((row) => row.kind === "matrix_cell" && row.family === family && row.platform === platform), 1); },
    });
  }
  cases.push({ name: "timed cell bound to wrong source", change: (x) => { const row = x.rows.find((candidate) => candidate.kind === "matrix_cell" && candidate.family === "VS Code"); row.timing_source = x.manifest.accepted_captures.macos.path; } });
  for (const parameter of Object.keys(base.manifest.candidate_grid)) cases.push({
    name: `candidate dimension ${parameter} incomplete`,
    change: (x) => { x.rows.splice(x.rows.findIndex((row) => row.kind === "candidate" && row.parameter === parameter), 1); },
  });
  for (const parameter of Object.keys(base.manifest.candidate_grid)) cases.push({
    name: `tool candidate dimension ${parameter} incomplete`,
    change: (x) => { x.rows.splice(x.rows.findIndex((row) => row.kind === "tool_candidate" && row.parameter === parameter), 1); },
  });
  return cases;
}

function validateSemanticOracle() {
  const errors = [];
  const prefix = `node_modules/${FIXTURE.package_name}/`;
  const tree = new Map(
    fixtureEntries()
      .filter(([path]) => path.startsWith("files/"))
      .map(([path, bytes]) => [`${prefix}${path}`, { dir: false, bytes: Buffer.from(bytes) }]),
  );
  const exact = fixtureSemanticOutcome(tree);
  if (!exact.fixture_bytes_verified || exact.missing_fixture_files !== 0 || exact.unexpected_fixture_files !== 0 || exact.mismatched_fixture_files !== 0) errors.push("fixture semantic oracle rejects exact bytes");
  const first = tree.values().next().value;
  first.bytes[0] ^= 0xff;
  const corrupted = fixtureSemanticOutcome(tree);
  if (corrupted.fixture_bytes_verified || corrupted.mismatched_fixture_files !== 1) errors.push("fixture semantic oracle accepts a corrupted file");
  return errors;
}

function setPath(object, path, value) {
  let cursor = object;
  for (const part of path.slice(0, -1)) cursor = cursor[part];
  cursor[path.at(-1)] = value;
}

function only(rows, predicate, name, errors) {
  const matches = rows.filter(predicate);
  equal(matches.length, 1, `${name} row count`, errors);
  return matches[0];
}

function equal(actual, expected, name, errors) { if (actual !== expected) errors.push(`${name}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`); }
function sha(value) { return typeof value === "string" && /^sha256:[0-9a-f]{64}$/.test(value); }
function digest(bytes) { return `sha256:${createHash("sha256").update(bytes).digest("hex")}`; }
function canonical(input) {
  if (Array.isArray(input)) return `[${input.map(canonical).join(",")}]`;
  if (input && typeof input === "object") return `{${Object.keys(input).sort().map((key) => `${JSON.stringify(key)}:${canonical(input[key])}`).join(",")}}`;
  return JSON.stringify(input);
}
function finish(errors) { for (const error of errors) process.stderr.write(`checkpoint-settling: ${error}\n`); throw verificationFailed; }
