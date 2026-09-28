import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");

for (const relativeOverride of [false, true]) {
  test(`demo checks the ${relativeOverride ? "relative" : "absolute"} Cargo target override`, {
    skip: process.platform === "win32",
  }, () => {
    const directory = mkdtempSync(join(tmpdir(), "mesh-demo-target-test-"));
    const target = join(directory, "absent-build");
    let scratch;
    try {
      const result = spawnSync(process.execPath, [join(repo, "examples/local-daemon-demo.mjs"), "--skip-build"], {
        // Invocation cwd must not change how Cargo's repo-relative target override is resolved.
        cwd: directory,
        env: { ...process.env, CARGO_TARGET_DIR: relativeOverride ? relative(repo, target) : target },
        encoding: "utf8",
        timeout: 30_000,
      });
      const output = `${result.stdout ?? ""}\n${result.stderr ?? ""}`;
      scratch = /Demo files retained at (\/[^\n]+)/u.exec(output)?.[1];
      assert.ifError(result.error);
      assert.equal(result.status, 1);
      assert.ok(output.includes(`${join(target, "debug", "meshd")} is missing`), output);
    } finally {
      // These fixtures are created by this test's child invocation, never existing user work.
      if (scratch?.startsWith("/tmp/mesh-demo-")) rmSync(scratch, { recursive: true, force: true });
      rmSync(directory, { recursive: true, force: true });
    }
  });
}
