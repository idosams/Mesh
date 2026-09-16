#!/usr/bin/env node

import { spawn } from "node:child_process";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { pathToFileURL } from "node:url";

const IDEA_BINARY = "/opt/idea/bin/idea";
const original = "alpha original document\n";
const edited = `JetBrains edit: ${original}`;

export function parseCliArgs(argv) {
  if (argv.length !== 1) {
    throw new Error("usage: jetbrains-linux-xdotool.mjs <capture-root>");
  }
  return argv[0];
}

export function parseWindowId(stdout) {
  const ids = stdout.trim().split(/\s+/).filter(Boolean);
  if (ids.length !== 1 || !/^\d+$/.test(ids[0])) {
    throw new Error("expected exactly one owned JetBrains dialog");
  }
  return ids[0];
}

export function parseOwnedPid(stdout, expectedPid) {
  const pid = Number(stdout.trim());
  if (!Number.isInteger(pid) || pid !== expectedPid) {
    throw new Error("refusing a JetBrains dialog not owned by the launched child");
  }
  return pid;
}

export function parseGeometry(stdout) {
  const values = Object.fromEntries(stdout.trim().split(/\r?\n/).map((line) => line.split("=", 2)));
  const geometry = Object.fromEntries(["X", "Y", "WIDTH", "HEIGHT"].map((key) => [key, Number(values[key])]));
  if (!Object.values(geometry).every(Number.isInteger) || geometry.WIDTH < 500 || geometry.HEIGHT < 350) {
    throw new Error("JetBrains dialog reported invalid geometry");
  }
  return geometry;
}

export function agreementPoints(geometry) {
  return {
    checkbox: [geometry.X + 42, geometry.Y + geometry.HEIGHT - 68],
    continueButton: [geometry.X + geometry.WIDTH - 61, geometry.Y + geometry.HEIGHT - 29],
  };
}

export function dataSharingPoint(geometry) {
  return [geometry.X + 323, geometry.Y + geometry.HEIGHT - 29];
}

export function editorWindowPattern(documentPath) {
  const escapedParent = dirname(documentPath).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return `^doc\\.txt - ${escapedParent}$`;
}

export async function runCapture(root, {
  spawnImpl = spawn,
  createTempDir = (prefix) => mkdtempSync(join(tmpdir(), prefix)),
  removeTempDir = (path) => rmSync(path, { recursive: true, force: true }),
  runCommand = run,
  delayImpl = delay,
  commandTimeoutMs = 5_000,
  startupTimeoutMs = 30_000,
} = {}) {
  const documentPath = join(root, "doc.txt");
  if (readFileSync(documentPath, "utf8") !== original) {
    throw new Error("doc.txt must contain the exact baseline: alpha original document\\n");
  }

  const ownedRoot = createTempDir("mesh-jetbrains-profile-");
  const config = join(ownedRoot, "config");
  const system = join(ownedRoot, "system");
  const plugins = join(ownedRoot, "plugins");
  const log = join(ownedRoot, "log");
  const properties = join(ownedRoot, "idea.properties");
  for (const directory of [config, system, plugins, log]) mkdirSync(directory);
  writeFileSync(properties, [
    `idea.config.path=${config}`,
    `idea.system.path=${system}`,
    `idea.plugins.path=${plugins}`,
    `idea.log.path=${log}`,
    "",
  ].join("\n"));

  let xvfb;
  let idea;
  try {
    xvfb = spawnImpl("Xvfb", [":99", "-screen", "0", "1280x800x24"], { stdio: "ignore" });
    await waitForSpawn(xvfb);
    await delayImpl(500);

    const env = { ...process.env, DISPLAY: ":99", IDEA_PROPERTIES: properties };
    idea = spawnImpl(IDEA_BINARY, [
      "dontReopenProjects",
      "disableNonBundledPlugins",
      "nosplash",
      "-e",
      documentPath,
    ], { env, stdio: "ignore" });
    await waitForSpawn(idea);

    const agreement = await waitForDialog("IntelliJ IDEA User Agreement", idea, startupTimeoutMs, env, commandTimeoutMs, runCommand, delayImpl);
    const points = agreementPoints(agreement);
    await click(points.checkbox, env, commandTimeoutMs, runCommand);
    await click(points.continueButton, env, commandTimeoutMs, runCommand);

    const sharing = await waitForDialog("Data Sharing", idea, startupTimeoutMs, env, commandTimeoutMs, runCommand, delayImpl);
    await click(dataSharingPoint(sharing), env, commandTimeoutMs, runCommand);

    const editor = await waitForOwnedWindow(
      editorWindowPattern(documentPath),
      "the supplied LightEdit document",
      idea,
      startupTimeoutMs,
      env,
      commandTimeoutMs,
      runCommand,
      delayImpl,
    );
    await click([editor.X + 190, editor.Y + 40], env, commandTimeoutMs, runCommand);
    await xdotool(["key", "--clearmodifiers", "ctrl+Home"], env, commandTimeoutMs, runCommand);
    await xdotool(["type", "--delay", "2", "JetBrains edit: "], env, commandTimeoutMs, runCommand);
    await xdotool(["key", "--clearmodifiers", "ctrl+s"], env, commandTimeoutMs, runCommand);

    const deadline = Date.now() + 5_000;
    while (Date.now() < deadline && readFileSync(documentPath, "utf8") !== edited) await delayImpl(25);
    const actual = readFileSync(documentPath, "utf8");
    if (actual !== edited) {
      throw new Error(`JetBrains persisted unexpected bytes: ${JSON.stringify(actual)}`);
    }
  } finally {
    try {
      if (idea) await terminateOwnedChild(idea);
      if (xvfb) await terminateOwnedChild(xvfb);
    } finally {
      removeTempDir(ownedRoot);
    }
  }
}

async function waitForDialog(title, child, timeoutMs, env, commandTimeoutMs, runCommand, delayImpl) {
  return waitForOwnedWindow(`^${title}$`, title, child, timeoutMs, env, commandTimeoutMs, runCommand, delayImpl);
}

async function waitForOwnedWindow(pattern, label, child, timeoutMs, env, commandTimeoutMs, runCommand, delayImpl) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (child.exitCode !== null || child.signalCode !== null) {
      throw new Error(`JetBrains exited before showing ${label}`);
    }
    try {
      const found = await runCommand("xdotool", ["search", "--onlyvisible", "--name", pattern], env, commandTimeoutMs);
      const window = parseWindowId(found);
      const pid = await runCommand("xdotool", ["getwindowpid", window], env, commandTimeoutMs);
      parseOwnedPid(pid, child.pid);
      const geometry = await runCommand("xdotool", ["getwindowgeometry", "--shell", window], env, commandTimeoutMs);
      return parseGeometry(geometry);
    } catch {
      await delayImpl(100);
    }
  }
  throw new Error(`timed out waiting for owned JetBrains window: ${label}`);
}

async function click([x, y], env, timeoutMs, runCommand) {
  await xdotool(["mousemove", String(x), String(y), "click", "1"], env, timeoutMs, runCommand);
}

async function xdotool(args, env, timeoutMs, runCommand) {
  await runCommand("xdotool", args, env, timeoutMs);
}

function run(binary, args, env, timeoutMs) {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, args, { env, stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk) => (stdout += chunk));
    child.stderr.on("data", (chunk) => (stderr += chunk));
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      reject(new Error(`${binary} timed out`));
    }, timeoutMs);
    child.once("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
    child.once("exit", (code, signal) => {
      clearTimeout(timer);
      if (code === 0) resolve(stdout);
      else reject(new Error(`${binary} exited ${code ?? signal}: ${stderr.trim()}`));
    });
  });
}

async function terminateOwnedChild(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  child.kill("SIGTERM");
  if (!(await waitForExit(child, 5_000))) child.kill("SIGKILL");
  if (!(await waitForExit(child, 3_000))) throw new Error("owned compatibility child did not terminate");
}

function waitForSpawn(child) {
  return new Promise((resolve, reject) => {
    child.once("spawn", resolve);
    child.once("error", reject);
  });
}

function waitForExit(child, timeoutMs) {
  if (child.exitCode !== null || child.signalCode !== null) return Promise.resolve(true);
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve(false), timeoutMs);
    child.once("exit", () => {
      clearTimeout(timer);
      resolve(true);
    });
  });
}

const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await runCapture(parseCliArgs(process.argv.slice(2)));
}
