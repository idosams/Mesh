#!/usr/bin/env node

import assert from "node:assert/strict";
import { launchLinuxCode, parseCliArgs } from "./vscode-linux-cdp.mjs";

assert.equal(parseCliArgs(["/tmp/capture"]), "/tmp/capture");
assert.throws(() => parseCliArgs([]), /usage/);
assert.throws(() => parseCliArgs(["/tmp/capture", "9331"]), /usage/);

const marker = {};
let received;
const returned = launchLinuxCode("/usr/bin/code", ["--user-data-dir", "/tmp/profile"], { stdio: "ignore" },
  (binary, args, options) => {
    received = { binary, args, options };
    return marker;
  });
assert.equal(returned, marker);
assert.deepEqual(received, {
  binary: "/usr/bin/code",
  args: ["--no-sandbox", "--user-data-dir", "/tmp/profile"],
  options: { stdio: "ignore" },
});

console.log("vscode-linux-cdp-selftest: PASS — fixed sandbox and CLI arguments");
