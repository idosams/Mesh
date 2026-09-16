#!/usr/bin/env node

import { spawn } from "node:child_process";
import { pathToFileURL } from "node:url";
import { runCapture } from "./vscode-macos-cdp.mjs";

export function parseCliArgs(argv) {
  if (argv.length !== 1) {
    throw new Error("usage: vscode-linux-cdp.mjs <capture-root>");
  }
  return argv[0];
}

export function launchLinuxCode(binary, args, options, spawnImpl = spawn) {
  return spawnImpl(binary, ["--no-sandbox", ...args], options);
}

export async function runLinuxCapture(root, options = {}) {
  const WebSocketImpl = options.WebSocketImpl
    ?? (await import("file:///usr/share/nodejs/ws/wrapper.mjs")).default;
  return runCapture(root, {
    codeBinary: "/usr/bin/code",
    launchCode: launchLinuxCode,
    saveModifiers: 2,
    WebSocketImpl,
    ...options,
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await runLinuxCapture(parseCliArgs(process.argv.slice(2)));
}
