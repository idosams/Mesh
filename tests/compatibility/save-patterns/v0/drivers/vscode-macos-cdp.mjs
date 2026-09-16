#!/usr/bin/env node

import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { pathToFileURL } from "node:url";

const CODE_BINARY = "/Applications/Visual Studio Code.app/Contents/MacOS/Code";
const original = "alpha original document\n";
const SOCKET_CONNECTING = 0;
const SOCKET_OPEN = 1;

export function parseCliArgs(argv) {
  if (argv.length !== 1) {
    throw new Error("usage: vscode-macos-cdp.mjs <capture-root>");
  }
  return argv[0];
}

export function parseDevToolsActivePort(contents) {
  const [portText, browserPath, ...extra] = contents.trim().split(/\r?\n/);
  const port = Number(portText);
  if (!Number.isInteger(port) || port <= 0 || port > 65_535) {
    throw new Error("DevToolsActivePort did not contain a valid port");
  }
  if (extra.length !== 0 || !/^\/devtools\/browser\/[A-Za-z0-9-]+$/.test(browserPath ?? "")) {
    throw new Error("DevToolsActivePort did not contain one browser endpoint");
  }
  return { port, browserPath };
}

export function ownedWebSocketUrl(port, path) {
  if (!Number.isInteger(port) || port <= 0 || port > 65_535 || !/^\/devtools\/(?:browser|page)\/[A-Za-z0-9-]+$/.test(path)) {
    throw new Error("refusing an unowned DevTools endpoint");
  }
  return `ws://127.0.0.1:${port}${path}`;
}

export function validateTargetEntry(entry, port, documentName) {
  if (entry?.type !== "page"
    || typeof entry.id !== "string"
    || !/^[A-Za-z0-9-]+$/.test(entry.id)
    || typeof entry.webSocketDebuggerUrl !== "string") return null;
  let endpoint;
  try {
    endpoint = new URL(entry.webSocketDebuggerUrl);
  } catch {
    return null;
  }
  if (!['127.0.0.1', 'localhost'].includes(endpoint.hostname) || Number(endpoint.port) !== port) return null;
  if (endpoint.pathname !== `/devtools/page/${entry.id}`) return null;
  if (typeof entry.title !== "string" || !entry.title.includes(documentName)) return null;
  return ownedWebSocketUrl(port, endpoint.pathname);
}

export async function terminateOwnedChild(child, socket, browserOwned) {
  if (browserOwned && socket?.readyState === SOCKET_OPEN) {
    try {
      socket.send(JSON.stringify({ id: 1_000_000, method: "Browser.close", params: {} }));
    } catch {}
  }
  if (!(await waitForExit(child, 1_500))) {
    child.kill("SIGTERM");
  }
  if (!(await waitForExit(child, 3_000))) {
    child.kill("SIGKILL");
    await waitForExit(child, 3_000);
  }
  if (child.exitCode === null && child.signalCode === null) {
    throw new Error("the owned VS Code process did not terminate");
  }
  if (socket?.readyState === SOCKET_OPEN || socket?.readyState === SOCKET_CONNECTING) {
    try {
      socket.close();
    } catch {}
  }
}

export async function runCapture(root, {
  codeBinary = CODE_BINARY,
  launchCode = defaultLaunchCode,
  createTempDir = (prefix) => mkdtempSync(join(tmpdir(), prefix)),
  removeTempDir = (path) => rmSync(path, { recursive: true, force: true }),
  discoverEndpoint = waitForOwnedEndpoint,
  fetchJson = fetchOwnedJson,
  WebSocketImpl = WebSocket,
  commandTimeoutMs = 5_000,
  saveModifiers = 4,
} = {}) {
  const documentPath = join(root, "doc.txt");
  const edited = `VS Code edit: ${original}`;
  if (readFileSync(documentPath, "utf8") !== original) {
    throw new Error("doc.txt must contain the exact baseline: alpha original document\\n");
  }

  const profile = createTempDir("mesh-vscode-profile-");
  const extensions = createTempDir("mesh-vscode-extensions-");
  let child;
  let socket;
  let cdp;
  let browserOwned = false;

  try {
    child = launchCode(codeBinary, [
      "--user-data-dir", profile,
      "--extensions-dir", extensions,
      "--disable-extensions",
      "--disable-workspace-trust",
      "--skip-welcome",
      "--new-window",
      "--remote-debugging-address=127.0.0.1",
      "--remote-debugging-port=0",
      documentPath,
    ], { stdio: "ignore" });
    await new Promise((resolve, reject) => {
      child.once("spawn", resolve);
      child.once("error", reject);
    });

    const deadline = Date.now() + 20_000;
    const { port, browserPath } = await discoverEndpoint(profile, child, deadline);
    const version = await fetchJson(port, "/json/version", commandTimeoutMs);
    const browserEndpoint = new URL(version.webSocketDebuggerUrl);
    if (!['127.0.0.1', 'localhost'].includes(browserEndpoint.hostname)
      || Number(browserEndpoint.port) !== port
      || browserEndpoint.pathname !== browserPath) {
      throw new Error("the browser endpoint does not match this profile's DevToolsActivePort");
    }
    browserOwned = true;

    let targetUrl;
    while (Date.now() < deadline) {
      const list = await fetchJson(port, "/json/list", commandTimeoutMs);
      targetUrl = list.map((entry) => validateTargetEntry(entry, port, basename(documentPath))).find(Boolean);
      if (targetUrl) break;
      await delay(100);
    }
    if (!targetUrl) throw new Error("the owned VS Code instance did not publish the supplied document");

    socket = new WebSocketImpl(targetUrl);
    await waitForSocketOpen(socket, commandTimeoutMs);
    cdp = createCdpCaller(socket, commandTimeoutMs);
    const { call } = cdp;

    await call("Page.bringToFront");
    let ready = false;
    while (Date.now() < deadline) {
      const result = await call("Runtime.evaluate", {
        expression: `(() => {
          const tab = document.querySelector('.tab.active .label-name');
          const input = document.querySelectorAll('.native-edit-context[role="textbox"]')[0];
          if (!tab || tab.textContent.trim() !== ${JSON.stringify(basename(documentPath))} || !input) return false;
          input.focus();
          return document.activeElement === input;
        })()`,
        returnByValue: true,
      });
      if (result.result?.value === true) {
        ready = true;
        break;
      }
      await delay(100);
    }
    if (!ready) throw new Error("the supplied document never became keyboard-ready in the owned VS Code instance");

    // Native EditContext consumes the first event if text arrives immediately after focus.
    await delay(500);
    await typeText(call, "VS Code edit: ");
    await key(call, "s", "KeyS", 83, saveModifiers);

    const saveDeadline = Date.now() + 5_000;
    while (Date.now() < saveDeadline && readFileSync(documentPath, "utf8") !== edited) {
      await delay(50);
    }
    const actual = readFileSync(documentPath, "utf8");
    if (actual !== edited) {
      throw new Error(`VS Code persisted unexpected bytes: ${JSON.stringify(actual)}`);
    }
  } finally {
    try {
      cdp?.dispose(new Error("the capture is closing"));
      if (child) await terminateOwnedChild(child, socket, browserOwned);
    } finally {
      removeTempDir(profile);
      removeTempDir(extensions);
    }
  }
}

export function createCdpCaller(socket, timeoutMs) {
  let nextId = 1;
  const pending = new Map();

  function rejectPending(error) {
    for (const waiter of pending.values()) waiter.reject(error);
    pending.clear();
  }

  function onMessage(event) {
    let message;
    try {
      message = JSON.parse(String(event.data));
    } catch {
      rejectPending(new Error("owned DevTools sent malformed JSON"));
      return;
    }
    if (!Number.isInteger(message.id)) return;
    const waiter = pending.get(message.id);
    if (!waiter) return;
    pending.delete(message.id);
    if (message.error) waiter.reject(new Error(JSON.stringify(message.error)));
    else waiter.resolve(message.result);
  }

  function onClose() {
    rejectPending(new Error("owned DevTools closed with a command in flight"));
  }

  function onError() {
    rejectPending(new Error("owned DevTools failed with a command in flight"));
  }

  socket.addEventListener("message", onMessage);
  socket.addEventListener("close", onClose);
  socket.addEventListener("error", onError);

  function call(method, params = {}) {
    if (socket.readyState !== SOCKET_OPEN) {
      return Promise.reject(new Error(`owned DevTools is not open for ${method}`));
    }
    const id = nextId++;
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        pending.delete(id);
        reject(new Error(`owned DevTools timed out during ${method}`));
      }, timeoutMs);
      pending.set(id, {
        resolve(value) {
          clearTimeout(timeout);
          resolve(value);
        },
        reject(error) {
          clearTimeout(timeout);
          reject(error);
        },
      });
      try {
        socket.send(JSON.stringify({ id, method, params }));
      } catch (error) {
        pending.get(id)?.reject(error);
        pending.delete(id);
      }
    });
  }

  function dispose(error) {
    socket.removeEventListener("message", onMessage);
    socket.removeEventListener("close", onClose);
    socket.removeEventListener("error", onError);
    rejectPending(error);
  }

  return { call, dispose };
}

async function waitForOwnedEndpoint(profile, child, deadline) {
  const activePortPath = join(profile, "DevToolsActivePort");
  while (Date.now() < deadline) {
    if (child.exitCode !== null || child.signalCode !== null) {
      throw new Error("the owned VS Code process exited before publishing DevToolsActivePort");
    }
    try {
      return parseDevToolsActivePort(readFileSync(activePortPath, "utf8"));
    } catch {}
    await delay(100);
  }
  throw new Error("the owned VS Code process did not publish DevToolsActivePort");
}

export async function fetchOwnedJson(port, path, timeoutMs, fetchImpl = fetch) {
  const controller = new AbortController();
  try {
    return await withTimeout((async () => {
      const response = await fetchImpl(`http://127.0.0.1:${port}${path}`, { signal: controller.signal });
      if (!response.ok) throw new Error(`owned DevTools endpoint returned HTTP ${response.status}`);
      return response.json();
    })(), timeoutMs, `owned DevTools timed out fetching ${path}`, () => controller.abort());
  } finally {
    controller.abort();
  }
}

async function waitForSocketOpen(socket, timeoutMs) {
  await withTimeout(new Promise((resolve, reject) => {
    function cleanup() {
      socket.removeEventListener("open", onOpen);
      socket.removeEventListener("error", onError);
      socket.removeEventListener("close", onClose);
    }
    function onOpen() {
      cleanup();
      resolve();
    }
    function onError() {
      cleanup();
      reject(new Error("owned DevTools failed during WebSocket handshake"));
    }
    function onClose() {
      cleanup();
      reject(new Error("owned DevTools closed during WebSocket handshake"));
    }
    socket.addEventListener("open", onOpen, { once: true });
    socket.addEventListener("error", onError, { once: true });
    socket.addEventListener("close", onClose, { once: true });
  }), timeoutMs, "owned DevTools WebSocket handshake timed out", () => socket.close());
}

function defaultLaunchCode(binary, args, options) {
  return spawn(binary, args, options);
}

function withTimeout(promise, timeoutMs, message, onTimeout) {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      try {
        onTimeout?.();
      } finally {
        reject(new Error(message));
      }
    }, timeoutMs);
    promise.then(
      (value) => {
        clearTimeout(timeout);
        resolve(value);
      },
      (error) => {
        clearTimeout(timeout);
        reject(error);
      },
    );
  });
}

async function typeText(call, text) {
  for (const character of text) {
    const virtualKeyCode = character.toUpperCase().charCodeAt(0);
    await call("Input.dispatchKeyEvent", {
      type: "keyDown",
      key: character,
      code: `Key${character.toUpperCase()}`,
      text: character,
      unmodifiedText: character,
      windowsVirtualKeyCode: virtualKeyCode,
      nativeVirtualKeyCode: virtualKeyCode,
    });
    await call("Input.dispatchKeyEvent", {
      type: "keyUp",
      key: character,
      code: `Key${character.toUpperCase()}`,
      windowsVirtualKeyCode: virtualKeyCode,
      nativeVirtualKeyCode: virtualKeyCode,
    });
  }
}

async function key(call, keyValue, code, virtualKeyCode, modifiers) {
  await call("Input.dispatchKeyEvent", {
    type: "keyDown",
    key: keyValue,
    code,
    modifiers,
    windowsVirtualKeyCode: virtualKeyCode,
    nativeVirtualKeyCode: virtualKeyCode,
  });
  await call("Input.dispatchKeyEvent", {
    type: "keyUp",
    key: keyValue,
    code,
    modifiers,
    windowsVirtualKeyCode: virtualKeyCode,
    nativeVirtualKeyCode: virtualKeyCode,
  });
}

async function waitForExit(child, timeoutMs) {
  if (child.exitCode !== null || child.signalCode !== null) return true;
  return new Promise((resolve) => {
    const timeout = setTimeout(() => {
      child.removeListener("exit", onExit);
      resolve(false);
    }, timeoutMs);
    function onExit() {
      clearTimeout(timeout);
      resolve(true);
    }
    child.once("exit", onExit);
  });
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await runCapture(parseCliArgs(process.argv.slice(2)));
}
