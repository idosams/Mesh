import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { scan } from "./scan.mjs";

function cleanBundle() {
  return {
    schema: "mesh-support-bundle/v1",
    producer: { component: "mesh-daemon", version: "0.0.0" },
    workspace_correlation: `blake3:${"ab".repeat(32)}`,
    included: ["crash-diagnostics"],
    excluded: ["configuration", "event-ledger", "file-content", "key-material", "raw-paths"],
    "crash-diagnostics": {
      section: "crash-diagnostics",
      serving: true,
      severity: "routine",
      saved_records: 2,
      boundary_bytes: 128,
      unfinished_bytes: 0,
      checkpoint_state_available: false,
      meaningful_checkpoint_through: null,
      recovery_preserved_through: null,
      open_activity_from: null,
      open_activity_through: null,
      elapsed_ms: 3,
      sentence: "Mesh started and your workspace is up to date. 2 saved changes were read back.",
    },
  };
}

test("accepts the exact safe schema", () => {
  assert.equal(scan(cleanBundle()).schema, "mesh-support-bundle/v1");
});

test("preserves the full u64 checkpoint sequence domain as canonical decimal strings", () => {
  const bundle = cleanBundle();
  bundle["crash-diagnostics"].checkpoint_state_available = true;
  bundle["crash-diagnostics"].meaningful_checkpoint_through = "18446744073709551615";
  bundle["crash-diagnostics"].recovery_preserved_through = "18446744073709551615";
  assert.equal(scan(bundle), bundle);

  for (const invalid of [9007199254740992, "01", "0", "18446744073709551616"]) {
    const mutation = cleanBundle();
    mutation["crash-diagnostics"].checkpoint_state_available = true;
    mutation["crash-diagnostics"].meaningful_checkpoint_through = invalid;
    assert.throws(
      () => scan(mutation),
      /canonical positive u64 decimal string|exceeds u64/,
      `accepted lossy or noncanonical checkpoint sequence ${invalid}`,
    );
  }
});

test("rejects an unknown field rather than guessing whether it is safe", () => {
  const bundle = cleanBundle();
  bundle.configuration = { token: "sk-planted-secret" };
  assert.throws(() => scan(bundle), /keys must be exactly/);
});

test("rejects a raw workspace path in place of its correlation digest", () => {
  const bundle = cleanBundle();
  bundle.workspace_correlation = "/Users/alice/private-project";
  assert.throws(() => scan(bundle), /BLAKE3 digest/);
});

test("rejects file content smuggled into an otherwise allowed sentence field", () => {
  const bundle = cleanBundle();
  bundle["crash-diagnostics"].sentence = "quarterly-plan.txt says acquire Example Corp";
  assert.throws(() => scan(bundle), /sanitized message set/);
});

test("rejects key material smuggled into an otherwise allowed producer field", () => {
  const bundle = cleanBundle();
  // Assemble the forbidden bytes only in memory: the OSS boundary scans this
  // tracked fixture too, so committing the exact planted header would be the
  // leak this test is meant to prevent.
  bundle.producer.version = ["-----BEGIN", "PRIVATE", "KEY-----"].join(" ");
  assert.throws(() => scan(bundle), /must exactly match the scanner version/);
});

test("rejects encoded private data in semver build metadata", () => {
  const bundle = cleanBundle();
  bundle.producer.version = "0.0.0+private-project-alpha";
  assert.throws(
    () => scan(bundle),
    /must exactly match the scanner version/,
    "SemVer metadata is free-form data and must not cross the support boundary",
  );
});

test("rejects an unverified data class unless it remains explicitly excluded", () => {
  const bundle = cleanBundle();
  bundle.excluded = bundle.excluded.filter((name) => name !== "event-ledger");
  assert.throws(() => scan(bundle), /must contain exactly/);
});

test("rejects contradictory crash-diagnostic verdicts", () => {
  const blockingAsServing = cleanBundle();
  blockingAsServing["crash-diagnostics"].sentence =
    "Mesh could not open your workspace and has changed nothing. Your saved work is still on this " +
    "device. Quit Mesh and start it again; if this message comes back, the workspace needs " +
    "attention before it can be used.";
  assert.throws(() => scan(blockingAsServing), /contradictory outcome fields/);

  const interruptedAsRoutine = cleanBundle();
  interruptedAsRoutine["crash-diagnostics"].unfinished_bytes = 17;
  assert.throws(() => scan(interruptedAsRoutine), /contradictory outcome fields/);

  const fastCleanAsNotable = cleanBundle();
  fastCleanAsNotable["crash-diagnostics"].severity = "notable";
  assert.throws(
    () => scan(fastCleanAsNotable),
    /recovery budget verdict is inconsistent/,
  );

  const slowCleanAsRoutine = cleanBundle();
  slowCleanAsRoutine["crash-diagnostics"].elapsed_ms = 5_000;
  assert.throws(
    () => scan(slowCleanAsRoutine),
    /recovery budget verdict is inconsistent/,
  );
});

test("accepts a known torn tail when a later unsafe layout blocks serving", () => {
  const bundle = cleanBundle();
  const crash = bundle["crash-diagnostics"];
  crash.serving = false;
  crash.severity = "blocking";
  crash.saved_records = 1;
  crash.boundary_bytes = 64;
  crash.unfinished_bytes = 4;
  crash.sentence =
    "Mesh could not open your workspace and has changed nothing. Your saved work is still on this " +
    "device. Quit Mesh and start it again; if this message comes back, the workspace needs " +
    "attention before it can be used.";

  assert.equal(
    scan(bundle),
    bundle,
    "the scanner must accept the producer's fail-closed unsafe-layout report without erasing its observed torn tail",
  );
});

test("rejects impossible durable-boundary and open-window shapes", () => {
  const missingBoundary = cleanBundle();
  missingBoundary["crash-diagnostics"].boundary_bytes = 0;
  assert.throws(() => scan(missingBoundary), /durable boundary is inconsistent/);

  const impossibleFrameDensity = cleanBundle();
  impossibleFrameDensity["crash-diagnostics"].saved_records = 4;
  impossibleFrameDensity["crash-diagnostics"].sentence =
    "Mesh started and your workspace is up to date. 4 saved changes were read back.";
  assert.throws(
    () => scan(impossibleFrameDensity),
    /durable boundary cannot contain the claimed records/,
    "the scanner accepted more records than the framed journal boundary can physically contain",
  );

  const beyondProducerLimit = cleanBundle();
  beyondProducerLimit["crash-diagnostics"].boundary_bytes = 64 * 1024 * 1024 + 1;
  assert.throws(
    () => scan(beyondProducerLimit),
    /exceeds the producer journal inspection limit/,
    "the scanner accepted a serving report the bounded producer cannot emit",
  );

  const reversedWindow = cleanBundle();
  reversedWindow["crash-diagnostics"].checkpoint_state_available = true;
  reversedWindow["crash-diagnostics"].open_activity_from = "8";
  reversedWindow["crash-diagnostics"].open_activity_through = "7";
  assert.throws(() => scan(reversedWindow), /open activity window is inconsistent/);
});

test("the exported scanner can be imported from an eval host", () => {
  const scanner = new URL("./scan.mjs", import.meta.url).href;
  const imported = spawnSync(
    process.execPath,
    ["--input-type=module", "--eval", `await import(${JSON.stringify(scanner)})`],
    { encoding: "utf8" },
  );
  assert.equal(imported.status, 0, imported.stderr);
});

test("enforces the checkpoint relationships of the durable recovery state", () => {
  for (const name of [
    "meaningful_checkpoint_through",
    "recovery_preserved_through",
    "open_activity_from",
    "open_activity_through",
  ]) {
    const zeroSequence = cleanBundle();
    zeroSequence["crash-diagnostics"].checkpoint_state_available = true;
    zeroSequence["crash-diagnostics"][name] = "0";
    if (name === "open_activity_from") {
      zeroSequence["crash-diagnostics"].open_activity_through = "1";
    } else if (name === "open_activity_through") {
      zeroSequence["crash-diagnostics"].open_activity_from = "1";
    }
    assert.throws(
      () => scan(zeroSequence),
      /must be a canonical positive u64 decimal string/,
      `${name} accepted sequence zero even though the producer uses NonZeroU64`,
    );
  }

  const recoveryOutsideWindow = cleanBundle();
  recoveryOutsideWindow["crash-diagnostics"].checkpoint_state_available = true;
  recoveryOutsideWindow["crash-diagnostics"].recovery_preserved_through = "4";
  recoveryOutsideWindow["crash-diagnostics"].open_activity_from = "5";
  recoveryOutsideWindow["crash-diagnostics"].open_activity_through = "7";
  assert.throws(
    () => scan(recoveryOutsideWindow),
    /recovery prefix must belong to the current window or prior meaningful state/,
  );

  const orphanedRecovery = cleanBundle();
  orphanedRecovery["crash-diagnostics"].checkpoint_state_available = true;
  orphanedRecovery["crash-diagnostics"].recovery_preserved_through = "4";
  assert.throws(
    () => scan(orphanedRecovery),
    /recovery prefix without an open window exceeds meaningful state/,
  );

  const overlappingMeaningful = cleanBundle();
  overlappingMeaningful["crash-diagnostics"].checkpoint_state_available = true;
  overlappingMeaningful["crash-diagnostics"].meaningful_checkpoint_through = "5";
  overlappingMeaningful["crash-diagnostics"].open_activity_from = "5";
  overlappingMeaningful["crash-diagnostics"].open_activity_through = "7";
  assert.throws(
    () => scan(overlappingMeaningful),
    /meaningful checkpoint must precede the open activity window/,
  );

  const recoveryInGap = cleanBundle();
  recoveryInGap["crash-diagnostics"].checkpoint_state_available = true;
  recoveryInGap["crash-diagnostics"].meaningful_checkpoint_through = "3";
  recoveryInGap["crash-diagnostics"].recovery_preserved_through = "4";
  recoveryInGap["crash-diagnostics"].open_activity_from = "5";
  recoveryInGap["crash-diagnostics"].open_activity_through = "7";
  assert.throws(
    () => scan(recoveryInGap),
    /recovery prefix must belong to the current window or prior meaningful state/,
  );

  const recoveryBeyondClosedCheckpoint = cleanBundle();
  recoveryBeyondClosedCheckpoint["crash-diagnostics"].checkpoint_state_available = true;
  recoveryBeyondClosedCheckpoint["crash-diagnostics"].meaningful_checkpoint_through = "4";
  recoveryBeyondClosedCheckpoint["crash-diagnostics"].recovery_preserved_through = "5";
  assert.throws(
    () => scan(recoveryBeyondClosedCheckpoint),
    /recovery prefix without an open window exceeds meaningful state/,
  );

  const retainedPriorRecovery = cleanBundle();
  retainedPriorRecovery["crash-diagnostics"].checkpoint_state_available = true;
  retainedPriorRecovery["crash-diagnostics"].meaningful_checkpoint_through = "4";
  retainedPriorRecovery["crash-diagnostics"].recovery_preserved_through = "3";
  retainedPriorRecovery["crash-diagnostics"].open_activity_from = "5";
  retainedPriorRecovery["crash-diagnostics"].open_activity_through = "7";
  assert.equal(
    scan(retainedPriorRecovery),
    retainedPriorRecovery,
    "the scanner rejected a valid prior recovery retained beneath meaningful state",
  );

  const checkpointOnFailure = cleanBundle();
  checkpointOnFailure["crash-diagnostics"].serving = false;
  checkpointOnFailure["crash-diagnostics"].severity = "blocking";
  checkpointOnFailure["crash-diagnostics"].checkpoint_state_available = true;
  checkpointOnFailure["crash-diagnostics"].sentence =
    "Mesh could not open your workspace and has changed nothing. Your saved work is still on this " +
    "device. Quit Mesh and start it again; if this message comes back, the workspace needs " +
    "attention before it can be used.";
  assert.throws(
    () => scan(checkpointOnFailure),
    /checkpoint state cannot be available for a non-serving report/,
    "the scanner accepted checkpoint state that CrashReport::of_failure cannot emit",
  );
});

test("rejects literal and escaped duplicate JSON keys before a secret can be collapsed", async () => {
  const root = await mkdtemp(join(tmpdir(), "mesh-support-scanner-"));
  try {
    const marker = '"sentence":';
    const secret = ["-----BEGIN", "PRIVATE", "KEY-----"].join(" ");
    const encoded = JSON.stringify(cleanBundle());
    const mutations = [
      ["literal", `${marker}${JSON.stringify(secret)},${marker}`],
      ["escaped", `"sent\\u0065nce":${JSON.stringify(secret)},${marker}`],
    ];
    for (const [name, replacement] of mutations) {
      const bundle = join(root, `${name}.json`);
      await writeFile(bundle, encoded.replace(marker, replacement));
      const result = spawnSync(
        process.execPath,
        [fileURLToPath(new URL("./scan.mjs", import.meta.url)), "--bundle", bundle],
        { encoding: "utf8" },
      );

      assert.equal(result.status, 1, `${name}: ${result.stdout}`);
      assert.match(result.stderr, /duplicate JSON key/, name);
    }
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("rejects noncanonical JSON whitespace that can carry excluded data", async () => {
  const root = await mkdtemp(join(tmpdir(), "mesh-support-scanner-canonical-"));
  try {
    const secretBits = Array.from(Buffer.from("private workspace note"))
      .flatMap((byte) => Array.from({ length: 8 }, (_, bit) => (byte >> bit) & 1))
      .map((bit) => (bit === 0 ? " " : "\t"))
      .join("");
    const bundle = join(root, "noncanonical.json");
    await writeFile(bundle, `${secretBits}${JSON.stringify(cleanBundle())}\n`);
    const result = spawnSync(
      process.execPath,
      [fileURLToPath(new URL("./scan.mjs", import.meta.url)), "--bundle", bundle],
      { encoding: "utf8" },
    );

    assert.equal(result.status, 1, result.stdout);
    assert.match(result.stderr, /canonical JSON/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("refuses an oversized bundle before parsing it", async () => {
  const root = await mkdtemp(join(tmpdir(), "mesh-support-scanner-size-"));
  try {
    const bundle = join(root, "oversized.json");
    await writeFile(bundle, " ".repeat(65_537));
    const result = spawnSync(
      process.execPath,
      [fileURLToPath(new URL("./scan.mjs", import.meta.url)), "--bundle", bundle],
      { encoding: "utf8" },
    );

    assert.equal(result.status, 1, result.stdout);
    assert.match(result.stderr, /exceeds the 65536-byte limit/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test(
  "refuses a fifo without waiting for a writer",
  { skip: process.platform === "win32" },
  async () => {
    const root = await mkdtemp(join(tmpdir(), "mesh-support-scanner-fifo-"));
    try {
      const bundle = join(root, "bundle.pipe");
      const created = spawnSync("mkfifo", [bundle], { encoding: "utf8" });
      assert.equal(created.status, 0, created.stderr);

      const result = spawnSync(
        process.execPath,
        [fileURLToPath(new URL("./scan.mjs", import.meta.url)), "--bundle", bundle],
        { encoding: "utf8", timeout: 1_000 },
      );

      assert.equal(result.error, undefined, "scanner blocked on a non-regular input");
      assert.equal(result.status, 1, result.stdout);
      assert.match(result.stderr, /must be a regular file/);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  },
);
