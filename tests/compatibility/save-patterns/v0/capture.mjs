#!/usr/bin/env node
//
// The polling capturer behind every `observed-by-polling` pattern in this corpus.
//
// README §5 described this method in prose and the original captures were taken with an
// ad-hoc loop that was never committed, so a second platform could not reproduce them and
// the Linux half of the capture matrix stayed empty. This is that loop, committed.
//
// It re-reads the whole tree in a tight loop and records, per entry, the inode, the size,
// the link count and whether it is a directory. A change is an entry appearing, vanishing,
// keeping its inode with a different size, or keeping its NAME with a DIFFERENT inode —
// the last is what makes a rename over a target visible without a syscall trace.
//
// It has the folder-watching backend's blind spots deliberately and by construction: it
// cannot see open, flush, fsync or close, and anything created and removed between two
// readings is invisible to it. That is the point. A capture taken here is honest evidence
// for a `folder` stream and never for a `mount` stream.
//
//   node capture.mjs --root <dir> --out <file.json> -- <command> [args...]

import { spawn } from "node:child_process";
import { lstatSync, mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { join, relative } from "node:path";

const argv = process.argv.slice(2);
const split = argv.indexOf("--");
if (split === -1) fail("usage: capture.mjs --root <dir> --out <file> -- <command> [args...]");

const options = argv.slice(0, split);
const command = argv.slice(split + 1);
if (command.length === 0) fail("no command given after --");

const root = valueOf(options, "--root") ?? fail("--root is required");
const out = valueOf(options, "--out") ?? fail("--out is required");
const settleMs = Number(valueOf(options, "--settle-ms") ?? 150);

mkdirSync(root, { recursive: true });

/** One reading of the whole tree: path -> identity. */
function read(dir) {
  const seen = new Map();
  const walk = (current) => {
    let entries;
    try {
      entries = readdirSync(current, { withFileTypes: true });
    } catch {
      return; // A directory removed mid-walk is a vanish, not an error.
    }
    for (const entry of entries) {
      const full = join(current, entry.name);
      let stat;
      try {
        stat = lstatSync(full);
      } catch {
        continue; // Removed between readdir and lstat: it vanished.
      }
      seen.set(relative(dir, full), {
        inode: Number(stat.ino),
        size: Number(stat.size),
        links: Number(stat.nlink),
        dir: stat.isDirectory(),
      });
      if (stat.isDirectory()) walk(full);
    }
  };
  walk(dir);
  return seen;
}

/** What changed between two readings, in the four shapes this method can see. */
function diff(before, after) {
  const changes = [];
  for (const [path, now] of after) {
    const then = before.get(path);
    if (!then) {
      changes.push({ change: "appeared", path, inode: now.inode, size: now.size });
    } else if (then.inode !== now.inode) {
      changes.push({ change: "replaced", path, from_inode: then.inode, inode: now.inode, size: now.size });
    } else if (then.size !== now.size) {
      changes.push({ change: "resized", path, inode: now.inode, from: then.size, size: now.size });
    }
  }
  for (const [path, then] of before) {
    if (!after.has(path)) changes.push({ change: "vanished", path, inode: then.inode });
  }
  return changes;
}

const readings = [];
let previous = read(root);
readings.push({ reading: 0, changes: [], note: "baseline" });

let polling = true;
const loop = (async () => {
  while (polling) {
    const now = read(root);
    const changes = diff(previous, now);
    if (changes.length > 0) readings.push({ reading: readings.length, changes });
    previous = now;
    await new Promise((resolve) => setImmediate(resolve));
  }
})();

const started = Date.now();
const child = spawn(command[0], command.slice(1), { cwd: root, stdio: ["ignore", "pipe", "pipe"] });
let stderr = "";
child.stderr.setEncoding("utf8");
child.stderr.on("data", (chunk) => (stderr += chunk));
const code = await new Promise((resolve) => child.once("exit", resolve));

// Keep reading after the process exits: a tool can leave work in flight.
await new Promise((resolve) => setTimeout(resolve, settleMs));
polling = false;
await loop;

const final = read(root);
const trailing = diff(previous, final);
if (trailing.length > 0) readings.push({ reading: readings.length, changes: trailing });

const capture = {
  contract: "mesh-save-capture/0",
  method: "observed-by-polling",
  platform: `${process.platform} ${process.arch}`,
  command,
  exit_code: code,
  elapsed_ms: Date.now() - started,
  stderr: stderr.slice(0, 2000),
  readings: readings.filter((entry) => entry.changes.length > 0 || entry.note),
};

writeFileSync(out, `${JSON.stringify(capture, null, 2)}\n`);
console.log(`capture: ${capture.readings.length} readings with changes, exit ${code}, ${capture.elapsed_ms} ms -> ${out}`);
if (code !== 0) console.log(`capture: command exited ${code}; stderr:\n${stderr.slice(0, 500)}`);

function valueOf(list, flag) {
  const at = list.indexOf(flag);
  return at === -1 ? undefined : list[at + 1];
}

// Throws rather than calling process.exit: an exit code is a verdict, and this file is
// used in expression position (`?? fail(...)`) where returning would be wrong anyway.
function fail(message) {
  throw new Error(`capture: ${message}`);
}
