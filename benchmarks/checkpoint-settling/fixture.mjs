#!/usr/bin/env node

import { createHash } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

export const FIXTURE = Object.freeze({
  contract: "mesh-checkpoint-settling-fixture/1",
  seed: 347,
  files: 2048,
  bytes_per_file: 4096,
  directories: 64,
  package_name: "mesh-settling-full-scale-fixture",
  package_version: "1.0.0",
});

function bytesFor(index) {
  const prefix = `mesh-settling-v1 seed=${FIXTURE.seed} file=${index.toString().padStart(4, "0")}\n`;
  const unit = `${prefix}${createHash("sha256").update(prefix).digest("hex")}\n`;
  return Buffer.from(unit.repeat(Math.ceil(FIXTURE.bytes_per_file / Buffer.byteLength(unit))).slice(0, FIXTURE.bytes_per_file));
}

export function fixtureEntries() {
  const packageJson = Buffer.from(`${JSON.stringify({
    name: FIXTURE.package_name,
    version: FIXTURE.package_version,
    description: "Deterministic offline package for TASK-347 settling measurements",
    files: ["files"],
    license: "UNLICENSED",
  }, null, 2)}\n`);
  const out = [["package.json", packageJson]];
  for (let index = 0; index < FIXTURE.files; index += 1) {
    const directory = `d${(index % FIXTURE.directories).toString().padStart(2, "0")}`;
    out.push([`files/${directory}/f${index.toString().padStart(4, "0")}.txt`, bytesFor(index)]);
  }
  return out.sort(([a], [b]) => a.localeCompare(b));
}

export function fixtureDigest() {
  const hash = createHash("sha256");
  for (const [path, bytes] of fixtureEntries()) hash.update(path).update("\0").update(bytes).update("\0");
  return `sha256:${hash.digest("hex")}`;
}

export function materializeFixture(directory) {
  for (const [path, bytes] of fixtureEntries()) {
    const target = join(directory, path);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, bytes);
  }
  return { ...FIXTURE, digest: fixtureDigest(), content_bytes: FIXTURE.files * FIXTURE.bytes_per_file };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const at = process.argv.indexOf("--out");
  const directory = at === -1 ? null : process.argv[at + 1];
  const summary = directory ? materializeFixture(resolve(directory)) : {
    ...FIXTURE,
    digest: fixtureDigest(),
    content_bytes: FIXTURE.files * FIXTURE.bytes_per_file,
  };
  process.stdout.write(`${JSON.stringify(summary)}\n`);
}
