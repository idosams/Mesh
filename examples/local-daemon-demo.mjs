#!/usr/bin/env node

import { spawn, spawnSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { homedir, tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const argv = process.argv.slice(2);
const keep = argv.includes("--keep");
const verbose = argv.includes("--verbose");
const offline = argv.includes("--offline");
const skipBuild = argv.includes("--skip-build");
const mounted = argv.includes("--mounted");
const EXPECTED_SURFACE_VERSION = 8;
// Every public onboarding route names this exact proof size. Keep the count here so adding or
// removing a check cannot silently leave one alpha promise stale while another remains correct.
const EXPECTED_DEFAULT_CHECKS = 44;
const executable = process.platform === "win32" ? ".exe" : "";
const rustupCargo = join(homedir(), ".cargo", "bin", `cargo${executable}`);
const cargo = process.env.CARGO ?? (existsSync(rustupCargo) ? rustupCargo : "cargo");
// Cargo runs with the repository as cwd, so a relative override is relative to that same root.
// Keep lookup and build in agreement when verification shares a target outside the worktree.
const target = resolve(repo, process.env.CARGO_TARGET_DIR ?? "target");
const meshd = join(target, "debug", `meshd${executable}`);
const meshctl = join(target, "debug", `meshctl${executable}`);
const checkpoint = join(target, "debug", "examples", `capture-checkpoint${executable}`);
// Colour only a real terminal; piped output and CI logs stay plain text.
const colourful = process.stdout.isTTY && process.env.NO_COLOR === undefined;

const KNOWN_FLAGS = new Set([
  "--keep",
  "--verbose",
  "--offline",
  "--skip-build",
  "--mounted",
  "--help",
  "-h",
]);
const usage = [
  "Usage: node examples/local-daemon-demo.mjs [options]",
  "",
  "  --keep       keep the temporary workspace after a passing run",
  "  --verbose    also print the raw JSON Lines exchanged with the service",
  "  --offline    tell Cargo to build from its local cache only",
  "  --skip-build reuse already-built debug binaries (the smoke-test path)",
  "  --mounted    additionally run the privileged network-disabled FUSE proof",
  "  --help, -h   show this message",
  "",
  "Exit codes: 0 every check passed, 1 a check failed.",
].join("\n");

if (argv.includes("--help") || argv.includes("-h")) {
  console.log(usage);
  process.exit(0);
}

// An ignored flag is worse than a rejected one: a mistyped --keep would silently
// delete the very workspace the user asked to inspect.
const unknown = argv.filter((argument) => !KNOWN_FLAGS.has(argument));
if (unknown.length > 0) {
  console.error(`demo: unknown argument ${unknown.join(" ")}\n\n${usage}`);
  process.exit(1);
}
if (offline && skipBuild) {
  console.error(`demo: --offline and --skip-build are alternatives, not a useful combination\n\n${usage}`);
  process.exit(1);
}

if (process.platform === "win32") {
  fail("the current local demo requires Unix-domain sockets; use macOS or Linux");
}

const checks = [];
let currentStage = "start-up";
// Stage 1 answers from local rules alone; the service is not running yet. Counted
// separately so the summary never claims more service coverage than it has.
const LOCAL_CHECKS = 4;

heading("Mesh local daemon demo");

// Unix-domain socket paths are short (about 104 bytes on macOS), while macOS's TMPDIR is long.
const scratchRoot = existsSync("/tmp") ? "/tmp" : tmpdir();
const scratch = mkdtempSync(join(scratchRoot, "mesh-demo-"));
const workspace = join(scratch, "workspace");
const rulesWorkspace = join(scratch, "exclusion-example");
const socket = join(scratch, "daemon.sock");
const reviewerKey = join(scratch, "reviewer.key");
const reviewerPublicKey = "66be7e332c7a453332bd9d0a7f7db055f5c5ef1a06ada66d98b39fb6810c473a";
const actorId = "aa".repeat(32);
mkdirSync(join(rulesWorkspace, "mounts"), { recursive: true });
mkdirSync(workspace, { recursive: true });
writeFileSync(join(workspace, "notes.txt"), "first draft\n");
writeFileSync(reviewerKey, Buffer.alloc(32, 11), { mode: 0o600 });
chmodSync(reviewerKey, 0o600);
writeFileSync(join(rulesWorkspace, ".meshignore"), "target/\n");
writeFileSync(join(rulesWorkspace, "mounts", ".gitignore"), "build/\n");
field("workspace", workspace);

let daemon;
let daemonError = "";
let finished = false;
let boundary = [];

try {
  verifyDocumentedDefaultCheckCount();
  prepareBinaries();

  step("1. Explain local restrictions and one exclusion (no service needed)");
  const restrictions = lastJson(ctl(["restrictions"]));
  check("the restriction report names the selected fallback", restrictions.backend === "folder-watch");
  check("the restriction report states the authority boundary", restrictions.authoritative === false);
  field("backend", restrictions.backend);
  field("authoritative", String(restrictions.authoritative), "this fallback never claims it saw everything");
  field("named limits", count(restrictions.restrictions));

  const exclusions = lastJson(ctl(["exclusions", rulesWorkspace, "target/debug/app"]));
  check("the exclusion verdict is not-versioned", exclusions.verdict?.answer === "not-versioned");
  check("the exclusion verdict cites the rule that produced it", exclusions.verdict?.source === ".meshignore");
  field("target/debug/app", exclusions.verdict.answer, `rule "${exclusions.verdict.rule}" from ${exclusions.verdict.source}`);

  step("2. Start meshd with explicit reviewer trust and checkpoint parameters");
  daemon = spawn(meshd, daemonArguments(), {
    cwd: repo,
    stdio: ["pipe", "pipe", "pipe"],
  });
  daemon.stderr.setEncoding("utf8");
  daemon.stderr.on("data", (chunk) => {
    daemonError += chunk;
  });
  const ready = await firstJsonLine(daemon, 5_000, "meshd readiness");
  trace("ready", ready);
  field("endpoint", ready.endpoint);
  field("surface version", String(ready.surface_version));
  check("meshd reports ready and serving", ready.ready === true && ready.serving === true);
  check(
    `meshd readiness reports surface version ${EXPECTED_SURFACE_VERSION}`,
    ready.surface_version === EXPECTED_SURFACE_VERSION,
  );
  check("meshd bound the endpoint it was given", ready.endpoint === socket);
  check("meshd selected the folder-watch backend", ready.backend === "folder-watch");
  check("the fallback does not claim authoritative capture", ready.authoritative === false);

  step("3. Query the service from separate meshctl processes");
  const status = response(ctl(["--endpoint", socket, "status"]));
  check("a separate process reads service health", status.serving === true);
  const describe = response(ctl(["--endpoint", socket, "describe"]));
  check(
    `the service catalogue reports surface version ${EXPECTED_SURFACE_VERSION}`,
    describe.surface_version === EXPECTED_SURFACE_VERSION,
  );
  check("the catalogue lists the methods it serves", Array.isArray(describe.methods) && describe.methods.length > 0);
  field("methods", describe.methods.map((method) => method.name).join(", "));
  const startup = response(ctl(["--endpoint", socket, "startup"]));
  check("the service explains what it found at start-up", typeof startup.sentence === "string" && startup.sentence.length > 0);
  field("startup", startup.sentence);

  step("4. Subscribe to events, then open the workspace");
  const watching = capture(meshctl, ["--endpoint", socket, "watch", "2"], repo);
  await delay(150);
  const opened = response(ctl(["--endpoint", socket, "open", workspace]));
  trace("open", opened);
  field("root", opened.root);
  field("saved records", String(opened.records));
  field("durable digest", opened.digest);
  check("a fresh workspace starts with zero saved records", opened.records === 0);
  check("opening the workspace created private records", existsSync(join(workspace, ".mesh", "records.mesh")));
  check("opening the workspace created a private index", existsSync(join(workspace, ".mesh", "metadata.sqlite")));

  const watchOutput = await watching;
  check("the event subscription exited cleanly", watchOutput.code === 0, watchOutput.stderr);
  const events = jsonLines(watchOutput.stdout).filter((line) => line.t === "event");
  const eventKinds = events.map((event) => event.event?.kind ?? event.value?.kind ?? event.kind);
  field("events received", eventKinds.join(" → "));
  check("exactly two live events arrived", events.length === 2);
  check("the first event is serving", eventKinds[0] === "serving");
  check("the second event is workspace-opened", eventKinds[1] === "workspace-opened");

  step("5. Restart over a real saved change, then open its exact review");
  daemon.stdin.end("stop\n");
  let stopped = await childExit(daemon, 5_000, "meshd pre-capture shutdown");
  check("the configured daemon stopped before the external capture", stopped === 0, daemonError);
  daemon = undefined;
  check("the socket was removed before restart", !existsSync(socket));

  const captured = join(workspace, "notes.txt");
  writeFileSync(captured, "second draft\n");
  check(
    "the ordinary native folder contains the edit before its explicit save",
    readFileSync(captured, "utf8") === "second draft\n",
  );
  const captureOutput = run(checkpoint, [workspace, captured, actorId, "notes.txt"], {
    cwd: repo,
  }).stdout;
  const target = /\btarget=([0-9a-f]{64})\b/u.exec(captureOutput)?.[1];
  check("the capture named its exact saved operation", typeof target === "string");

  daemonError = "";
  daemon = spawn(meshd, [...daemonArguments(), "--workspace", workspace], {
    cwd: repo,
    stdio: ["pipe", "pipe", "pipe"],
  });
  daemon.stderr.setEncoding("utf8");
  daemon.stderr.on("data", (chunk) => (daemonError += chunk));
  await firstJsonLine(daemon, 5_000, "configured meshd restart");
  const saved = response(ctl(["--endpoint", socket, "state"]));
  check("the restarted daemon restored the captured saved change", saved.records === 2 && saved.operations === 1);
  check("the saved change exposes one derived private version", typeof saved.private_version?.version === "string");
  check("configured trust reports a truthful null shared version", saved.shared_version === null);

  const openedReview = response(
    ctl(["--endpoint", socket, "review-current", reviewerKey]),
  );
  check("review.open persisted one exact review", openedReview.reviews === 1);
  const reviewBundle = openedReview.review_items?.[0]?.bundle;
  check("Mesh computed the exact review bundle", typeof reviewBundle === "string");
  const approval = spawnSync(
    meshctl,
    ["--endpoint", socket, "approve", reviewBundle, target, "genesis", reviewerKey],
    { cwd: repo, encoding: "utf8", stdio: "pipe" },
  );
  check("software-held approval is refused before publication", approval.status === 2);
  check(
    "the refusal names the missing human-held authority",
    approval.stderr.includes("no verified human-held signing authority"),
    approval.stderr,
  );

  step("6. Restart and prove the review survives without inventing a shared version");
  daemon.stdin.end("stop\n");
  stopped = await childExit(daemon, 5_000, "meshd publication shutdown");
  check("meshd exited zero after review", stopped === 0, daemonError);
  daemon = undefined;
  daemonError = "";
  daemon = spawn(meshd, [...daemonArguments(), "--workspace", workspace], {
    cwd: repo,
    stdio: ["pipe", "pipe", "pipe"],
  });
  daemon.stderr.setEncoding("utf8");
  daemon.stderr.on("data", (chunk) => (daemonError += chunk));
  await firstJsonLine(daemon, 5_000, "post-review meshd restart");
  const restored = response(ctl(["--endpoint", socket, "state"]));
  check("the shared version stays unavailable without HumanHeld authority", restored.shared_version === null);
  check("the review remains in the durable journal", restored.records === 3 && restored.reviews === 1);
  boundary = restored.not_yet ?? [];
  const boundarySubjects = boundary.map((item) => item.subject);
  check(
    "file names and folders are no longer in the not-yet boundary",
    !boundarySubjects.includes("file names and folders"),
  );
  check(
    "the service keeps the shared-version boundary visible",
    boundarySubjects.includes("shared version"),
  );
  field("remaining boundary", boundarySubjects.join(", "));

  step("7. Preview the shipped redacted support bundle");
  const bundleOutput = ctl(["--endpoint", socket, "support-bundle", workspace]);
  const bundle = lastJson(bundleOutput);
  check("the support bundle uses the stable v1 schema", bundle.schema === "mesh-support-bundle/v1");
  check(
    "the support bundle includes only crash diagnostics",
    Array.isArray(bundle.included) &&
      bundle.included.length === 1 &&
      bundle.included[0] === "crash-diagnostics",
  );
  check(
    "the support bundle explicitly excludes paths, content, and keys",
    ["raw-paths", "file-content", "key-material"].every((item) => bundle.excluded?.includes(item)),
  );
  check(
    "the support bundle replaces the workspace path with a correlation digest",
    /^blake3:[0-9a-f]{64}$/u.test(bundle.workspace_correlation ?? "") &&
      !bundleOutput.includes(workspace),
  );
  check(
    "the live support bundle reports the daemon's verified serving state",
    bundle["crash-diagnostics"]?.serving === true,
  );
  check("the support bundle contains no saved file bytes", !bundleOutput.includes("second draft"));
  field("support schema", bundle.schema);
  field("included", bundle.included.join(", "));

  if (mounted) {
    step("8. Run the optional mounted capture in a network-disabled container");
    run("sh", [join(repo, "examples", "run-local-mounted-demo.sh")], {
      cwd: repo,
      stream: true,
    });
    check("the mounted stage captured, reviewed, and restarted with networking disabled", true);
  }

  step(`${mounted ? "9" : "8"}. Stop cleanly and prove the socket is gone`);
  daemon.stdin.end("stop\n");
  stopped = await childExit(daemon, 5_000, "meshd shutdown");
  check("meshd exited zero on request", stopped === 0, `meshd exited ${stopped}\n${daemonError}`);
  daemon = undefined;
  check("the socket was removed on shutdown", !existsSync(socket));

  finished = true;
  report();
} catch (error) {
  // The demo stops at the first failed condition, because every later stage depends
  // on the state an earlier one established. Report it as a named failure rather
  // than letting the throw escape as an unlabelled stack trace.
  process.exitCode = 1;
  console.log("");
  console.log("─".repeat(78));
  console.log(`FAIL — during "${currentStage}"`);
  console.log(`  ${String(error?.message ?? error).replace(/^demo: /u, "")}`);
  console.log(`  ${checks.filter((entry) => entry.ok).length} of ${checks.length} checks passed before this one.`);
} finally {
  if (daemon && daemon.exitCode === null) {
    daemon.stdin?.end("stop\n");
    await childExit(daemon, 2_000, "cleanup").catch(() => daemon.kill("SIGKILL"));
  }
  if (keep || !finished) {
    console.log(`\nDemo files retained at ${scratch}`);
  } else {
    rmSync(scratch, { recursive: true, force: true });
  }
}

function report() {
  const expectedChecks = EXPECTED_DEFAULT_CHECKS + (mounted ? 1 : 0);
  assert(
    checks.length === expectedChecks,
    `the demo check inventory changed from ${expectedChecks} to ${checks.length}; update the executable expectation and every onboarding document together`,
  );
  console.log("");
  console.log("─".repeat(78));
  console.log(`PASS — ${checks.length} checks, all green (${LOCAL_CHECKS} local rules, the rest end to end).`);
  console.log("Checkpoint thresholds were explicitly accepted on every process start.");
  console.log("The save used the shipped explicit capture-checkpoint helper; idle settlement is not claimed.");
  console.log("The support bundle was previewed locally and never transmitted.");
  console.log(
    mounted
      ? "The optional mounted proof also passed with container networking disabled."
      : "The privileged FUSE proof was not run; add --mounted to request it explicitly.",
  );

  if (boundary.length > 0) {
    console.log("");
    console.log("Current boundary — what this build does NOT yet do:");
    for (const item of boundary) {
      console.log(`  • ${item.subject}`);
      for (const line of wrap(item.reason, 72)) console.log(`      ${line}`);
    }
    console.log("");
    console.log("The remaining boundary is reported by the running service, not inferred by");
    console.log("this script. The proof claims only the explicit save and durable review above.");
  }
}

function verifyDocumentedDefaultCheckCount() {
  const sources = [
    {
      path: "README.md",
      pattern: /normal passing run prints (\d+) `✓`/u,
    },
    {
      path: "docs/demo.md",
      pattern: /PASS — (\d+) checks, all green/u,
    },
    {
      path: "docs/user-guide.md",
      pattern: /passing run prints (\d+) `✓` checks/iu,
    },
    {
      path: "docs/project-status.md",
      pattern: /local demo should print (\d+) passing checks/iu,
    },
  ];
  for (const source of sources) {
    const text = readFileSync(join(repo, source.path), "utf8");
    const count = Number(text.match(source.pattern)?.[1]);
    assert(
      count === EXPECTED_DEFAULT_CHECKS,
      `${source.path} must name the executable demo's ${EXPECTED_DEFAULT_CHECKS} default checks`,
    );
  }
}

function heading(text) {
  console.log(text);
  console.log("=".repeat(text.length));
}

function step(text) {
  currentStage = text;
  console.log(`\n${text}`);
}

// Parse the payload line of a single-reply command through the same error path as
// every other reply, so a malformed line names the stage instead of throwing SyntaxError.
function lastJson(output) {
  const lines = jsonLines(output);
  assert(lines.length >= 1, `expected a JSON reply, received: ${output}`);
  return lines.at(-1);
}

function count(value) {
  return Array.isArray(value) ? String(value.length) : "(missing)";
}

function field(label, value, note) {
  const rendered = note ? `${value}  — ${note}` : value;
  console.log(`   ${label.padEnd(20)} ${rendered}`);
}

function check(label, condition, detail) {
  checks.push({ label, ok: Boolean(condition) });
  if (condition) {
    console.log(`   ${green("✓")} ${label}`);
    return;
  }
  console.log(`   ${red("✗")} ${label}`);
  fail(detail ? `${label}\n${detail}` : label);
}

function green(text) {
  return colourful ? `[32m${text}[0m` : text;
}

function red(text) {
  return colourful ? `[31m${text}[0m` : text;
}

function trace(label, value) {
  if (verbose) console.log(`   [raw] ${label}: ${JSON.stringify(value)}`);
}

function wrap(text, width) {
  const words = String(text).split(/\s+/u).filter(Boolean);
  return words.reduce((lines, word) => {
    const last = lines.at(-1);
    if (last && `${last} ${word}`.length <= width) {
      return [...lines.slice(0, -1), `${last} ${word}`];
    }
    return [...lines, word];
  }, []);
}

function seconds(ms) {
  return `${(ms / 1000).toFixed(1)}s`;
}

function prepareBinaries() {
  if (skipBuild) {
    step("Checking the already-built local binaries");
    for (const binary of [meshd, meshctl, checkpoint]) {
      if (!existsSync(binary)) {
        fail(
          `${binary} is missing; run \`${cargo} build -p mesh-daemon --bins --example capture-checkpoint\` first, or omit --skip-build`,
        );
      }
    }
    field("build", "reused local debug binaries");
    return;
  }

  step(offline ? "Building from the local Cargo cache" : "Building the real local binaries");
  const buildStarted = Date.now();
  // A cold build of this workspace takes minutes; streaming it is the only thing that
  // tells a first-time user a slow build apart from a hang.
  const cargoArguments = [
    "build",
    "-p",
    "mesh-daemon",
    "--bins",
    "--example",
    "capture-checkpoint",
  ];
  if (offline) cargoArguments.push("--offline");
  run(cargo, cargoArguments, { cwd: repo, stream: true });
  field("build", `ok in ${seconds(Date.now() - buildStarted)}`);
}

function ctl(args) {
  const result = run(meshctl, args, { cwd: repo });
  if (verbose) process.stdout.write(indent(result.stdout));
  return result.stdout;
}

function daemonArguments() {
  return [
    "--endpoint",
    socket,
    "--trusted-reviewer-key",
    reviewerPublicKey,
    "--checkpoint-idle-ms",
    "8",
    "--checkpoint-maximum-bytes",
    "16",
    "--checkpoint-maximum-interval-ms",
    "20",
  ];
}

function indent(text) {
  return text
    .split(/\r?\n/u)
    .map((line) => (line ? `   [raw] ${line}` : line))
    .join("\n");
}

function response(output) {
  const lines = jsonLines(output);
  assert(lines.length >= 2, `expected welcome and response JSON lines, received: ${output}`);
  assert(lines[0].t === "welcome", "meshctl did not negotiate a welcome");
  const last = lines.at(-1);
  return last.value ?? last;
}

function jsonLines(output) {
  return output
    .split(/\r?\n/u)
    .filter(Boolean)
    .map((line) => {
      try {
        return JSON.parse(line);
      } catch {
        fail(`expected JSON Lines output, received: ${line}`);
      }
    });
}

function run(command, args, { cwd = repo, stream = false } = {}) {
  const result = spawnSync(command, args, {
    cwd,
    encoding: "utf8",
    stdio: stream ? "inherit" : "pipe",
  });
  if (result.error) fail(`${command} could not start: ${result.error.message}`);
  if (result.status !== 0) {
    fail(`${command} ${args.join(" ")} exited ${result.status}\n${result.stderr ?? ""}`);
  }
  return result;
}

function capture(command, args, cwd) {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(command, args, { cwd, stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk) => (stdout += chunk));
    child.stderr.on("data", (chunk) => (stderr += chunk));
    child.once("error", reject);
    child.once("exit", (code) => resolvePromise({ code, stdout, stderr }));
  });
}

function firstJsonLine(child, timeoutMs, label) {
  const stream = child.stdout;
  stream.setEncoding("utf8");
  return new Promise((resolvePromise, reject) => {
    let buffered = "";
    const finish = (callback) => {
      clearTimeout(timer);
      stream.off("data", onData);
      child.off("exit", onExit);
      callback();
    };
    const timer = setTimeout(
      () => finish(() => reject(new Error(`${label} timed out; ${daemonError}`))),
      timeoutMs,
    );
    const onExit = (code) =>
      finish(() => reject(new Error(`${label} exited ${code} before readiness; ${daemonError}`)));
    const onData = (chunk) => {
      buffered += chunk;
      const newline = buffered.indexOf("\n");
      if (newline < 0) return;
      finish(() => {
        try {
          resolvePromise(JSON.parse(buffered.slice(0, newline)));
        } catch (error) {
          reject(new Error(`${label} was not JSON: ${error.message}`));
        }
      });
    };
    stream.on("data", onData);
    child.on("exit", onExit);
  });
}

function childExit(child, timeoutMs, label) {
  if (child.exitCode !== null) return Promise.resolve(child.exitCode);
  return new Promise((resolvePromise, reject) => {
    const timer = setTimeout(() => reject(new Error(`${label} timed out`)), timeoutMs);
    child.once("exit", (code) => {
      clearTimeout(timer);
      resolvePromise(code);
    });
  });
}

function delay(ms) {
  return new Promise((resolvePromise) => setTimeout(resolvePromise, ms));
}

function assert(condition, message) {
  if (!condition) fail(message);
}

function fail(message) {
  throw new Error(`demo: ${message}`);
}
