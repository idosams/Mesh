#!/usr/bin/env node

import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statfsSync, writeFileSync } from "node:fs";
import { cpus, totalmem, tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import process from "node:process";
import { fileURLToPath } from "node:url";

import { FIXTURE, fixtureDigest, materializeFixture } from "./fixture.mjs";
import { fixtureSemanticOutcome } from "./semantic.mjs";

const args = process.argv.slice(2);
const out = required("--out");
const platformTag = required("--platform");
const commit = required("--commit");
const npmCommand = value("--npm") ?? "npm";
const expectedDigest = required("--fixture-digest");
const networkEvidence = required("--network-evidence");
if (!["npm-offline-native", "docker-network-none"].includes(networkEvidence)) fail(`unsupported network evidence: ${networkEvidence}`);
if (expectedDigest !== fixtureDigest()) fail(`fixture digest mismatch: manifest ${expectedDigest}, generator ${fixtureDigest()}`);

const scratch = mkdtempSync(join(tmpdir(), "mesh-settling-capture-"));
const packageRoot = join(scratch, "package");
const packRoot = join(scratch, "pack");
const installRoot = join(scratch, "install");
const cacheRoot = join(scratch, "npm-cache");
for (const path of [packageRoot, packRoot, installRoot, cacheRoot]) mkdirSync(path, { recursive: true });

try {
  const fixture = materializeFixture(packageRoot);
  const packed = spawnSync(npmCommand, ["pack", "--ignore-scripts", "--json", "--pack-destination", packRoot], {
    cwd: packageRoot,
    env: { ...process.env, npm_config_cache: cacheRoot, npm_config_audit: "false", npm_config_fund: "false" },
    encoding: "utf8",
  });
  if (packed.status !== 0) fail(`npm pack failed (${packed.status}): ${packed.stderr}`);
  const packRows = JSON.parse(packed.stdout);
  const tarball = join(packRoot, packRows[0].filename);
  const tarballDigest = `sha256:${createHash("sha256").update(readFileSync(tarball)).digest("hex")}`;

  writeFileSync(join(installRoot, "package.json"), `${JSON.stringify({ name: "mesh-settling-consumer", version: "1.0.0", private: true }, null, 2)}\n`);
  const before = readTree(installRoot);
  let previous = before;
  const observations = [];
  let polls = 0;
  let polling = true;
  const started = performance.now();
  const poller = (async () => {
    while (polling) {
      const now = readTree(installRoot);
      const changes = treeDiff(previous, now);
      polls += 1;
      if (changes.length > 0) observations.push({ at_ms: round(performance.now() - started), changes });
      previous = now;
      await new Promise((done) => setImmediate(done));
    }
  })();

  const command = [npmCommand, "install", "--offline", "--no-audit", "--no-fund", "--ignore-scripts", tarball];
  const child = spawn(command[0], command.slice(1), {
    cwd: installRoot,
    env: { ...process.env, npm_config_cache: cacheRoot, npm_config_audit: "false", npm_config_fund: "false" },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stderr = "";
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk) => (stderr += chunk));
  const exitCode = await new Promise((done) => child.once("exit", done));
  await new Promise((done) => setTimeout(done, 250));
  polling = false;
  await poller;
  const elapsedMs = round(performance.now() - started);
  if (exitCode !== 0) fail(`npm install failed (${exitCode}): ${stderr.slice(0, 2000)}`);

  const finalTree = readTree(installRoot, true);
  const fixtureSemantic = fixtureSemanticOutcome(finalTree);
  const packageLock = join(installRoot, "package-lock.json");
  const hiddenLock = join(installRoot, "node_modules", ".package-lock.json");
  const semantic = {
    ...fixtureSemantic,
    package_lock_present: exists(packageLock),
    hidden_lock_present: exists(hiddenLock),
    consumer_names_dependency: readFileSync(join(installRoot, "package.json"), "utf8").includes(FIXTURE.package_name),
  };
  semantic.verified = semantic.fixture_bytes_verified && semantic.package_lock_present && semantic.hidden_lock_present && semantic.consumer_names_dependency;
  if (!semantic.verified) fail(`final semantic outcome failed: ${JSON.stringify(semantic)}`);

  const capture = {
    contract: "mesh-checkpoint-settling-capture/1",
    capture_revision: 3,
    task: "01KZM78X6WQ0YACD52KBVDPVRJ",
    repository_commit: commit,
    capture_script_digest: `sha256:${createHash("sha256").update(readFileSync(fileURLToPath(import.meta.url))).digest("hex")}`,
    fixture,
    tarball_digest: tarballDigest,
    platform: platform(platformTag),
    command: command.map((part) => part === tarball ? "<generated-fixture.tgz>" : part),
    network_evidence: networkEvidence,
    cache_state: "cold-empty-private-npm-cache",
    polling: { clock: "performance.now monotonic", polls, observations: observations.length, blind_spot: "changes completed between readings may be absent" },
    elapsed_ms: elapsedMs,
    observations,
    final_tree_digest: digestTree(finalTree),
    semantic,
  };
  mkdirSync(dirname(resolve(out)), { recursive: true });
  writeFileSync(resolve(out), `${JSON.stringify(capture, null, 2)}\n`);
  process.stdout.write(`capture: ${platformTag} ${observations.length} observations, ${polls} polls, ${elapsedMs} ms, ${semantic.installed_fixture_files} byte-verified files -> ${out}\n`);
} finally {
  rmSync(scratch, { recursive: true, force: true });
}

function readTree(root, withBytes = false) {
  const seen = new Map();
  const walk = (directory) => {
    let entries = [];
    try { entries = readdirSync(directory, { withFileTypes: true }); } catch { return; }
    for (const entry of entries) {
      const full = join(directory, entry.name);
      let stat;
      try { stat = lstatSync(full); } catch { continue; }
      const path = relative(root, full);
      const row = { inode: Number(stat.ino), size: Number(stat.size), links: Number(stat.nlink), dir: stat.isDirectory() };
      if (withBytes && !row.dir) row.bytes = readFileSync(full);
      seen.set(path, row);
      if (row.dir) walk(full);
    }
  };
  walk(root);
  return seen;
}

function treeDiff(before, after) {
  const changes = [];
  for (const [path, now] of after) {
    const then = before.get(path);
    if (!then) changes.push({ change: "appeared", path, inode: now.inode, size: now.size, bytes_delta: now.dir ? 0 : now.size });
    else if (then.inode !== now.inode) changes.push({ change: "replaced", path, from_inode: then.inode, inode: now.inode, size: now.size, bytes_delta: now.dir ? 0 : now.size });
    else if (then.size !== now.size) changes.push({ change: "resized", path, inode: now.inode, from: then.size, size: now.size, bytes_delta: now.dir ? 0 : Math.abs(now.size - then.size) });
  }
  for (const [path, then] of before) if (!after.has(path)) changes.push({ change: "vanished", path, inode: then.inode, bytes_delta: 0 });
  return changes.sort((a, b) => a.path.localeCompare(b.path));
}

function digestTree(tree) {
  const hash = createHash("sha256");
  for (const [path, row] of [...tree.entries()].sort(([a], [b]) => a.localeCompare(b))) {
    hash.update(path).update("\0").update(row.dir ? "dir" : row.bytes).update("\0");
  }
  return `sha256:${hash.digest("hex")}`;
}

function platform(tag) {
  let filesystem_type = "unknown";
  try { filesystem_type = String(statfsSync(tmpdir()).type); } catch {}
  const cpuRows = cpus();
  return {
    tag,
    os: process.platform,
    os_version: process.report.getReport().header.osRelease,
    arch: process.arch,
    filesystem: filesystemName(),
    filesystem_type,
    node: process.version,
    npm: spawnSync(npmCommand, ["--version"], { encoding: "utf8" }).stdout.trim(),
    docker_image: process.env.MESH_SETTLING_IMAGE ?? null,
    hardware: {
      cpu_model: cpuModel(cpuRows),
      cpu_vendor: cpuVendor(),
      logical_cores: cpuRows.length,
      physical_cores: physicalCores(),
      memory_bytes: totalmem(),
    },
  };
}

function filesystemName() {
  if (process.platform === "darwin") {
    const output = commandOut("diskutil", ["info", tmpdir()]);
    const match = output.match(/^\s*File System Personality:\s*(.+)$/m);
    if (match?.[1]) return match[1].trim();
    try { if (Number(statfsSync(tmpdir()).type) === 26) return "apfs"; } catch {}
    return "unknown";
  }
  if (process.platform === "linux") return commandOut("stat", ["-f", "-c", "%T", tmpdir()]) || "unknown";
  return "unknown";
}

function physicalCores() {
  if (process.platform === "darwin") {
    const value = Number(commandOut("sysctl", ["-n", "hw.physicalcpu"]));
    if (Number.isInteger(value) && value > 0) return value;
    const profile = commandOut("system_profiler", ["SPHardwareDataType"]);
    const match = profile.match(/^\s*Total Number of Cores:\s*(\d+)/m);
    return match ? Number(match[1]) : null;
  }
  if (process.platform === "linux") {
    const rows = commandOut("lscpu", ["-p=Core,Socket"])
      .split("\n")
      .filter((line) => line && !line.startsWith("#"));
    const unique = new Set(rows);
    return unique.size > 0 ? unique.size : null;
  }
  return null;
}

function cpuModel(cpuRows) {
  const fromNode = cpuRows.find((row) => row.model?.trim() && row.model.trim() !== "unknown")?.model.trim();
  if (fromNode) return fromNode;
  if (process.platform === "linux") return lscpuField("Model name") || "unavailable";
  return "unknown";
}

function cpuVendor() {
  if (process.platform === "darwin") return "Apple";
  if (process.platform === "linux") return lscpuField("Vendor ID") || "unavailable";
  return "unknown";
}

function lscpuField(label) {
  const line = commandOut("lscpu", []).split("\n").find((row) => row.startsWith(`${label}:`));
  return line ? line.slice(line.indexOf(":") + 1).trim() : "";
}

function commandOut(command, commandArgs) {
  const result = spawnSync(command, commandArgs, { encoding: "utf8" });
  return result.status === 0 ? result.stdout.trim() : "";
}

function exists(path) { try { lstatSync(path); return true; } catch { return false; } }
function value(flag) { const at = args.indexOf(flag); return at === -1 ? null : args[at + 1]; }
function required(flag) { return value(flag) ?? fail(`${flag} is required`); }
function round(number) { return Math.round(number * 1000) / 1000; }
function fail(message) { throw new Error(`checkpoint-settling capture: ${message}`); }
