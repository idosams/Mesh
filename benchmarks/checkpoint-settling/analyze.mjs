#!/usr/bin/env node

import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, "..", "..");
const manifest = json(join(here, "manifest.json"));
const cataloguePath = join(repo, "tests", "compatibility", "save-patterns", "v0", "catalogue.json");
const catalogue = json(cataloguePath);

export function buildArtifacts() {
  const rows = [];
  rows.push({
    kind: "provenance",
    contract: "mesh-checkpoint-settling-results/1",
    task: manifest.task,
    manifest_digest: digestFile(join(here, "manifest.json")),
    catalogue_digest: digestFile(cataloguePath),
    historical_changes: manifest.historical_provenance.changes,
    historical_selection_input: false,
  });

  const patternRows = replayPatterns();
  rows.push(...patternRows);
  const toolCaptures = manifest.tool_acquisition.captures.map(loadToolCapture);
  const timingSources = new Map();
  for (const input of toolCaptures) {
    const platform = input.capture.platform.os === "darwin" ? "macOS" : "Linux";
    for (const arm of input.capture.arms) timingSources.set(`${arm.family}/${platform}`, input.configured.path);
  }
  timingSources.set("a package manager install/macOS", manifest.accepted_captures.macos.path);
  timingSources.set("a package manager install/Linux", manifest.accepted_captures.linux.path);
  const matrixRows = matrix(patternRows, timingSources);
  rows.push(...matrixRows);

  const captures = ["macos", "linux"].map((name) => loadCapture(name));
  for (const capture of captures) {
    rows.push(captureSummary(capture));
    rows.push(...normalizedObservations(capture));
    rows.push(...candidateRows(capture));
  }
  for (const capture of toolCaptures) rows.push(...toolCaptureRows(capture));

  const missing = matrixRows.filter((row) => !row.timed).map((row) => `${row.family}/${row.platform}`);
  const decision = selectParameters(rows, matrixRows, missing);
  rows.push(decision);

  const results = `${rows.map((row) => JSON.stringify(row)).join("\n")}\n`;
  const resultsDigest = digestBytes(results);
  const report = renderReport({ captures, toolCaptures, matrixRows, missing, decision, resultsDigest });
  return { results, report, rows };
}

function replayPatterns() {
  return catalogue.patterns.map((entry) => {
    const path = join(dirname(cataloguePath), entry.file);
    const pattern = json(path);
    const mount = pattern.streams.mount;
    const folder = pattern.streams.folder;
    checkStream(pattern.pattern, "mount", mount, pattern.expected.meaningful_sequence_point.mount, pattern.expected.forbidden_sequence_points.mount);
    checkStream(pattern.pattern, "folder", folder, pattern.expected.meaningful_sequence_point.folder, pattern.expected.forbidden_sequence_points.folder);
    return {
      kind: "pattern_replay",
      pattern: pattern.pattern,
      family: familyFor(pattern.category, pattern.tool.name),
      category: pattern.category,
      platform_text: pattern.tool.platform,
      provenance: entry.provenance,
      source: relative(repo, path),
      source_digest: digestFile(path),
      mount_events: mount.events.length,
      folder_events: folder.events.length,
      mount_meaningful_sequence: pattern.expected.meaningful_sequence_point.mount,
      folder_meaningful_sequence: pattern.expected.meaningful_sequence_point.folder,
      mount_forbidden_points: pattern.expected.forbidden_sequence_points.mount.length,
      folder_forbidden_points: pattern.expected.forbidden_sequence_points.folder.length,
      replay: "pass",
    };
  });
}

function matrix(patternRows, timingSources) {
  const rows = [];
  for (const [family, platforms] of Object.entries(catalogue.coverage.cells)) {
    for (const [platform, cell] of Object.entries(platforms)) {
      const replay = patternRows.find((row) => row.pattern === cell.pattern);
      if (!replay) fail(`matrix ${family}/${platform} names absent pattern ${cell.pattern}`);
      const timingSource = timingSources.get(`${family}/${platform}`) ?? null;
      const timed = timingSource !== null;
      rows.push({
        kind: "matrix_cell",
        family,
        platform,
        pattern: cell.pattern,
        observed: cell.observed,
        semantic_replay: replay.replay,
        timed,
        timing_source: timingSource,
        status: timed ? (family === "a package manager install" ? "timed-successor" : "timed-tool-capture") : "semantic-only-timing-absent",
      });
    }
  }
  if (rows.length !== manifest.required_save_pattern_cells) fail(`matrix has ${rows.length} cells, expected ${manifest.required_save_pattern_cells}`);
  return rows;
}

function loadToolCapture(configured) {
  const path = join(here, configured.path);
  const capture = json(path);
  if (digestFile(path) !== configured.digest) fail(`${configured.path}: tool capture digest changed`);
  if (capture.corpus_digest !== configured.corpus_digest) fail(`${configured.path}: corpus digest changed`);
  if (capture.capture_script_digest !== configured.capture_script_digest) fail(`${configured.path}: capture script changed`);
  if (capture.repository_commit !== configured.instrument_commit) fail(`${configured.path}: repository commit changed`);
  return { configured, path, capture };
}

function toolCaptureRows({ configured, path, capture }) {
  const platform = capture.platform.os === "darwin" ? "macOS" : "Linux";
  const rows = [{
    kind: "tool_capture",
    source: relative(repo, path),
    source_digest: configured.digest,
    corpus_digest: capture.corpus_digest,
    repository_commit: capture.repository_commit,
    capture_script_digest: capture.capture_script_digest,
    platform,
    environment: capture.platform,
    network_evidence: capture.network_evidence,
    cache_state: capture.cache_state,
    sample_count_per_arm: capture.sample_count_per_arm,
    arms: capture.arms.length,
  }];
  for (const arm of capture.arms) {
    for (const sample of arm.samples) {
      const observations = normalizedToolObservations(platform, arm, sample);
      rows.push({
        kind: "tool_sample",
        family: arm.family,
        pattern: arm.pattern,
        platform,
        sample: sample.sample,
        seed: sample.seed,
        tool_version: arm.tool_version,
        input_digest: arm.input_digest,
        invocation: sample.invocation,
        initial_tree_digest: sample.initial_tree_digest,
        final_tree_digest: sample.final_tree_digest,
        polls: sample.polls,
        observations: observations.length,
        changes: observations.reduce((sum, row) => sum + row.change_count, 0),
        changed_bytes: observations.reduce((sum, row) => sum + row.bytes_delta, 0),
        elapsed_ms: sample.elapsed_ms,
        semantic: sample.semantic,
      });
      rows.push(...observations);
      rows.push(...toolCandidateRows(platform, arm, sample, observations));
    }
  }
  return rows;
}

function normalizedToolObservations(platform, arm, sample) {
  let previous = 0;
  return sample.observations.map((observation, index) => {
    const row = {
      kind: "tool_observation",
      family: arm.family,
      pattern: arm.pattern,
      platform,
      sample: sample.sample,
      observation: index + 1,
      at_ms: observation.at_ms,
      gap_ms: round(observation.at_ms - previous),
      change_count: observation.changes.length,
      bytes_delta: observation.changes.reduce((sum, change) => sum + (change.bytes_delta ?? 0), 0),
      changes_digest: digestBytes(JSON.stringify(observation.changes)),
    };
    previous = observation.at_ms;
    return row;
  });
}

function toolCandidateRows(platform, arm, sample, observations) {
  const common = { family: arm.family, pattern: arm.pattern, platform, sample: sample.sample, sample_count: 1, uncertainty: "one polling schedule; counts are lower bounds", eligible: null, ineligible_reason: "selection not evaluated" };
  const rows = [];
  for (const candidate of manifest.candidate_grid.idle_interval_ms) {
    const gaps = observations.slice(1).map((row) => row.gap_ms);
    rows.push({ kind: "tool_candidate", ...common, parameter: "idle_interval_ms", candidate, false_meaningful_cuts: gaps.filter((gap) => gap >= candidate).length, settling_latency_ms: candidate, maximum_observed_inter_event_gap_ms: round(Math.max(0, ...gaps)) });
  }
  for (const candidate of manifest.candidate_grid.maximum_uncheckpointed_bytes) rows.push({ kind: "tool_candidate", ...common, parameter: "maximum_uncheckpointed_bytes", candidate, ...simulateBytes(observations, candidate) });
  for (const candidate of manifest.candidate_grid.maximum_uncheckpointed_time_ms) rows.push({ kind: "tool_candidate", ...common, parameter: "maximum_uncheckpointed_time_ms", candidate, ...simulateTime(observations, candidate) });
  return rows;
}

function loadCapture(name) {
  const configured = manifest.accepted_captures[name];
  const path = join(here, configured.path);
  const capture = json(path);
  if (digestFile(path) !== configured.digest) fail(`${name} capture digest changed`);
  if (!capture.semantic.verified) fail(`${name} semantic result is false`);
  if (capture.fixture.digest !== manifest.successor_fixture.input_digest) fail(`${name} fixture digest changed`);
  if (capture.tarball_digest !== manifest.accepted_captures.tarball_digest) fail(`${name} tarball digest changed`);
  if (capture.repository_commit !== manifest.accepted_captures.instrument_commit) fail(`${name} instrumentation commit changed`);
  if (capture.capture_script_digest !== manifest.accepted_captures.capture_script_digest) fail(`${name} capture script changed`);
  const required = [
    capture.platform.tag,
    capture.platform.os,
    capture.platform.os_version,
    capture.platform.arch,
    capture.platform.filesystem,
    capture.platform.node,
    capture.platform.npm,
    capture.platform.hardware.cpu_model,
    capture.platform.hardware.cpu_vendor,
    capture.platform.hardware.logical_cores,
    capture.platform.hardware.physical_cores,
    capture.platform.hardware.memory_bytes,
    capture.network_evidence,
    capture.cache_state,
  ];
  if (required.some((value) => value === null || value === undefined || value === "" || value === "unknown")) fail(`${name} environment is incomplete`);
  return { name, path, capture };
}

function captureSummary({ name, path, capture }) {
  const observations = normalizedObservations({ name, path, capture });
  return {
    kind: "successor_capture",
    platform: name,
    source: relative(repo, path),
    source_digest: digestFile(path),
    repository_commit: capture.repository_commit,
    capture_script_digest: capture.capture_script_digest,
    fixture_digest: capture.fixture.digest,
    tarball_digest: capture.tarball_digest,
    final_tree_digest: capture.final_tree_digest,
    environment: capture.platform,
    invocation: capture.command,
    network_evidence: capture.network_evidence,
    cache_state: capture.cache_state,
    polls: capture.polling.polls,
    observations: capture.observations.length,
    changes: observations.reduce((sum, row) => sum + row.change_count, 0),
    changed_bytes: observations.reduce((sum, row) => sum + row.bytes_delta, 0),
    elapsed_ms: capture.elapsed_ms,
    final_observation_at_ms: observations.at(-1)?.at_ms ?? null,
    final_to_capture_end_ms: round(capture.elapsed_ms - (observations.at(-1)?.at_ms ?? capture.elapsed_ms)),
    semantic: capture.semantic,
  };
}

function normalizedObservations({ name, capture }) {
  let previous = 0;
  return capture.observations.map((observation, index) => {
    const kinds = Object.fromEntries([...new Set(observation.changes.map((change) => change.change))].sort().map((kind) => [kind, observation.changes.filter((change) => change.change === kind).length]));
    const row = {
      kind: "successor_observation",
      platform: name,
      sample: 1,
      observation: index + 1,
      at_ms: observation.at_ms,
      gap_ms: round(observation.at_ms - previous),
      change_count: observation.changes.length,
      bytes_delta: observation.changes.reduce((sum, change) => sum + (change.bytes_delta ?? 0), 0),
      change_kinds: kinds,
      changes_digest: digestBytes(JSON.stringify(observation.changes)),
    };
    previous = observation.at_ms;
    return row;
  });
}

function candidateRows(input) {
  const observations = normalizedObservations(input);
  const rows = [];
  for (const candidate of manifest.candidate_grid.idle_interval_ms) {
    const gaps = observations.slice(1).map((row) => row.gap_ms);
    rows.push({
      kind: "candidate",
      platform: input.name,
      parameter: "idle_interval_ms",
      candidate,
      false_meaningful_cuts: gaps.filter((gap) => gap >= candidate).length,
      settling_latency_ms: candidate,
      maximum_observed_inter_event_gap_ms: round(Math.max(0, ...gaps)),
      sample_count: 1,
      uncertainty: "one accepted observation schedule; no confidence interval",
      eligible: null,
      ineligible_reason: "selection not evaluated",
    });
  }
  for (const candidate of manifest.candidate_grid.maximum_uncheckpointed_bytes) {
    const metric = simulateBytes(observations, candidate);
    rows.push({ kind: "candidate", platform: input.name, parameter: "maximum_uncheckpointed_bytes", candidate, ...metric, sample_count: 1, uncertainty: "one accepted observation schedule; no confidence interval", eligible: null, ineligible_reason: "selection not evaluated" });
  }
  for (const candidate of manifest.candidate_grid.maximum_uncheckpointed_time_ms) {
    const metric = simulateTime(observations, candidate);
    rows.push({ kind: "candidate", platform: input.name, parameter: "maximum_uncheckpointed_time_ms", candidate, ...metric, sample_count: 1, uncertainty: "one accepted observation schedule; no confidence interval", eligible: null, ineligible_reason: "selection not evaluated" });
  }
  return rows;
}

function simulateBytes(observations, candidate) {
  let atRisk = 0;
  let maximum = 0;
  let checkpoints = 0;
  for (const observation of observations) {
    atRisk += observation.bytes_delta;
    maximum = Math.max(maximum, atRisk);
    if (atRisk >= candidate) {
      checkpoints += 1;
      atRisk = 0;
    }
  }
  return { recovery_checkpoints: checkpoints, maximum_bytes_at_risk: maximum, final_bytes_at_risk: atRisk, preserved_intermediate: checkpoints > 0 };
}

function simulateTime(observations, candidate) {
  if (observations.length === 0) return { recovery_checkpoints: 0, maximum_time_at_risk_ms: 0, final_time_at_risk_ms: 0, preserved_intermediate: false };
  let dirtySince = observations[0].at_ms;
  let maximum = 0;
  let checkpoints = 0;
  for (const observation of observations.slice(1)) {
    const risk = observation.at_ms - dirtySince;
    maximum = Math.max(maximum, Math.min(risk, candidate));
    if (risk >= candidate) {
      checkpoints += 1;
      dirtySince = observation.at_ms;
    }
  }
  const finalRisk = observations.at(-1).at_ms - dirtySince;
  return { recovery_checkpoints: checkpoints, maximum_time_at_risk_ms: round(maximum), final_time_at_risk_ms: round(finalRisk), preserved_intermediate: checkpoints > 0 };
}

function selectParameters(rows, matrixRows, missing) {
  const selected = {
    idle_interval_ms: null,
    maximum_uncheckpointed_bytes: null,
    maximum_uncheckpointed_time_ms: null,
  };
  const candidates = rows.filter((row) => row.kind === "candidate" || row.kind === "tool_candidate");
  if (missing.length === 0) {
    selected.idle_interval_ms = manifest.candidate_grid.idle_interval_ms.find((candidate) =>
      candidates.filter((row) => row.parameter === "idle_interval_ms" && row.candidate === candidate)
        .every((row) => row.false_meaningful_cuts === 0)) ?? null;
    for (const parameter of ["maximum_uncheckpointed_bytes", "maximum_uncheckpointed_time_ms"]) {
      selected[parameter] = manifest.candidate_grid[parameter].find((candidate) => {
        const matching = candidates.filter((row) => row.parameter === parameter && row.candidate === candidate);
        const platformP95Passes = ["macOS", "Linux"].every((platform) => {
          const counts = matching.filter((row) => normalizedPlatform(row.platform) === platform).map((row) => row.recovery_checkpoints).sort((a, b) => a - b);
          return counts.length > 0 && percentile(counts, 0.95) <= 32;
        });
        const successorsPreserved = matching.filter((row) => row.kind === "candidate").every((row) => row.preserved_intermediate);
        return platformP95Passes && successorsPreserved;
      }) ?? null;
    }
  }
  for (const row of candidates) {
    const chosen = selected[row.parameter];
    row.eligible = chosen !== null && row.candidate === chosen;
    row.ineligible_reason = row.eligible ? null : missing.length > 0 ? "full timed matrix absent" : "not selected by frozen rule";
  }
  const complete = missing.length === 0 && Object.values(selected).every((value) => value !== null);
  return {
    kind: "decision",
    status: complete ? "selected" : missing.length > 0 ? "unresolved-incomplete-matrix" : "unresolved-no-eligible-candidate",
    selected,
    required_timed_cells: manifest.required_save_pattern_cells,
    timed_cells: matrixRows.length - missing.length,
    missing_timed_cells: missing,
    reason: complete
      ? "All twelve timed cells exist and each value is the smallest candidate satisfying its frozen rule."
      : missing.length > 0
        ? "The frozen rule forbids selection until all twelve save-pattern cells have timed normalized streams."
        : "The full timed matrix exists, but at least one frozen rule has no eligible candidate.",
  };
}

function normalizedPlatform(platform) {
  if (platform === "macos" || platform === "macOS") return "macOS";
  if (platform === "linux" || platform === "Linux") return "Linux";
  return platform;
}

function renderReport({ captures, toolCaptures, matrixRows, missing, decision, resultsDigest }) {
  const summaries = captures.map(captureSummary);
  const table = summaries.map((row) => `| ${row.platform} | ${row.environment.os} ${row.environment.os_version} / ${row.environment.filesystem} | ${row.environment.node} / npm ${row.environment.npm} | ${row.polls.toLocaleString("en-US")} | ${row.observations} | ${row.changes.toLocaleString("en-US")} | ${row.elapsed_ms.toFixed(3)} |`).join("\n");
  const allCandidates = captures.flatMap(candidateRows);
  const cell = (platform, parameter, candidate) => allCandidates.find((row) => row.platform === platform && row.parameter === parameter && row.candidate === candidate);
  const idleTable = manifest.candidate_grid.idle_interval_ms.map((candidate) => {
    const mac = cell("macos", "idle_interval_ms", candidate);
    const linux = cell("linux", "idle_interval_ms", candidate);
    return `| ${candidate} | ${mac.false_meaningful_cuts} | ${linux.false_meaningful_cuts} | ${mac.maximum_observed_inter_event_gap_ms.toFixed(3)} | ${linux.maximum_observed_inter_event_gap_ms.toFixed(3)} |`;
  }).join("\n");
  const byteTable = manifest.candidate_grid.maximum_uncheckpointed_bytes.map((candidate) => {
    const mac = cell("macos", "maximum_uncheckpointed_bytes", candidate);
    const linux = cell("linux", "maximum_uncheckpointed_bytes", candidate);
    return `| ${candidate} | ${mac.recovery_checkpoints} | ${linux.recovery_checkpoints} | ${mac.maximum_bytes_at_risk} | ${linux.maximum_bytes_at_risk} |`;
  }).join("\n");
  const timeTable = manifest.candidate_grid.maximum_uncheckpointed_time_ms.map((candidate) => {
    const mac = cell("macos", "maximum_uncheckpointed_time_ms", candidate);
    const linux = cell("linux", "maximum_uncheckpointed_time_ms", candidate);
    return `| ${candidate} | ${mac.recovery_checkpoints} | ${linux.recovery_checkpoints} | ${mac.maximum_time_at_risk_ms.toFixed(3)} | ${linux.maximum_time_at_risk_ms.toFixed(3)} |`;
  }).join("\n");
  const toolSamples = toolCaptures.flatMap((input) => input.capture.arms.flatMap((arm) => arm.samples.map((sample) => ({ family: arm.family, platform: input.capture.platform.os === "darwin" ? "macOS" : "Linux", sample }))));
  const toolSummary = [...new Set(toolSamples.map((row) => `${row.family}\u0000${row.platform}`))].map((key) => {
    const [family, platform] = key.split("\u0000");
    const samples = toolSamples.filter((row) => row.family === family && row.platform === platform).map((row) => row.sample);
    const elapsed = samples.map((row) => row.elapsed_ms).sort((a, b) => a - b);
    const observations = samples.reduce((sum, row) => sum + row.observations.length, 0);
    return `| ${family} | ${platform} | ${samples.length} | ${observations} | ${elapsed[0].toFixed(3)} | ${percentile(elapsed, 0.5).toFixed(3)} | ${percentile(elapsed, 0.95).toFixed(3)} | ${elapsed.at(-1).toFixed(3)} |`;
  }).join("\n");
  const selected = decision.status === "selected";
  const decisionText = selected
    ? `**Selected from the complete twelve-cell timed matrix.** Every value is the smallest predeclared candidate satisfying its frozen rule.`
    : `**Unresolved. No complete scheduler parameter set is selected.** ${decision.reason}`;
  return `# TASK-347 settling measurement report\n\n## Decision\n\n${decisionText}\n\n- idle interval: ${decision.selected.idle_interval_ms ?? "unresolved"} ms\n- maximum uncheckpointed bytes: ${decision.selected.maximum_uncheckpointed_bytes ?? "unresolved"}\n- maximum uncheckpointed time: ${decision.selected.maximum_uncheckpointed_time_ms ?? "unresolved"} ms\n- timed cells: ${matrixRows.length - missing.length}/${matrixRows.length}\n- missing timing: ${missing.length === 0 ? "none" : missing.join(", ")}\n\nThe historical 12,225-change npm observation remains aggregate provenance only and was not read as a selection row. The candidate grid and rule were frozen before the comparative revision-2 captures were read.\n\n## Full-scale successor captures\n\n| Platform | OS / filesystem | Runtime | Polls | Observations | Changes | Elapsed ms |\n| --- | --- | --- | ---: | ---: | ---: | ---: |\n${table}\n\nBoth captures use fixture ${manifest.successor_fixture.input_digest} and tarball ${manifest.accepted_captures.tarball_digest}. Both installed all 2,048 files, produced both lockfiles and left the consumer naming the dependency. Different final-tree digests are expected because the pinned npm versions write platform/version-specific metadata.\n\n## Tool captures\n\n| Family | Platform | Samples | Observations | Min ms | Median ms | p95 ms | Max ms |\n| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |\n${toolSummary}\n\nEvery tool sample carries its exact input, corpus, script, environment, invocation and final-tree digest in the raw capture and normalized JSONL. All ten editor, version-control, and formatter cells are timed; the two package-manager cells come from the full-scale successor captures.\n\n## Package-manager candidate distributions\n\n### Idle interval\n\n| Candidate ms | macOS false cuts | Linux false cuts | macOS max gap ms | Linux max gap ms |\n| ---: | ---: | ---: | ---: | ---: |\n${idleTable}\n\n### Maximum uncheckpointed bytes\n\n| Candidate bytes | macOS recovery checkpoints | Linux recovery checkpoints | macOS max bytes at risk | Linux max bytes at risk |\n| ---: | ---: | ---: | ---: | ---: |\n${byteTable}\n\n### Maximum uncheckpointed time\n\n| Candidate ms | macOS recovery checkpoints | Linux recovery checkpoints | macOS max time at risk ms | Linux max time at risk ms |\n| ---: | ---: | ---: | ---: | ---: |\n${timeTable}\n\n## What the rows contain\n\n\`results.jsonl\` contains all 16 corpus replay results, all 12 required matrix cells, every normalized successor and tool observation, every predeclared candidate for every acquired sample, and the exact decision. Its digest is ${resultsDigest}.\n\nFalse meaningful-save cuts, recovery-preservation frequency, and bytes/time at risk remain separate columns. Only the globally selected candidate in each dimension is marked eligible. Polling can miss changes completed between readings, so counts are lower bounds on filesystem activity and no reliability claim is made.\n\n## Reproduction\n\n\`node benchmarks/checkpoint-settling/analyze.mjs --check\` regenerates the exact JSONL and report in memory. \`node benchmarks/checkpoint-settling/verify.mjs --full --mutations\` verifies capture/environment/corpus digests, the complete candidate grid, fail-closed selection and planted defects. Exact host and network-disabled Docker acquisition recipes are in \`manifest.json\`.\n`;
}

function percentile(sorted, fraction) {
  if (sorted.length === 0) return 0;
  return sorted[Math.min(sorted.length - 1, Math.ceil(sorted.length * fraction) - 1)];
}

function checkStream(pattern, name, stream, meaningful, forbidden) {
  if (!Array.isArray(stream.events) || stream.events.length === 0) fail(`${pattern}/${name}: empty stream`);
  const sequences = stream.events.map((event) => event.sequence);
  if (sequences.some((sequence, index) => sequence !== index + 1)) fail(`${pattern}/${name}: non-dense sequence`);
  if (meaningful !== sequences.at(-1)) fail(`${pattern}/${name}: meaningful point is not final`);
  if (forbidden.includes(meaningful) || forbidden.some((sequence) => !sequences.includes(sequence))) fail(`${pattern}/${name}: invalid forbidden point`);
}

function familyFor(category, tool) {
  if (category === "editor" && /Visual Studio Code/i.test(tool)) return "VS Code";
  if (category === "editor" && /IntelliJ|JetBrains/i.test(tool)) return "a JetBrains IDE";
  if (category === "editor") return "vim or neovim";
  if (category === "version-control") return "Git operations";
  if (category === "formatter") return "a formatter";
  if (category === "package-manager") return "a package manager install";
  return category;
}

function json(path) { return JSON.parse(readFileSync(path, "utf8")); }
function digestFile(path) { return digestBytes(readFileSync(path)); }
function digestBytes(bytes) { return `sha256:${createHash("sha256").update(bytes).digest("hex")}`; }
function round(number) { return Math.round(number * 1000) / 1000; }
function fail(message) { throw new Error(`checkpoint-settling analysis: ${message}`); }

if (fileURLToPath(import.meta.url) === process.argv[1]) {
  const built = buildArtifacts();
  const resultsPath = join(here, "results.jsonl");
  const reportPath = join(here, "report.md");
  if (process.argv.includes("--check")) {
    if (readFileSync(resultsPath, "utf8") !== built.results) fail("results.jsonl is stale");
    if (readFileSync(reportPath, "utf8") !== built.report) fail("report.md is stale");
    process.stdout.write(`checkpoint-settling analysis: PASS — ${built.rows.length} rows\n`);
  } else {
    writeFileSync(resultsPath, built.results);
    writeFileSync(reportPath, built.report);
    process.stdout.write(`checkpoint-settling analysis: wrote ${built.rows.length} rows\n`);
  }
}
