#!/usr/bin/env node

import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statfsSync,
  writeFileSync,
} from "node:fs";
import { cpus, totalmem, tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import process from "node:process";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const task = "01KZM78X6WQ0YACD52KBVDPVRJ";
const seed = 347_002;
const samples = Number(value("--samples") ?? "5");
const platformTag = required("--platform");
const repositoryCommit = required("--commit");
const out = resolve(required("--out"));
const networkEvidence = required("--network-evidence");
const image = value("--image");
const requested = required("--families").split(",").filter(Boolean);
const tools = {
  idea: value("--idea-format"),
  vim: value("--vim"),
  git: value("--git"),
  rustfmt: value("--rustfmt"),
  vscode: value("--vscode"),
  xvfb: value("--xvfb-run"),
};
const definitions = Object.freeze({
  vscode: { family: "VS Code", pattern: platformTag.startsWith("macos") ? "vscode-macos-in-place-truncate" : "vscode-linux-in-place-truncate", tool: "vscode" },
  jetbrains: { family: "a JetBrains IDE", pattern: platformTag.startsWith("macos") ? "jetbrains-macos-command-line-format-backup-in-place-truncate" : "jetbrains-linux-backup-in-place-truncate", tool: "idea" },
  vim: { family: "vim or neovim", pattern: platformTag.startsWith("macos") ? "vim-in-place-truncate" : "linux-vim-rename-over-with-backup", tool: "vim" },
  git: { family: "Git operations", pattern: platformTag.startsWith("macos") ? "git-checkout-switch-branch" : "linux-git-checkout-in-place", tool: "git" },
  formatter: { family: "a formatter", pattern: platformTag.startsWith("macos") ? "rustfmt-in-place-truncate" : "linux-rustfmt-in-place-truncate", tool: "rustfmt" },
});
const vscodeCaptureExtension = `const vscode = require("vscode");
async function activate() {
  const expected = process.env.MESH_SETTLING_EDIT;
  if (!expected) return;
  const deadline = Date.now() + 20000;
  while (Date.now() < deadline) {
    const document = vscode.workspace.textDocuments.find((entry) => entry.uri.scheme === "file" && !entry.isUntitled);
    if (document) {
      const edit = new vscode.WorkspaceEdit();
      const end = document.lineAt(document.lineCount - 1).range.end;
      edit.replace(document.uri, new vscode.Range(new vscode.Position(0, 0), end), expected.trimEnd());
      if (!(await vscode.workspace.applyEdit(edit))) throw new Error("VS Code refused the capture edit");
      if (!(await document.save())) throw new Error("VS Code refused the capture save");
      await vscode.commands.executeCommand("workbench.action.closeWindow");
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error("capture document did not open before the deadline");
}
function deactivate() {}
module.exports = { activate, deactivate };
`;

if (!Number.isInteger(samples) || samples < 1) fail("--samples must be a positive integer");
if (!new Set(["host-network-not-used", "docker-network-none"]).has(networkEvidence)) fail(`unsupported network evidence ${networkEvidence}`);
if (platformTag.startsWith("linux") && !image) fail("Linux capture requires --image");
for (const name of requested) {
  const definition = definitions[name] ?? fail(`unknown family ${name}`);
  if (!tools[definition.tool]) fail(`${name} requires --${definition.tool === "idea" ? "idea-format" : definition.tool}`);
  if (name === "vscode" && platformTag.startsWith("linux") && !tools.xvfb) fail("Linux vscode requires --xvfb-run");
}

const arms = [];
for (const name of requested) {
  const definition = definitions[name];
  const rows = [];
  for (let sample = 1; sample <= samples; sample += 1) rows.push(await captureOne(name, definition, sample));
  arms.push({
    id: name,
    family: definition.family,
    pattern: definition.pattern,
    tool_path: tools[definition.tool],
    tool_version: toolVersion(name),
    input_digest: inputDigest(name),
    sample_count: samples,
    samples: rows,
  });
}

const capture = {
  contract: "mesh-checkpoint-settling-tool-capture/1",
  task,
  repository_commit: repositoryCommit,
  capture_script_digest: digest(readFileSync(fileURLToPath(import.meta.url))),
  seed,
  platform: platform(platformTag, image),
  network_evidence: networkEvidence,
  cache_state: "fresh-private-tool-state-per-sample",
  sample_count_per_arm: samples,
  invocation: process.argv.slice(1).map((part) => resolveInvocation(part)),
  arms,
};
capture.corpus_digest = digest(Buffer.from(canonical(arms)));
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, `${JSON.stringify(capture, null, 2)}\n`);
process.stdout.write(`tool capture: ${platformTag} ${arms.length} arms x ${samples} samples, corpus ${capture.corpus_digest} -> ${out}\n`);

async function captureOne(name, definition, sample) {
  // VS Code's signed macOS launcher did not load a development extension from the per-user
  // Darwin temporary symlink. Use the same local APFS volume through its stable /private/tmp path;
  // every sample remains a fresh directory and is removed in the finally block.
  const scratchRoot = name === "vscode" && platformTag.startsWith("macos") ? "/private/tmp" : tmpdir();
  const scratch = mkdtempSync(join(scratchRoot, `mesh-settling-${name}-${sample}-`));
  const workspace = join(scratch, "workspace");
  const state = join(scratch, "tool-state");
  mkdirSync(workspace, { recursive: true });
  mkdirSync(state, { recursive: true });
  try {
    const prepared = prepare(name, workspace, state, sample);
    let previous = readTree(workspace);
    const initialTreeDigest = digestTree(previous);
    const observations = [];
    let polls = 0;
    let polling = true;
    const started = performance.now();
    const poller = (async () => {
      while (polling) {
        const now = readTree(workspace);
        const changes = treeDiff(previous, now);
        polls += 1;
        if (changes.length > 0) observations.push({ at_ms: round(performance.now() - started), changes });
        previous = now;
        await new Promise((done) => setImmediate(done));
      }
    })();
    const execution = await run(prepared.command, prepared.commandArgs, prepared.env, workspace);
    await new Promise((done) => setTimeout(done, 100));
    polling = false;
    await poller;
    const finalTree = readTree(workspace);
    const finalChanges = treeDiff(previous, finalTree);
    if (finalChanges.length > 0) observations.push({ at_ms: round(performance.now() - started), changes: finalChanges });
    const elapsedMs = round(performance.now() - started);
    if (execution.code !== 0) fail(`${name} sample ${sample} failed (${execution.code}): ${execution.stderr.slice(0, 2000)}`);
    const semantic = verifySemantic(name, workspace, state, prepared.expected);
    if (!semantic.verified) fail(`${name} sample ${sample} semantic failure: ${JSON.stringify(semantic)}`);
    if (observations.length === 0) fail(`${name} sample ${sample} produced no filesystem observation`);
    return {
      sample,
      seed: seed + sample,
      invocation: [prepared.command, ...prepared.commandArgs].map(resolveInvocation),
      initial_tree_digest: initialTreeDigest,
      final_tree_digest: digestTree(finalTree),
      polls,
      elapsed_ms: elapsedMs,
      observations,
      semantic,
    };
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

function prepare(name, workspace, state, sample) {
  const edited = `edited seed=${seed} sample=${sample}\n`;
  if (name === "vscode") {
    const target = join(workspace, "document.txt");
    const extension = join(state, "extension");
    writeFileSync(target, `original seed=${seed} sample=${sample}\n`);
    mkdirSync(extension, { recursive: true });
    writeFileSync(join(extension, "package.json"), `${JSON.stringify({
      name: "mesh-settling-capture",
      displayName: "Mesh Settling Capture",
      version: "0.0.1",
      publisher: "mesh-local",
      engines: { vscode: "^1.100.0" },
      main: "./extension.js",
      activationEvents: ["onStartupFinished"],
    }, null, 2)}\n`);
    writeFileSync(join(extension, "extension.js"), vscodeCaptureExtension);
    const codeArgs = [
      "--wait",
      "--new-window",
      "--disable-workspace-trust",
      "--disable-telemetry",
      "--skip-welcome",
      "--skip-release-notes",
      "--user-data-dir", join(state, "user-data"),
      "--extensions-dir", join(state, "extensions"),
      "--extensionDevelopmentPath", extension,
      target,
    ];
    const linux = platformTag.startsWith("linux");
    if (linux) codeArgs.unshift("--no-sandbox", "--disable-gpu", "--password-store=basic");
    const isolatedEnvironment = {
      XDG_CONFIG_HOME: join(state, "config"),
      XDG_CACHE_HOME: join(state, "cache"),
      MESH_SETTLING_EDIT: edited,
    };
    // The signed macOS application is launched by LaunchServices and must retain the login HOME.
    // Its explicit user-data and extension directories still make every sample fresh. Linux runs
    // directly inside the disposable container and can bind HOME to the sample state as well.
    if (linux) isolatedEnvironment.HOME = state;
    return {
      command: linux ? tools.xvfb : tools.vscode,
      commandArgs: linux ? ["-a", tools.vscode, ...codeArgs] : codeArgs,
      env: isolatedEnvironment,
      expected: { target, edited: edited.trimEnd() },
    };
  }
  if (name === "jetbrains") {
    const target = join(workspace, "Example.java");
    writeFileSync(target, `class Example{public static void main(String[]args){System.out.println("${seed}-${sample}");}}\n`);
    return {
      command: tools.idea,
      commandArgs: ["-allowDefaults", target],
      env: { HOME: state, XDG_CONFIG_HOME: join(state, "config"), XDG_CACHE_HOME: join(state, "cache") },
      expected: { target },
    };
  }
  if (name === "vim") {
    const target = join(workspace, "doc.txt");
    writeFileSync(target, `original seed=${seed} sample=${sample}\n`);
    return {
      command: tools.vim,
      commandArgs: ["-Nu", "NONE", "-n", "-es", target, "-c", `call setline(1, '${edited.trim()}')`, "-c", "wq"],
      env: { HOME: state },
      expected: { target, edited },
    };
  }
  if (name === "git") {
    const runGit = (...parts) => commandChecked(tools.git, parts, workspace);
    runGit("init", "-b", "base");
    runGit("config", "user.email", "settling@example.invalid");
    runGit("config", "user.name", "TASK-347 fixture");
    for (let index = 0; index < 128; index += 1) writeFileSync(join(workspace, `f${String(index).padStart(3, "0")}.txt`), `base ${seed} ${sample} ${index}\n`);
    runGit("add", ".");
    runGit("commit", "-m", "base");
    runGit("checkout", "-b", "other");
    for (let index = 0; index < 128; index += 1) writeFileSync(join(workspace, `f${String(index).padStart(3, "0")}.txt`), `other ${seed} ${sample} ${index}\n`);
    runGit("add", ".");
    runGit("commit", "-m", "other");
    runGit("checkout", "base");
    return { command: tools.git, commandArgs: ["checkout", "other"], env: {}, expected: { sample } };
  }
  if (name === "formatter") {
    const target = join(workspace, "main.rs");
    let source = "";
    for (let index = 0; index < 512; index += 1) source += `fn f${index}(){println!("${seed}-${sample}-${index}");}\n`;
    writeFileSync(target, source);
    return { command: tools.rustfmt, commandArgs: [target], env: { HOME: state }, expected: { target } };
  }
  fail(`no preparation for ${name}`);
}

function verifySemantic(name, workspace, state, expected) {
  if (name === "vscode") return { verified: readFileSync(expected.target, "utf8") === expected.edited };
  if (name === "jetbrains") {
    const text = readFileSync(expected.target, "utf8");
    return { verified: text.includes("class Example {") && text.includes("public static void main"), content_digest: digest(Buffer.from(text)) };
  }
  if (name === "vim") return { verified: readFileSync(expected.target, "utf8") === expected.edited };
  if (name === "git") {
    const branch = commandOut(tools.git, ["branch", "--show-current"], workspace);
    const files = readdirSync(workspace).filter((entry) => /^f\d+\.txt$/.test(entry));
    const exact = files.length === 128 && files.every((entry) => {
      const index = Number(entry.slice(1, 4));
      return readFileSync(join(workspace, entry), "utf8") === `other ${seed} ${expected.sample} ${index}\n`;
    });
    return { verified: branch === "other" && exact, branch, files: files.length };
  }
  if (name === "formatter") {
    const check = spawnSync(tools.rustfmt, ["--check", expected.target], { encoding: "utf8" });
    return { verified: check.status === 0, check_status: check.status };
  }
  return { verified: false };
}

function inputDigest(name) {
  const hash = createHash("sha256");
  hash.update(canonical({ name, seed, samples, definition: definitions[name] }));
  return `sha256:${hash.digest("hex")}`;
}

function toolVersion(name) {
  if (name === "vscode") return commandOut(tools.vscode, ["--version"]).split("\n")[0];
  if (name === "jetbrains") {
    const contents = dirname(dirname(tools.idea));
    const product = [join(contents, "product-info.json"), join(contents, "Resources", "product-info.json")].find(exists);
    if (product) return JSON.parse(readFileSync(product, "utf8")).version;
    return commandOut(tools.idea, ["-h"]).split("\n")[0];
  }
  if (name === "vim") return commandOut(tools.vim, ["--version"]).split("\n")[0];
  if (name === "git") return commandOut(tools.git, ["--version"]);
  if (name === "formatter") return commandOut(tools.rustfmt, ["--version"]);
  return "unavailable";
}

async function run(command, commandArgs, extraEnv, cwd) {
  const child = spawn(command, commandArgs, {
    cwd,
    env: { ...process.env, ...extraEnv },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  child.stdout.on("data", (chunk) => (stdout += chunk));
  child.stderr.on("data", (chunk) => (stderr += chunk));
  const exited = new Promise((done) => child.once("exit", done));
  let timeout;
  const code = await Promise.race([
    exited,
    new Promise((done) => {
      timeout = setTimeout(() => {
        child.kill("SIGKILL");
        done(124);
      }, 60_000);
    }),
  ]);
  clearTimeout(timeout);
  return { code, stdout, stderr };
}

function readTree(root) {
  const seen = new Map();
  const walk = (directory) => {
    let entries = [];
    try { entries = readdirSync(directory, { withFileTypes: true }); } catch { return; }
    for (const entry of entries) {
      const full = join(directory, entry.name);
      let stat;
      try { stat = lstatSync(full); } catch { continue; }
      const path = relative(root, full);
      const dir = stat.isDirectory();
      let contentDigest = null;
      if (!dir) {
        try { contentDigest = digest(readFileSync(full)); } catch { continue; }
      }
      const row = { inode: Number(stat.ino), size: Number(stat.size), dir, digest: contentDigest };
      seen.set(path, row);
      if (dir) walk(full);
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
    else if (then.size !== now.size || then.digest !== now.digest) changes.push({ change: "written", path, inode: now.inode, from: then.size, size: now.size, bytes_delta: now.dir ? 0 : now.size });
  }
  for (const [path, then] of before) if (!after.has(path)) changes.push({ change: "vanished", path, inode: then.inode, bytes_delta: 0 });
  return changes.sort((a, b) => a.path.localeCompare(b.path));
}

function digestTree(tree) {
  const hash = createHash("sha256");
  for (const [path, row] of [...tree].sort(([a], [b]) => a.localeCompare(b))) hash.update(path).update("\0").update(row.dir ? "dir" : row.digest).update("\0");
  return `sha256:${hash.digest("hex")}`;
}

function platform(tag, dockerImage) {
  const cpuRows = cpus();
  const nodeCpuModel = cpuRows
    .map((row) => row.model?.trim())
    .find((model) => model && model !== "unknown" && model !== "-");
  return {
    tag,
    os: process.platform,
    os_version: process.report.getReport().header.osRelease,
    arch: process.arch,
    filesystem: filesystemName(),
    filesystem_type: safe(() => String(statfsSync(tmpdir()).type), "unavailable"),
    node: process.version,
    docker_image: dockerImage,
    hardware: {
      cpu_model: nodeCpuModel || lscpuField("Model name") || "unavailable",
      cpu_vendor: process.platform === "darwin" ? "Apple" : lscpuField("Vendor ID") || "unavailable",
      logical_cores: cpuRows.length,
      physical_cores: process.platform === "darwin" ? Number(commandOut("sysctl", ["-n", "hw.physicalcpu"])) || cpuRows.length : physicalLinux(),
      memory_bytes: totalmem(),
    },
  };
}

function filesystemName() {
  if (process.platform === "darwin") {
    const output = commandOut("diskutil", ["info", tmpdir()]);
    return output.match(/^\s*File System Personality:\s*(.+)$/m)?.[1]?.trim() || (Number(safe(() => statfsSync(tmpdir()).type, 0)) === 26 ? "apfs" : "unavailable");
  }
  return commandOut("stat", ["-f", "-c", "%T", tmpdir()]) || "unavailable";
}

function physicalLinux() {
  const rows = commandOut("lscpu", ["-p=Core,Socket"]).split("\n").filter((line) => line && !line.startsWith("#"));
  return new Set(rows).size || cpus().length;
}

function lscpuField(label) {
  const line = commandOut("lscpu", []).split("\n").find((row) => row.startsWith(`${label}:`));
  return line ? line.slice(line.indexOf(":") + 1).trim() : "";
}

function commandChecked(command, commandArgs, cwd) {
  const result = spawnSync(command, commandArgs, { cwd, encoding: "utf8" });
  if (result.status !== 0) fail(`${command} ${commandArgs.join(" ")} failed: ${result.stderr}`);
  return result.stdout.trim();
}
function commandOut(command, commandArgs, cwd) {
  const result = spawnSync(command, commandArgs, { cwd, encoding: "utf8" });
  return result.status === 0 ? result.stdout.trim() : "";
}
function canonical(input) {
  if (Array.isArray(input)) return `[${input.map(canonical).join(",")}]`;
  if (input && typeof input === "object") return `{${Object.keys(input).sort().map((key) => `${JSON.stringify(key)}:${canonical(input[key])}`).join(",")}}`;
  return JSON.stringify(input);
}
function digest(bytes) { return `sha256:${createHash("sha256").update(bytes).digest("hex")}`; }
function exists(path) { try { lstatSync(path); return true; } catch { return false; } }
function value(flag) { const at = args.indexOf(flag); return at === -1 ? null : args[at + 1]; }
function required(flag) { return value(flag) ?? fail(`${flag} is required`); }
function resolveInvocation(part) { return part.startsWith(tmpdir()) ? "<temporary-path>" : part; }
function round(number) { return Math.round(number * 1000) / 1000; }
function safe(operation, fallback) { try { return operation(); } catch { return fallback; } }
function fail(message) { throw new Error(`checkpoint-settling tool capture: ${message}`); }
