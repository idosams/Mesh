#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const here = dirname(fileURLToPath(import.meta.url));
const patternsDirectory = join(here, "patterns");
const catalogue = JSON.parse(readFileSync(join(here, "catalogue.json"), "utf8"));
const vocabulary = JSON.parse(
  readFileSync(join(here, "..", "..", "adapter", "v0", "vocabulary.json"), "utf8"),
);

const EVENT_KINDS = new Set(vocabulary.event_kinds);
const BOUNDARY_REASONS = new Set(vocabulary.boundary_reasons);
const OBSERVATION_CONFIDENCE = new Set([
  "ExactIntegratedRead",
  "ExactFilesystemRange",
  "FilesystemReadAhead",
  "ProcessInferred",
  "RecoveryDetected",
  "Unknown",
]);

function loadPatterns() {
  return readdirSync(patternsDirectory)
    .filter((name) => name.endsWith(".json"))
    .sort()
    .map((name) => JSON.parse(readFileSync(join(patternsDirectory, name), "utf8")));
}

function assertDenseAscendingEvents(pattern, streamName, stream) {
  const sequences = stream.events.map((event) => event.sequence);
  assert.deepEqual(
    sequences,
    Array.from({ length: sequences.length }, (_, index) => index + 1),
    `${pattern.pattern}/${streamName}: events must be dense and ascending`,
  );
  for (const event of stream.events) {
    assert(EVENT_KINDS.has(event.kind), `${pattern.pattern}/${streamName}: unknown event kind ${event.kind}`);
  }
}

function assertCandidates(pattern, streamName, stream) {
  const eventSequences = new Set(stream.events.map((event) => event.sequence));
  let previous = 0;
  for (const candidate of stream.candidates) {
    assert(eventSequences.has(candidate.through), `${pattern.pattern}/${streamName}: candidate is outside stream`);
    assert(candidate.through > previous, `${pattern.pattern}/${streamName}: candidates must be ascending`);
    assert(BOUNDARY_REASONS.has(candidate.reason), `${pattern.pattern}/${streamName}: unknown boundary reason`);
    previous = candidate.through;
  }
}

function checkpointAt(stream, through) {
  const visibleEvents = stream.events.filter((event) => event.sequence <= through);
  if (visibleEvents.length === 0) return [];
  const lastCandidate = stream.candidates.filter((candidate) => candidate.through <= through).at(-1);
  return [
    {
      from: visibleEvents[0].sequence,
      through,
      reason: lastCandidate?.reason ?? null,
    },
  ];
}

function admissiblePositions(stream, trigger) {
  if (trigger.admissible_positions === "after-each-event") {
    return stream.events.map((event) => event.sequence);
  }
  if (trigger.admissible_positions === "after-final-event") {
    return stream.events.length === 0 ? [] : [stream.events.at(-1).sequence];
  }
  assert.fail(`unknown admissible_positions ${trigger.admissible_positions}`);
}

function replayTrigger(stream, trigger, through) {
  const eventSequences = new Set(stream.events.map((event) => event.sequence));
  assert(eventSequences.has(through), `${trigger.name} trigger ${through} is outside stream`);
  if (trigger.effect === "preserve-recovery-only") {
    return [];
  }
  assert.equal(trigger.effect, "emit-meaningful-checkpoint", `${trigger.name}: unknown trigger effect`);
  // A trigger can only emit the state that exists when it fires. It must not read events that
  // follow `through`: doing so made a process exit at JetBrains sequence 6 look like [1, 8].
  return checkpointAt(stream, through);
}

function mutantCandidateAnchored(stream, trigger) {
  const candidate = stream.candidates.filter(({ through }) => through <= trigger).at(-1);
  if (!candidate) return [];
  return [{ from: stream.events[0].sequence, through: candidate.through, reason: candidate.reason }];
}

function expectedForbidden(pattern, streamName, stream) {
  const meaningful = pattern.expected.meaningful_sequence_point[streamName];
  return stream.events.map(({ sequence }) => sequence).filter((sequence) => sequence !== meaningful);
}

function declaredTriggers() {
  const triggers = catalogue.rule.triggers;
  assert(Array.isArray(triggers) && triggers.length > 0, "catalogue must declare trigger semantics");
  assert.equal(new Set(triggers.map(({ name }) => name)).size, triggers.length, "trigger names must be unique");
  return triggers;
}

function checkTriggerSchedules(pattern, streamName, stream, expected) {
  const forbidden = new Set(pattern.expected.forbidden_sequence_points[streamName]);
  for (const trigger of declaredTriggers()) {
    for (const through of admissiblePositions(stream, trigger)) {
      const actual = replayTrigger(stream, trigger, through);
      if (trigger.effect === "emit-meaningful-checkpoint") {
        assert.deepEqual(
          actual,
          expected.checkpoints,
          `${pattern.pattern}/${streamName}: ${trigger.name} at ${through} emitted a non-meaningful checkpoint`,
        );
      } else {
        assert.deepEqual(
          actual,
          [],
          `${pattern.pattern}/${streamName}: ${trigger.name} at ${through} claimed recovery data was a meaningful checkpoint`,
        );
      }
      for (const checkpoint of actual) {
        assert(
          !forbidden.has(checkpoint.through),
          `${pattern.pattern}/${streamName}: ${trigger.name} emitted forbidden point ${checkpoint.through}`,
        );
      }
    }
  }
}

function checkPattern(pattern) {
  assert.equal(pattern.contract, "mesh-save-patterns/0");
  for (const streamName of ["mount", "folder"]) {
    const stream = pattern.streams[streamName];
    const expected = pattern.expected[streamName];
    assertDenseAscendingEvents(pattern, streamName, stream);
    assertCandidates(pattern, streamName, stream);
    assert.deepEqual(
      pattern.expected.forbidden_sequence_points[streamName],
      expectedForbidden(pattern, streamName, stream),
      `${pattern.pattern}/${streamName}: forbidden points must be every non-meaningful point`,
    );
    assert(OBSERVATION_CONFIDENCE.has(expected.boundary_evidence.confidence));
    const lastCandidate = stream.candidates.at(-1);
    assert.equal(expected.boundary_evidence.through, lastCandidate?.through ?? null);
    checkTriggerSchedules(pattern, streamName, stream, expected);

    if (streamName === "folder") {
      for (const omissions of stream.admissible_omissions) {
        const retained = {
          ...stream,
          events: stream.events.filter(({ sequence }) => !omissions.includes(sequence)),
        };
        const through = retained.events.at(-1).sequence;
        const settledTrigger = declaredTriggers().find(({ name }) => name === "actor-becomes-idle-after-settling");
        const checkpoints = replayTrigger(retained, settledTrigger, through);
        assert.equal(checkpoints.length, 1);
        assert.equal(checkpoints[0].through, through);
      }
    }
  }
}

function mutationEvidence(patterns) {
  const candidateFailures = [];
  const processExitFailures = [];
  const processExit = declaredTriggers().find(({ name }) => name === "actor-process-exits");
  assert(processExit, "catalogue must declare actor-process-exits");
  const processExitMeaningful = { ...processExit, effect: "emit-meaningful-checkpoint" };
  for (const pattern of patterns) {
    for (const streamName of ["mount", "folder"]) {
      const stream = pattern.streams[streamName];
      const forbidden = new Set(pattern.expected.forbidden_sequence_points[streamName]);
      for (const event of stream.events) {
        for (const checkpoint of mutantCandidateAnchored(stream, event.sequence)) {
          if (forbidden.has(checkpoint.through)) {
            candidateFailures.push({ pattern: pattern.pattern, stream: streamName, trigger: event.sequence, through: checkpoint.through });
          }
        }
      }
      for (const through of admissiblePositions(stream, processExitMeaningful)) {
        for (const checkpoint of replayTrigger(stream, processExitMeaningful, through)) {
          if (forbidden.has(checkpoint.through)) {
            processExitFailures.push({
              pattern: pattern.pattern,
              stream: streamName,
              trigger: through,
              from: checkpoint.from,
              through: checkpoint.through,
              reason: checkpoint.reason,
            });
          }
        }
      }
    }
  }
  assert(
    candidateFailures.some(({ pattern, stream, trigger, through }) =>
      pattern === "jetbrains-safe-write" && stream === "mount" && trigger === 6 && through === 6),
    "candidate-anchor mutation did not reproduce the JetBrains sequence-6 forbidden emission",
  );
  assert(candidateFailures.length >= 22, `expected at least the cold-observed 22 candidate failures, got ${candidateFailures.length}`);
  assert(
    processExitFailures.some(({ pattern, stream, trigger, from, through, reason }) =>
      pattern === "jetbrains-safe-write" && stream === "mount" && trigger === 6
      && from === 1 && through === 6 && reason === "RenamedIntoPlace"),
    "process-exit mutation did not reproduce the JetBrains sequence-6 [1, 6] forbidden emission",
  );
  assert(processExitFailures.length > 0, "process-exit mutation did not emit any forbidden checkpoints");
  return { candidateFailures, processExitFailures };
}

function emit(patterns) {
  const settledTrigger = declaredTriggers().find(({ name }) => name === "actor-becomes-idle-after-settling");
  const output = patterns.map((pattern) => ({
    pattern: pattern.pattern,
    mount: replayTrigger(pattern.streams.mount, settledTrigger, pattern.streams.mount.events.at(-1).sequence),
    folder: replayTrigger(pattern.streams.folder, settledTrigger, pattern.streams.folder.events.at(-1).sequence),
  }));
  return JSON.stringify(output);
}

const patterns = loadPatterns();
if (process.argv.includes("--emit")) {
  process.stdout.write(`${emit(patterns)}\n`);
} else {
  for (const pattern of patterns) checkPattern(pattern);
  const inProcessOne = emit(patterns);
  const inProcessTwo = emit(patterns);
  assert.equal(inProcessOne, inProcessTwo, "in-process replay changed");
  const child = spawnSync(process.execPath, [fileURLToPath(import.meta.url), "--emit"], {
    encoding: "utf8",
    env: process.env,
  });
  assert.equal(child.status, 0, child.stderr);
  assert.equal(child.stdout.trim(), inProcessOne, "cross-process replay changed");
  const { candidateFailures, processExitFailures } = mutationEvidence(patterns);
  process.stdout.write(
    `save-pattern replay: ${patterns.length} patterns, 2 streams, every declared trigger schedule passes; candidate-anchor mutation rejected at ${candidateFailures.length} forbidden emissions; process-exit mutation rejected at ${processExitFailures.length} forbidden emissions\n`,
  );
}
