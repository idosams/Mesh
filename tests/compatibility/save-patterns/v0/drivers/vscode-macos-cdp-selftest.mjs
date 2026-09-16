#!/usr/bin/env node

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  createCdpCaller,
  fetchOwnedJson,
  ownedWebSocketUrl,
  parseCliArgs,
  parseDevToolsActivePort,
  runCapture,
  terminateOwnedChild,
  validateTargetEntry,
} from "./vscode-macos-cdp.mjs";

assert.equal(parseCliArgs(["/tmp/capture"]), "/tmp/capture");
assert.throws(() => parseCliArgs(["/tmp/capture", "9331"]), /usage/);

assert.deepEqual(parseDevToolsActivePort("50123\n/devtools/browser/owned-123\n"), {
  port: 50123,
  browserPath: "/devtools/browser/owned-123",
});
for (const invalid of [
  "9331\nws://example.test/devtools/browser/not-owned\n",
  "0\n/devtools/browser/not-owned\n",
  "50123\n/devtools/page/not-a-browser\n",
  "50123\n/devtools/browser/owned\nextra\n",
]) {
  assert.throws(() => parseDevToolsActivePort(invalid));
}

assert.equal(
  ownedWebSocketUrl(50123, "/devtools/page/owned-page"),
  "ws://127.0.0.1:50123/devtools/page/owned-page",
);
assert.equal(validateTargetEntry({
  type: "page",
  id: "owned-page",
  title: "doc.txt — Visual Studio Code",
  webSocketDebuggerUrl: "ws://localhost:50123/devtools/page/owned-page",
}, 50123, "doc.txt"), "ws://127.0.0.1:50123/devtools/page/owned-page");
assert.equal(validateTargetEntry({
  type: "page",
  id: "other-page",
  title: "doc.txt — unrelated browser",
  webSocketDebuggerUrl: "ws://127.0.0.1:9331/devtools/page/other-page",
}, 50123, "doc.txt"), null, "an occupied unrelated endpoint must not be selected");
assert.equal(validateTargetEntry({
  type: "page",
  id: "owned-page",
  title: "other.txt — Visual Studio Code",
  webSocketDebuggerUrl: "ws://127.0.0.1:50123/devtools/page/owned-page",
}, 50123, "doc.txt"), null, "the endpoint must expose the supplied document");
assert.equal(validateTargetEntry({
  type: "page",
  id: "owned-page",
  title: "doc.txt — Visual Studio Code",
  webSocketDebuggerUrl: "not a URL",
}, 50123, "doc.txt"), null, "malformed endpoint metadata must be refused");

const handshakeFailureChild = await launchSleeper();
let unrelatedCloseCount = 0;
await terminateOwnedChild(handshakeFailureChild, {
  readyState: WebSocket.OPEN,
  send() { unrelatedCloseCount += 1; },
  close() {},
}, false);
assert.equal(unrelatedCloseCount, 0, "a failed ownership handshake must not close an endpoint");
assert.notEqual(handshakeFailureChild.signalCode, null, "handshake failure must terminate the exact child");

const ownedChild = await launchSleeper();
let ownedCloseCount = 0;
await terminateOwnedChild(ownedChild, {
  readyState: WebSocket.OPEN,
  send(message) {
    assert.equal(JSON.parse(message).method, "Browser.close");
    ownedCloseCount += 1;
  },
  close() {},
}, true);
assert.equal(ownedCloseCount, 1, "only the profile-proven browser receives Browser.close");
assert.notEqual(ownedChild.signalCode, null, "cleanup must wait for the exact child to terminate");

const brokenSocketChild = await launchSleeper();
await terminateOwnedChild(brokenSocketChild, {
  readyState: WebSocket.OPEN,
  send() { throw new Error("simulated handshake teardown failure"); },
  close() { throw new Error("simulated close failure"); },
}, true);
assert.notEqual(brokenSocketChild.signalCode, null, "socket teardown failure must not leak the exact child");

class TestSocket extends EventTarget {
  readyState = WebSocket.OPEN;
  send() {}
  close() { this.readyState = WebSocket.CLOSED; }
}

class InFlightCloseSocket extends EventTarget {
  readyState = WebSocket.CONNECTING;

  constructor() {
    super();
    queueMicrotask(() => {
      this.readyState = WebSocket.OPEN;
      this.dispatchEvent(new Event("open"));
    });
  }

  send(message) {
    if (JSON.parse(message).method !== "Page.bringToFront") return;
    queueMicrotask(() => {
      this.readyState = WebSocket.CLOSED;
      this.dispatchEvent(new Event("close"));
    });
  }

  close() {
    this.readyState = WebSocket.CLOSED;
    this.dispatchEvent(new Event("close"));
  }
}

const silentSocket = new TestSocket();
const silentCdp = createCdpCaller(silentSocket, 10);
await assert.rejects(silentCdp.call("Runtime.evaluate"), /timed out/);
silentCdp.dispose(new Error("test complete"));

const malformedSocket = new TestSocket();
const malformedCdp = createCdpCaller(malformedSocket, 1_000);
const malformedCall = malformedCdp.call("Runtime.evaluate");
malformedSocket.dispatchEvent(new MessageEvent("message", { data: "{" }));
await assert.rejects(malformedCall, /malformed JSON/);
malformedCdp.dispose(new Error("test complete"));

await assert.rejects(
  fetchOwnedJson(50123, "/json/list", 10, () => new Promise(() => {})),
  /timed out fetching/,
);

const fakeRoot = mkdtempSync(join(tmpdir(), "mesh-vscode-inflight-test-"));
writeFileSync(join(fakeRoot, "doc.txt"), "alpha original document\n");
const createdDirs = [];
const removedDirs = [];
let inFlightChild;
try {
  await assert.rejects(runCapture(fakeRoot, {
    createTempDir(prefix) {
      const path = mkdtempSync(join(fakeRoot, prefix));
      createdDirs.push(path);
      return path;
    },
    removeTempDir(path) {
      assert.ok(inFlightChild.exitCode !== null || inFlightChild.signalCode !== null,
        "the exact child must exit before a created directory is removed");
      removedDirs.push(path);
      rmSync(path, { recursive: true, force: true });
    },
    launchCode() {
      inFlightChild = launchSleeperNow();
      return inFlightChild;
    },
    discoverEndpoint: async () => ({ port: 50123, browserPath: "/devtools/browser/owned-123" }),
    fetchJson: async (_port, path) => path === "/json/version"
      ? { webSocketDebuggerUrl: "ws://localhost:50123/devtools/browser/owned-123" }
      : [{
          type: "page",
          id: "owned-page",
          title: "doc.txt — Visual Studio Code",
          webSocketDebuggerUrl: "ws://localhost:50123/devtools/page/owned-page",
        }],
    WebSocketImpl: InFlightCloseSocket,
    commandTimeoutMs: 100,
  }), /closed with a command in flight/);
  assert.notEqual(inFlightChild.signalCode, null, "an in-flight disconnect must terminate the exact child");
  assert.deepEqual(removedDirs, createdDirs, "every created profile directory must be removed after child exit");
  assert.ok(createdDirs.every((path) => !existsSync(path)), "no created profile directory may remain");
} finally {
  rmSync(fakeRoot, { recursive: true, force: true });
}

console.log("vscode-macos-cdp-selftest: PASS (endpoint ownership, bounded transport and cleanup)");

async function launchSleeper() {
  const child = launchSleeperNow();
  await new Promise((resolve, reject) => {
    child.once("spawn", resolve);
    child.once("error", reject);
  });
  return child;
}

function launchSleeperNow() {
  return spawn(process.execPath, ["-e", "setInterval(() => {}, 1_000)"], { stdio: "ignore" });
}
