#!/usr/bin/env node

import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  agreementPoints,
  dataSharingPoint,
  editorWindowPattern,
  parseCliArgs,
  parseGeometry,
  parseOwnedPid,
  parseWindowId,
  runCapture,
} from "./jetbrains-linux-xdotool.mjs";

assert.equal(parseCliArgs(["/tmp/capture"]), "/tmp/capture");
assert.throws(() => parseCliArgs([]), /usage/);
assert.equal(parseWindowId("12345\n"), "12345");
assert.throws(() => parseWindowId("123\n456\n"), /exactly one/);
assert.throws(() => parseWindowId("owned\n"), /exactly one/);
assert.equal(parseOwnedPid("12345\n", 12345), 12345);
assert.throws(() => parseOwnedPid("12346\n", 12345), /not owned/);
assert.throws(() => parseOwnedPid("unowned\n", 12345), /not owned/);

const geometry = parseGeometry("WINDOW=12345\nX=340\nY=170\nWIDTH=600\nHEIGHT=460\nSCREEN=0\n");
assert.deepEqual(geometry, { X: 340, Y: 170, WIDTH: 600, HEIGHT: 460 });
assert.deepEqual(agreementPoints(geometry), {
  checkbox: [382, 562],
  continueButton: [879, 601],
});
assert.deepEqual(dataSharingPoint(geometry), [663, 601]);
assert.equal(editorWindowPattern("/evidence/raw.1/doc.txt"), "^doc\\.txt - /evidence/raw\\.1$");
assert.throws(() => parseGeometry("X=0\nY=0\nWIDTH=10\nHEIGHT=10\n"), /invalid geometry/);

class FakeChild extends EventEmitter {
  constructor(pid, label, events) {
    super();
    this.pid = pid;
    this.label = label;
    this.events = events;
    this.exitCode = null;
    this.signalCode = null;
    queueMicrotask(() => this.emit("spawn"));
  }

  kill(signal) {
    this.events.push(`kill:${this.label}:${signal}`);
    this.signalCode = signal;
    this.emit("exit", null, signal);
    return true;
  }
}

const bed = mkdtempSync(join(tmpdir(), "mesh-jetbrains-driver-selftest-"));
const captureRoot = join(bed, "capture");
mkdirSync(captureRoot);
writeFileSync(join(captureRoot, "doc.txt"), "alpha original document\n");
const events = [];
let spawnCount = 0;
const spawnImpl = () => new FakeChild(++spawnCount === 1 ? 1001 : 1002, spawnCount === 1 ? "xvfb" : "idea", events);
const profile = join(bed, "profile");
const runCommand = async (_binary, args) => {
  if (args[0] === "search") return "777\n";
  if (args[0] === "getwindowpid") return "1002\n";
  if (args[0] === "getwindowgeometry") return "WINDOW=777\nX=340\nY=170\nWIDTH=600\nHEIGHT=460\nSCREEN=0\n";
  if (args[0] === "mousemove") throw new Error("injected dialog click failure");
  throw new Error(`unexpected fake command: ${args.join(" ")}`);
};

await assert.rejects(
  runCapture(captureRoot, {
    spawnImpl,
    createTempDir() {
      mkdirSync(profile);
      return profile;
    },
    removeTempDir(path) {
      events.push(`remove:${path}`);
      rmSync(path, { recursive: true, force: true });
    },
    runCommand,
    delayImpl: async () => {},
  }),
  /injected dialog click failure/,
);
assert.deepEqual(events, [
  "kill:idea:SIGTERM",
  "kill:xvfb:SIGTERM",
  `remove:${profile}`,
]);
assert.equal(events.at(-1), `remove:${profile}`, "profiles must be removed only after both owned children exit");
rmSync(bed, { recursive: true, force: true });

console.log("jetbrains-linux-xdotool selftest: PASS — dialog ownership and failure cleanup");
