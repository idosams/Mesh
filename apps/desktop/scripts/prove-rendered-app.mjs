#!/usr/bin/env node

import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { createHash, randomBytes } from 'node:crypto';
import { chmod, lstat, mkdir, mkdtemp, readFile, readdir, readlink, realpath, rm, writeFile } from 'node:fs/promises';
import { createConnection } from 'node:net';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import {
  parseProofArguments,
  PROVE_RENDERED_APP_USAGE,
} from './prove-rendered-app-args.mjs';
import {
  RENDERER_PROOF_PREFIX,
  rendererProofReportsFromText,
} from './renderer-proof-protocol.mjs';
import { readRenderedProofImage } from './rendered-proof-image.mjs';
import { writeNewPrivateScreenshot } from './proof-screenshot-output.mjs';
import {
  assertSeedRepositoryUnchanged,
  inspectSeedRepository,
  restoreSeedRepository,
} from './proof-seed-repository.mjs';
import { proofDaemonIdleTimeoutMs } from './proof-daemon-timeout.mjs';
import { mirrorOrdinaryDirectories } from './proof-private-export-destination.mjs';

const options = parseProofArguments(process.argv.slice(2));
if (options.help) {
  process.stdout.write(`${PROVE_RENDERED_APP_USAGE}\n`);
  process.exit(0);
}

const here = dirname(fileURLToPath(import.meta.url));
const repository = resolve(here, '../../..');
const app = resolve(process.env.MESH_LOCAL_APP || join(repository, 'target/release/bundle/macos/Mesh.app'));
const executable = join(app, 'Contents/MacOS/mesh-desktop');
const screenshot = options.screenshot === null ? null : resolve(options.screenshot);
const requestedSeedRepository = options.seedRepository === null
  ? null
  : resolve(options.seedRepository);

// The desktop host creates its Unix socket below HOME/Library/Application Support. macOS limits
// sockaddr_un paths to 104 bytes, so the usual /var/folders/... TMPDIR is not a safe proof root.
const scratch = await realpath(await mkdtemp('/tmp/mesh-app-'));
const home = join(scratch, 'home');
const source = join(scratch, 'source');
const emptySource = `${source}-empty`;
const privateExport = join(home, 'private-export');
const appData = join(home, 'Library/Application Support/dev.mesh.desktop');
const endpoint = join(appData, 'runtime/daemon.sock');
const stableFolder = join(appData, 'native-workspace/current');
const versionStores = join(appData, 'workspace-versions');
const forkStore = join(versionStores, 'point-proof.mesh');
const probe = join(scratch, 'window-proof');
const IPC_VERSION = 7;
const MAX_DAEMON_MESSAGE_BYTES = 16 * 1024 * 1024;
const CHUNK_DATA_BYTES = 30_000;
const AGENT_PROOF_RESULT_PATH = 'agent-proof-result.txt';
const AGENT_PROOF_RESULT = 'packaged agent handoff result\n';
const AGENT_PROOF_IMAGE_PATH = 'agent-proof-result.png';
const FILES_PROOF_IMAGE_PATH = 'assets/mesh-proof.png';
// Keep a real, decodable raster beside the text result so the review proof can exercise both the
// exact text diff and a native-open type backed by the system image viewer. Text-file default
// associations are user-configurable and may be absent on an otherwise valid clean Mac.
// build.rs carries the unbranded development PNG because Tauri generates the ignored icon file at
// build time. Read that public-source constant directly so an extracted delivery can prove itself
// from a clean source checkout without depending on a generated internal-worktree file.
const AGENT_PROOF_IMAGE = await readRenderedProofImage();

const seedRepository = await inspectSeedRepository(requestedSeedRepository);
const proofEnvironment = {
  ...process.env,
  HOME: home,
  // Foundation does not derive NSHomeDirectory from HOME once the process is running. Without
  // this documented test override, a packaged proof can attach to the real user's Application
  // Support directory and collide with an already-running Mesh alpha app.
  CFFIXED_USER_HOME: home,
  TMPDIR: scratch,
};
delete proofEnvironment.MESH_RENDERER_PROOF_NONCE;
delete proofEnvironment.MESH_RENDERER_PROOF_SURFACE;
delete proofEnvironment.MESH_RENDERER_PROOF_SOURCE;
delete proofEnvironment.MESH_RENDERER_PROOF_DESTINATION;
delete proofEnvironment.MESH_RENDERER_PROOF_SCREENSHOT;

await mkdir(source);
await mkdir(emptySource);
if (seedRepository !== null) {
  restoreSeedRepository(seedRepository, source, scratch);
}
await mkdir(privateExport, { recursive: true, mode: 0o700 });
await mkdir(join(source, 'assets'), { recursive: true });
await writeFile(join(source, 'notes.txt'), 'first saved version\n', { flag: 'wx' });
await writeFile(join(source, 'run.sh'), "#!/bin/sh\nprintf 'native mesh\\n'\n", { flag: 'wx' });
await writeFile(join(source, FILES_PROOF_IMAGE_PATH), AGENT_PROOF_IMAGE, { flag: 'wx' });
await chmod(join(source, 'run.sh'), 0o755);
// macOS exposes /tmp through a symlink to /private/tmp. The daemon returns canonical roots, so
// compare filesystem identities instead of requiring temporary path spellings to survive.
const canonicalSource = await realpath(source);
await mkdir(appData, { recursive: true });
await chmod(appData, 0o700);
execFileSync('/usr/bin/xcrun', [
  'clang', '-fobjc-arc', '-framework', 'Foundation', '-framework', 'CoreGraphics',
  join(here, 'window-proof.m'), '-o', probe,
], { stdio: 'inherit' });

function running(child) {
  return child.exitCode === null && child.signalCode === null;
}

async function assertExecutable(path, message) {
  const metadata = await lstat(path);
  assert.notEqual(metadata.mode & 0o111, 0, message);
}

async function waitForPrivateExportReceipt() {
  if (seedRepository !== null) return;
  const expected = [
    'notes.txt',
    'run.sh',
    FILES_PROOF_IMAGE_PATH,
    AGENT_PROOF_RESULT_PATH,
    AGENT_PROOF_IMAGE_PATH,
  ];
  let lastError = null;
  let visible = [];
  for (let attempt = 0; attempt < 200; attempt += 1) {
    try {
      await Promise.all(expected.map((relative) => lstat(join(privateExport, relative))));
      return;
    } catch (error) {
      if (error?.code !== 'ENOENT') throw error;
      lastError = error;
      visible = await readdir(privateExport, { recursive: true });
      await delay(50);
    }
  }
  const state = await requestWorkspaceState();
  throw new Error(
    `workspace files=${state.file_histories.length}; the packaged private export reported completion before its exact files were visible: ${lastError?.message || 'missing receipt'}; visible=${visible.sort().join(',')}`,
  );
}

async function stop(child) {
  if (running(child)) child.kill('SIGTERM');
  await Promise.race([
    new Promise((resolveExit) => child.once('exit', resolveExit)),
    delay(2_000),
  ]);
  if (running(child)) {
    child.kill('SIGKILL');
    await new Promise((resolveExit) => child.once('exit', resolveExit));
  }
}

async function waitForWindow(child, stderr) {
  let probeFailure = '';
  for (let attempt = 0; attempt < 50; attempt += 1) {
    if (!running(child)) throw new Error(`the bundled app exited before drawing: ${stderr()}`);
    try {
      return JSON.parse(execFileSync(probe, [String(child.pid)], {
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'pipe'],
      }));
    } catch (error) {
      probeFailure = error.stderr?.toString().trim() || error.message;
      await delay(200);
    }
  }
  throw new Error(
    `the bundled app did not expose its expected on-screen window: app=${stderr()} probe=${probeFailure}`,
  );
}

function rendererProofSession(surface, expectedWorkspace = null) {
  const nonce = randomBytes(32).toString('hex');
  const proofSource = surface === 'agent-handoff' ? expectedWorkspace : canonicalSource;
  return {
    nonce,
    surface,
    source: proofSource,
    environment: {
      ...proofEnvironment,
      MESH_RENDERER_PROOF_NONCE: nonce,
      MESH_RENDERER_PROOF_SURFACE: surface,
      ...(surface === 'onboarding' || surface === 'private-export' || surface === 'agent-handoff'
        ? { MESH_RENDERER_PROOF_SOURCE: proofSource }
        : {}),
      ...(surface === 'private-export'
        ? { MESH_RENDERER_PROOF_DESTINATION: privateExport }
        : {}),
      ...(surface === 'files' && screenshot
        ? { MESH_RENDERER_PROOF_SCREENSHOT: '1' }
        : {}),
    },
  };
}

async function waitForRendererProof(child, stderr, session) {
  let lastDiagnostic = '';
  let agentResultWritten = false;
  // Onboarding contains six bounded 40-second setup waits followed by the bounded 15-minute
  // confirmed import. Agent finish can likewise spend 15 minutes on its complete post-release
  // scan and authenticated save. Keep each process-level deadline beyond its composed maximum so
  // the renderer's stage-specific, secret-free failure marker wins instead of an outer race.
  const maximumAttempts = session.surface === 'onboarding'
    ? 26_400
    : session.surface === 'agent-handoff'
      ? 22_800
      : 2_400;
  for (let attempt = 0; attempt < maximumAttempts; attempt += 1) {
    if (!running(child)) throw new Error(`the bundled app exited before renderer proof: ${stderr()}`);
    const current = stderr();
    const lastNewline = current.lastIndexOf('\n');
    const complete = lastNewline < 0 ? '' : current.slice(0, lastNewline + 1);
    if (
      session.surface === 'agent-handoff'
      && !agentResultWritten
      && complete.includes('mesh-renderer-proof-checkpoint:agent-handoff-launched')
    ) {
      await writeFile(join(session.source, AGENT_PROOF_RESULT_PATH), AGENT_PROOF_RESULT, {
        encoding: 'utf8',
        flag: 'wx',
        mode: 0o600,
      });
      await writeFile(join(session.source, AGENT_PROOF_IMAGE_PATH), AGENT_PROOF_IMAGE, {
        flag: 'wx',
        mode: 0o600,
      });
      agentResultWritten = true;
    }
    if (complete.includes(RENDERER_PROOF_PREFIX)) {
      return rendererProofReportsFromText(complete, session)[0];
    }
    if (complete.includes(`mesh-renderer-proof-failure:${session.surface}:`)) {
      throw new Error(`the bundled app reported a ${session.surface} renderer failure: ${current.trim()}`);
    }
    lastDiagnostic = current.trim();
    await delay(50);
  }
  throw new Error(
    `the bundled app did not complete ${session.surface} renderer proof: ${lastDiagnostic}`,
  );
}

function requestDaemon(method, params = {}, totalTimeoutMs = proofDaemonIdleTimeoutMs(method)) {
  return new Promise((resolveRequest, rejectRequest) => {
    const socket = createConnection(endpoint);
    let settled = false;
    let buffer = '';
    let partial = null;
    const totalTimer = globalThis.setTimeout(
      () => finish(new Error(`the embedded daemon did not complete ${method} within the overall deadline`)),
      totalTimeoutMs,
    );
    const finish = (error, value) => {
      if (settled) return;
      settled = true;
      clearTimeout(totalTimer);
      socket.destroy();
      if (error) rejectRequest(error);
      else resolveRequest(value);
    };
    socket.setTimeout(
      proofDaemonIdleTimeoutMs(method),
      () => finish(new Error(`the embedded daemon did not answer ${method} in its bounded time`)),
    );
    socket.on('error', (error) => finish(error));
    socket.on('close', () => {
      if (!settled) finish(new Error('the embedded daemon closed before returning workspace state'));
    });
    socket.on('connect', () => {
      socket.write(`${JSON.stringify({
        t: 'hello', id: 1, protocol: 'mesh-ipc', versions: [IPC_VERSION], session: 'mesh-rendered-proof',
      })}\n`);
    });
    socket.on('data', (chunk) => {
      buffer += chunk.toString('utf8');
      for (;;) {
        const newline = buffer.indexOf('\n');
        if (newline < 0) break;
        const line = buffer.slice(0, newline);
        buffer = buffer.slice(newline + 1);
        let message = JSON.parse(line);
        if (message.t === 'chunk') {
          if (
            !Number.isSafeInteger(message.id) ||
            !Number.isSafeInteger(message.index) ||
            !Number.isSafeInteger(message.parts) ||
            !Number.isSafeInteger(message.total_bytes) ||
            message.parts <= 0 ||
            message.total_bytes < 65_536 ||
            message.total_bytes > MAX_DAEMON_MESSAGE_BYTES ||
            typeof message.hex !== 'string' ||
            message.hex.length === 0 ||
            message.hex.length % 2 !== 0 ||
            message.hex.length > CHUNK_DATA_BYTES * 2 ||
            !/^[0-9a-f]+$/u.test(message.hex)
          ) {
            finish(new Error('the embedded daemon returned an invalid bounded response frame'));
            return;
          }
          if (partial === null) {
            if (message.index !== 0) {
              finish(new Error('the embedded daemon response did not begin at frame zero'));
              return;
            }
            partial = {
              id: message.id,
              parts: message.parts,
              totalBytes: message.total_bytes,
              next: 0,
              chunks: [],
              bytes: 0,
            };
          }
          const decoded = Buffer.from(message.hex, 'hex');
          if (
            partial.id !== message.id ||
            partial.parts !== message.parts ||
            partial.totalBytes !== message.total_bytes ||
            partial.next !== message.index ||
            message.index >= message.parts ||
            partial.bytes + decoded.length > partial.totalBytes
          ) {
            finish(new Error('the embedded daemon response frames changed identity or order'));
            return;
          }
          partial.chunks.push(decoded);
          partial.bytes += decoded.length;
          partial.next += 1;
          if (partial.next !== partial.parts) continue;
          if (partial.bytes !== partial.totalBytes) {
            finish(new Error('the embedded daemon response did not match its declared size'));
            return;
          }
          message = JSON.parse(Buffer.concat(partial.chunks).toString('utf8'));
          if (message.t === 'chunk' || message.id !== partial.id) {
            finish(new Error('the embedded daemon reconstructed the wrong logical response'));
            return;
          }
          partial = null;
        } else if (partial !== null) {
          finish(new Error('the embedded daemon interrupted a framed response'));
          return;
        }
        if (message.t === 'welcome' && message.id === 1) {
          assert.equal(message.version, IPC_VERSION, 'the packaged daemon did not negotiate IPC v7');
          assert.equal(message.surface_version, IPC_VERSION, 'the packaged daemon exposed a stale surface');
          socket.write(`${JSON.stringify({
            t: 'call', id: 2, method, version: IPC_VERSION, params,
          })}\n`);
        } else if (message.t === 'result' && message.id === 2) {
          finish(null, message.value);
        } else if ((message.t === 'failed' || message.t === 'refused') && message.id === 2) {
          finish(new Error(`${message.code}: ${message.message}`));
        }
      }
    });
  });
}

function requestWorkspaceState(totalTimeoutMs = proofDaemonIdleTimeoutMs('workspace.state')) {
  return requestDaemon('workspace.state', {}, totalTimeoutMs);
}

async function waitForWorkspace(child, stderr, maximumAttempts = 50) {
  let lastFailure = '';
  const deadline = Date.now() + proofDaemonIdleTimeoutMs('workspace.state');
  for (let attempt = 0; attempt < maximumAttempts && Date.now() < deadline; attempt += 1) {
    if (!running(child)) throw new Error(`the bundled app exited before reopening: ${stderr()}`);
    try {
      return await requestWorkspaceState(Math.max(1, deadline - Date.now()));
    } catch (error) {
      lastFailure = error.message;
      await delay(Math.min(100, Math.max(0, deadline - Date.now())));
    }
  }
  throw new Error(`the embedded daemon did not reopen the remembered workspace: ${lastFailure}`);
}

async function waitForNoWorkspace(child, stderr) {
  let lastFailure = '';
  const deadline = Date.now() + proofDaemonIdleTimeoutMs('workspace.state');
  for (let attempt = 0; attempt < 50 && Date.now() < deadline; attempt += 1) {
    if (!running(child)) throw new Error(`the empty-state app exited early: ${stderr()}`);
    try {
      const state = await requestWorkspaceState(Math.max(1, deadline - Date.now()));
      lastFailure = `the daemon unexpectedly exposed ${state.root}`;
    } catch (error) {
      if (/^no-workspace-open:/.test(error.message)) {
        return { code: 'no-workspace-open', service_ready: true };
      }
      lastFailure = error.message;
    }
    await delay(Math.min(100, Math.max(0, deadline - Date.now())));
  }
  throw new Error(`the bundled app did not expose its truthful empty state: ${lastFailure}`);
}

async function waitForRememberedWorkspace(expectedWorkspace, child = null, stderr = () => '') {
  let lastFailure = '';
  const recentPath = join(appData, 'recent-workspace.json');
  for (let attempt = 0; attempt < 300; attempt += 1) {
    if (child && !running(child)) {
      throw new Error(`the bundled app exited before remembering import: ${stderr()}`);
    }
    try {
      const document = JSON.parse(await readFile(recentPath, 'utf8'));
      const workspace = document.workspaces?.find(
        (candidate) => candidate?.path === expectedWorkspace,
      );
      if (workspace) return { document, workspace };
      lastFailure = `the recent list did not contain ${expectedWorkspace}`;
    } catch (error) {
      lastFailure = error.message;
    }
    await delay(100);
  }
  throw new Error(`the visible import did not remember its exact managed workspace: ${lastFailure}`);
}

async function proveConcurrentProcessForwardsAttention(expected) {
  const contender = spawn(executable, [], {
    cwd: dirname(executable),
    env: proofEnvironment,
    stdio: ['ignore', 'ignore', 'pipe'],
  });
  let stderr = '';
  contender.stderr.setEncoding('utf8');
  contender.stderr.on('data', (chunk) => { stderr += chunk; });
  const exited = await Promise.race([
    new Promise((resolveExit) => contender.once('exit', () => resolveExit(true))),
    delay(3_000).then(() => false),
  ]);
  if (!exited) {
    await stop(contender);
    throw new Error('a concurrent bundled app process stayed alive instead of forwarding attention');
  }
  assert.equal(contender.exitCode, 0, 'a concurrent bundled app process did not forward attention');
  assert.match(stderr, /running window was asked to come forward/);
  const retained = await requestWorkspaceState();
  assert.equal(retained.root, expected.root, 'the concurrent process changed endpoint ownership');
  assert.equal(retained.digest, expected.digest, 'the concurrent process changed daemon identity');
}

async function proveCodexContextBridge(expected) {
  const bridge = spawn(executable, [
    '--mesh-mcp',
    '--endpoint', endpoint,
    '--expected-workspace-root', expected.root,
    '--expected-workspace-installation', expected.installation,
  ], {
    cwd: dirname(executable),
    env: proofEnvironment,
    stdio: ['pipe', 'pipe', 'pipe'],
  });
  let stdout = '';
  let stderr = '';
  bridge.stdout.setEncoding('utf8');
  bridge.stderr.setEncoding('utf8');
  bridge.stdout.on('data', (chunk) => { stdout += chunk; });
  bridge.stderr.on('data', (chunk) => { stderr += chunk; });
  for (const request of [
    { jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-06-18' } },
    { jsonrpc: '2.0', method: 'notifications/initialized' },
    { jsonrpc: '2.0', id: 2, method: 'tools/list', params: {} },
    { jsonrpc: '2.0', id: 3, method: 'tools/call', params: { name: 'mesh_workspace_state', arguments: {} } },
  ]) {
    bridge.stdin.write(`${JSON.stringify(request)}\n`);
  }
  bridge.stdin.end();
  let deadline;
  const deadlineReached = new Promise((resolveDeadline) => {
    // mesh_workspace_state is the bridge's only tool and may inspect the complete native tree.
    // Keep the process-level deadline beyond its bounded five-minute daemon reply window.
    deadline = setTimeout(() => resolveDeadline(null), 6 * 60_000);
  });
  const exited = await Promise.race([
    new Promise((resolveExit) => bridge.once('exit', (code, signal) => resolveExit({ code, signal }))),
    deadlineReached,
  ]);
  clearTimeout(deadline);
  if (exited === null) {
    await stop(bridge);
    throw new Error('the packaged Mesh context bridge did not finish after its input closed');
  }
  assert.deepEqual(exited, { code: 0, signal: null }, `the packaged Mesh context bridge failed: ${stderr.trim()}`);
  const replies = stdout.trim().split('\n').filter(Boolean).map((line) => JSON.parse(line));
  const initialized = replies.find((reply) => reply.id === 1)?.result;
  const listed = replies.find((reply) => reply.id === 2)?.result;
  const state = replies.find((reply) => reply.id === 3)?.result;
  assert.equal(initialized?.protocolVersion, '2025-06-18');
  assert.deepEqual(
    listed?.tools?.map((tool) => tool.name),
    ['mesh_workspace_state'],
    'the packaged agent bridge exposed an unexpected tool surface',
  );
  assert.equal(
    state?.isError,
    false,
    `the packaged agent bridge refused the selected workspace: ${JSON.stringify(state)}`,
  );
  assert.equal(state?.structuredContent?.root, expected.root, 'the agent bridge returned the wrong native root');
  assert.equal(
    state?.structuredContent?.installation,
    expected.installation,
    'the agent bridge returned a different workspace installation',
  );
  assert.match(
    state?.structuredContent?.mesh_desktop_build_revision,
    /^(?:development|[0-9a-f]{40})$/u,
    'the agent bridge omitted the exact desktop build identity',
  );
  assert.equal(
    state?.structuredContent?.mesh_desktop_build_exact,
    state.structuredContent.mesh_desktop_build_revision !== 'development',
    'the agent bridge build-exact verdict disagreed with its revision',
  );
  if (process.env.MESH_EXPECTED_BUILD_REVISION) {
    assert.equal(
      state.structuredContent.mesh_desktop_build_revision,
      process.env.MESH_EXPECTED_BUILD_REVISION,
      'the extracted agent bridge did not identify the archived source revision',
    );
  }
  return {
    protocol_version: initialized.protocolVersion,
    tool: listed.tools[0].name,
    root: state.structuredContent.root,
    installation: state.structuredContent.installation,
    desktop_build_revision: state.structuredContent.mesh_desktop_build_revision,
    desktop_build_exact: state.structuredContent.mesh_desktop_build_exact,
    read_only: listed.tools[0].annotations?.readOnlyHint === true,
  };
}

async function verifyStableFolder(expectedWorkspace) {
  assert.equal(
    (await lstat(stableFolder)).isSymbolicLink(),
    true,
    'the stable native folder is not a symbolic link',
  );
  assert.equal(
    await readlink(stableFolder),
    expectedWorkspace,
    'the stable native folder names the wrong version',
  );
  assert.equal(
    await realpath(stableFolder),
    expectedWorkspace,
    'the stable native folder resolves to the wrong version',
  );
}

async function launch(
  label,
  expectedWorkspace,
  takeScreenshot = false,
  whileRunning = null,
  rendererSurface = null,
) {
  const rendererSession = rendererSurface
    ? rendererProofSession(rendererSurface, expectedWorkspace)
    : null;
  const child = spawn(executable, [], {
    cwd: dirname(executable),
    env: rendererSession?.environment || proofEnvironment,
    stdio: ['ignore', 'ignore', 'pipe'],
  });
  let stderr = '';
  child.stderr.setEncoding('utf8');
  child.stderr.on('data', (chunk) => { stderr += chunk; });
  try {
    const statePromise = waitForWorkspace(child, () => stderr.trim());
    const [window, state] = await Promise.all([
      waitForWindow(child, () => stderr.trim()),
      statePromise,
    ]);
    if (rendererSurface === 'review' || rendererSurface === 'private-export') {
      const current = state.workspace_versions.at(-1)?.operation;
      const item = state.review_items.find((candidate) => candidate.subject_operation === current);
      assert.ok(item, `the reopened workspace had no review for its current version: ${JSON.stringify(state)}`);
      assert.equal(
        item.content_complete,
        item.bundle_changes_not_listed === 0,
        `the reopened review completeness contradicted its omitted-change count: ${JSON.stringify(item)}`,
      );
      if (seedRepository === null) {
        assert.equal(item.content_complete, true, `the reopened current review was incomplete: ${JSON.stringify(item)}`);
        assert.equal(item.bundle_changes_not_listed, 0, `the reopened current review omitted changes: ${JSON.stringify(item)}`);
      } else {
        assert.ok(
          item.bundle_changes_not_listed > 0,
          `the real-workspace review did not exercise its bounded incomplete state: ${JSON.stringify(item)}`,
        );
        assert.equal(
          item.projection_authorizes_approval,
          false,
          `the bounded real-workspace review carried approval authority: ${JSON.stringify(item)}`,
        );
      }
      assert.ok(item.bundle_changes.length > 0, `the reopened current review contained no changes: ${JSON.stringify(item)}`);
    }
    if (rendererSurface === 'versions') {
      assert.ok(
        state.workspace_versions.at(-1)?.operation,
        `the reopened workspace had no durable point for the version navigator: ${JSON.stringify(state)}`,
      );
    }
    const rendererProof = rendererSession
      ? await waitForRendererProof(child, () => stderr, rendererSession)
      : null;
    assert.equal(
      await realpath(state.root),
      expectedWorkspace,
      `${label} process reopened the wrong workspace`,
    );
    await verifyStableFolder(expectedWorkspace);
    if (rendererSurface === 'private-export') {
      assert.equal(
        rendererProof.outcome,
        seedRepository === null
          ? 'private-export-completed'
          : 'private-export-blocked-without-complete-review',
        'the renderer outcome must match the expected export journey before checking disk receipts',
      );
    }
    if (rendererSurface === 'review') {
      assert.equal(
        rendererProof.outcome,
        seedRepository === null
          ? 'saved-side-native-launches-completed'
          : 'incomplete-review-disclosed-without-authority',
        'the renderer outcome must match the expected review journey',
      );
    }
    const result = whileRunning ? await whileRunning(state) : null;
    let screenshotProof = null;
    if (takeScreenshot && screenshot) {
      const internalScreenshot = join(
        appData,
        'renderer-proof',
        `files-${rendererSession.nonce}.png`,
      );
      const screenshotBytes = await readFile(internalScreenshot);
      assert.ok(
        screenshotBytes.length >= 1_024 && screenshotBytes.length <= 16 * 1_024 * 1_024,
        'the app-owned Files screenshot was not bounded',
      );
      assert.deepEqual(
        screenshotBytes.subarray(0, 8),
        Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
        'the app-owned Files screenshot was not PNG',
      );
      assert.equal(
        screenshotBytes.subarray(12, 16).toString('ascii'),
        'IHDR',
        'the app-owned Files screenshot had no PNG dimensions',
      );
      const screenshotWidth = screenshotBytes.readUInt32BE(16);
      const screenshotHeight = screenshotBytes.readUInt32BE(20);
      assert.ok(
        screenshotWidth >= 320 && screenshotWidth <= 8_192
          && screenshotHeight >= 240 && screenshotHeight <= 8_192,
        'the app-owned Files screenshot dimensions were invalid',
      );
      const screenshotSha256 = createHash('sha256').update(screenshotBytes).digest('hex');
      const screenshotMarkers = stderr
        .split(/\r?\n/u)
        .filter((line) => line.startsWith(
          `mesh-renderer-proof-screenshot:${rendererSession.nonce}:`,
        ));
      assert.equal(
        screenshotMarkers.length,
        1,
        'the app-owned Files screenshot did not emit one nonce-bound native receipt',
      );
      assert.equal(
        screenshotMarkers[0],
        `mesh-renderer-proof-screenshot:${rendererSession.nonce}:${screenshotSha256}:${screenshotBytes.length}:${screenshotWidth}:${screenshotHeight}`,
        'the app-owned Files screenshot did not match its native descriptor-bound receipt',
      );
      await writeNewPrivateScreenshot(screenshot, screenshotBytes);
      screenshotProof = {
        schema: 'mesh-rendered-screenshot-proof/v1',
        path: screenshot,
        nonce: rendererSession.nonce,
        sha256: screenshotSha256,
        bytes: screenshotBytes.length,
        width: screenshotWidth,
        height: screenshotHeight,
      };
    }
    return { window, state, result, rendererProof, screenshotProof };
  } finally {
    await stop(child);
    await writeFile(join(scratch, `${label}-stderr.log`), stderr, { mode: 0o600 });
  }
}

async function launchEmpty() {
  const rendererSession = rendererProofSession('onboarding');
  const child = spawn(executable, [], {
    cwd: dirname(executable),
    env: rendererSession.environment,
    stdio: ['ignore', 'ignore', 'pipe'],
  });
  let stderr = '';
  child.stderr.setEncoding('utf8');
  child.stderr.on('data', (chunk) => { stderr += chunk; });
  try {
    const [window, state, rendererProof] = await Promise.all([
      waitForWindow(child, () => stderr.trim()),
      waitForNoWorkspace(child, () => stderr.trim()),
      waitForRendererProof(child, () => stderr, rendererSession),
    ]);
    // The renderer proof drives Preview and Create through the visible first-launch UI. Wait for
    // that exact transaction to finish instead of issuing a second daemon confirmation that could
    // race the user-equivalent action or fail to cover its accessible progress state.
    // A renderer success means the native import and navigation transaction has already returned.
    // Leave only a short bounded allowance for the next socket read instead of nesting thousands
    // of one-second daemon deadlines after visible completion.
    const importedWorkspace = await waitForWorkspace(child, () => stderr.trim(), 20);
    const privateStore = dirname(importedWorkspace.root);
    assert.ok(stderr.includes('mesh-renderer-proof-checkpoint:onboarding-empty-refused'),
      'onboarding did not prove the empty-folder refusal before its valid import');
    assert.deepEqual(await readdir(emptySource), [], 'empty-folder preview changed its source');
    assert.deepEqual((await readdir(versionStores)).sort(), [privateStore.split('/').at(-1)],
      'empty-folder preview left an unexpected managed workspace store');
    const rememberedImport = await waitForRememberedWorkspace(
      importedWorkspace.root,
      child,
      () => stderr.trim(),
    );
    const initialized = await requestDaemon('review.open-current', {
      opened_by: '08'.repeat(32),
    });
    assert.equal(initialized.reviews, 1, 'the scratch workspace did not retain its exact review');
    assert.equal(initialized.review_items.length, 1, 'the scratch review projection was unavailable');
    const initialReview = initialized.review_items[0];
    assert.equal(
      initialReview.content_complete,
      initialReview.bundle_changes_not_listed === 0,
      'the scratch review completeness contradicted its omitted-change count',
    );
    assert.ok(
      initialReview.bundle_changes.length > 0,
      'the scratch review contained no visible bounded changes',
    );
    if (seedRepository === null) {
      assert.equal(
        initialReview.bundle_changes_not_listed,
        0,
        'the small scratch review unexpectedly omitted changes',
      );
    } else {
      assert.ok(
        initialReview.bundle_changes_not_listed > 0,
        'the seeded large-workspace review did not exercise its bounded state',
      );
    }
    assert.equal(
      initialReview.projection_authorizes_approval,
      false,
      'the scratch review projection carried approval authority',
    );
    assert.equal(
      await realpath(initialized.root),
      await realpath(join(privateStore, 'Mesh Version - Working Folder')),
      'the explicit first-launch import initialized the wrong native workspace',
    );
    assert.equal(await readFile(join(source, 'notes.txt'), 'utf8'), 'first saved version\n');
    await assertExecutable(
      join(initialized.root, 'run.sh'),
      'the first managed native folder lost the imported executable bit',
    );
    return { window, state, initialized, privateStore, rememberedImport, rendererProof };
  } finally {
    await stop(child);
  }
}

let proofCompleted = false;
try {
  const empty = await launchEmpty();
  const canonicalWorkspace = await realpath(empty.initialized.root);
  const rememberedImport = empty.rememberedImport;
  assert.equal(
    rememberedImport.document.schema,
    'mesh-desktop-recent-workspaces/v9',
    'the visible import did not persist the current recent-workspace schema',
  );
  assert.equal(
    await realpath(rememberedImport.workspace.export_root),
    canonicalSource,
    'the visible import remembered the wrong original export folder',
  );
  assert.equal(
    await realpath(rememberedImport.workspace.project_root),
    canonicalSource,
    'the visible import remembered the wrong original project folder',
  );
  const agentHandoff = await launch(
    'agent-handoff',
    canonicalWorkspace,
    false,
    async () => {
      assert.equal(
        await readFile(join(canonicalWorkspace, AGENT_PROOF_RESULT_PATH), 'utf8'),
        AGENT_PROOF_RESULT,
        'the packaged agent handoff did not retain the verifier-owned result',
      );
      assert.deepEqual(
        await readFile(join(canonicalWorkspace, AGENT_PROOF_IMAGE_PATH)),
        AGENT_PROOF_IMAGE,
        'the packaged agent handoff did not retain the verifier-owned image',
      );
      const saved = await requestWorkspaceState();
      assert.ok(
        saved.file_histories.some((history) => history.path === AGENT_PROOF_RESULT_PATH),
        'the packaged agent handoff did not authenticate its result into private history',
      );
      assert.ok(
        saved.file_histories.some((history) => history.path === AGENT_PROOF_IMAGE_PATH),
        'the packaged agent handoff did not authenticate its image into private history',
      );
      const reviewed = await requestDaemon('review.open-current', {
        opened_by: '09'.repeat(32),
      });
      assert.ok(reviewed.reviews >= 1, 'the post-agent saved version could not be reviewed');
      return reviewed;
    },
    'agent-handoff',
  );
  // The native export flow intentionally separates missing-folder creation from file updates, and
  // renderer proof authority permits exactly one confirmation. Reproduce only the current native
  // workspace's ordinary directory topology in the isolated empty destination so that one visible
  // confirmation still proves the complete file phase. Git metadata is not workspace content.
  await mirrorOrdinaryDirectories(canonicalWorkspace, privateExport, {
    excludedRootNames: ['.git'],
  });
  // A seeded real repository intentionally produces a bounded initial review. Exercise the agent
  // lifecycle first so its two exact saved changes create a complete current review before the
  // private-copy journey asks for whole-workspace export authority.
  const exported = await launch(
    'private-export',
    canonicalWorkspace,
    false,
    waitForPrivateExportReceipt,
    'private-export',
  );
  if (seedRepository === null) {
    assert.equal(
      await readFile(join(privateExport, 'notes.txt'), 'utf8'),
      'first saved version\n',
      'the packaged private export changed the saved text bytes',
    );
    assert.equal(
      await readFile(join(privateExport, 'run.sh'), 'utf8'),
      "#!/bin/sh\nprintf 'native mesh\\n'\n",
      'the packaged private export changed the saved executable bytes',
    );
    await assertExecutable(
      join(privateExport, 'run.sh'),
      'the packaged private export lost the executable bit',
    );
    assert.deepEqual(
      await readFile(join(privateExport, FILES_PROOF_IMAGE_PATH)),
      AGENT_PROOF_IMAGE,
      'the packaged private export changed the nested saved image bytes',
    );
    assert.equal(
      await readFile(join(privateExport, AGENT_PROOF_RESULT_PATH), 'utf8'),
      AGENT_PROOF_RESULT,
      'the packaged private export omitted the current post-agent text result',
    );
    assert.deepEqual(
      await readFile(join(privateExport, AGENT_PROOF_IMAGE_PATH)),
      AGENT_PROOF_IMAGE,
      'the packaged private export omitted the current post-agent image result',
    );
  } else {
    for (const relative of ['notes.txt', 'run.sh', AGENT_PROOF_RESULT_PATH, AGENT_PROOF_IMAGE_PATH]) {
      await assert.rejects(
        lstat(join(privateExport, relative)),
        { code: 'ENOENT' },
        `the bounded incomplete review exported ${relative} without complete review authority`,
      );
    }
  }
  assert.equal(
    await readFile(join(source, 'notes.txt'), 'utf8'),
    'first saved version\n',
    'the private-export proof changed the unmanaged original text',
  );
  assert.equal(
    await readFile(join(source, 'run.sh'), 'utf8'),
    "#!/bin/sh\nprintf 'native mesh\\n'\n",
    'the private-export proof changed the unmanaged original executable bytes',
  );
  await assertExecutable(
    join(source, 'run.sh'),
    'the private-export proof changed the unmanaged original executable mode',
  );
  const versions = await launch(
    'workspace-versions',
    canonicalWorkspace,
    false,
    null,
    'versions',
  );
  const first = await launch('first', canonicalWorkspace, false, async (state) => {
    await proveConcurrentProcessForwardsAttention(state);
    const pinnedAgentContext = await proveCodexContextBridge(state);
    assert.equal(
      await realpath(pinnedAgentContext.root),
      canonicalWorkspace,
      'the packaged agent did not receive version one before Mesh switched versions',
    );
    const selected = state.workspace_versions.at(-1);
    assert.ok(selected?.operation, 'the imported workspace exposed no durable version to open');
    await mkdir(versionStores, { mode: 0o700, recursive: true });
    const forked = await requestDaemon('workspace.version.fork', {
      operation: selected.operation,
      destination: forkStore,
      expected_root: state.root,
      expected_digest: state.digest,
      expected_installation: state.installation,
    });
    return { forked, pinnedAgentContext };
  }, 'review');
  const { forked, pinnedAgentContext } = first.result;
  const forkWorkspace = await realpath(forked.workspace.root);
  assert.notEqual(
    forkWorkspace,
    canonicalWorkspace,
    'the saved version reused the current native folder',
  );
  await assertExecutable(
    join(forkWorkspace, 'run.sh'),
    'loading the saved workspace version lost its executable bit',
  );
  assert.equal(
    execFileSync(join(forkWorkspace, 'run.sh'), [], { encoding: 'utf8' }),
    'native mesh\n',
    'the executable restored into the selected native folder did not run',
  );
  // Finder and default applications are outside Mesh custody and may add ordinary metadata to a
  // folder they display. Exercise those native Files actions only after every proof that requires
  // the canonical workspace to remain byte-for-byte unchanged; the independent fork below is the
  // sole workspace used by the remaining native-edit journey.
  const files = await launch(
    'files',
    canonicalWorkspace,
    true,
    null,
    'files',
  );
  await writeFile(
    join(appData, 'recent-workspace.json'),
    JSON.stringify({
      schema: 'mesh-desktop-recent-workspaces/v7',
      workspaces: [
        {
          path: forkWorkspace,
          export_root: canonicalSource,
          project_root: canonicalSource,
          agent_handoff_installation: null,
          source_point_ordinal: null,
          original_update_version: null,
        },
        {
          path: canonicalWorkspace,
          export_root: canonicalSource,
          project_root: canonicalSource,
          agent_handoff_installation: null,
          source_point_ordinal: null,
          original_update_version: null,
        },
      ],
    }),
    { encoding: 'utf8', mode: 0o600 },
  );
  const restarted = await launch('restarted', forkWorkspace, false, async (state) => {
    const selectedAgentContext = await proveCodexContextBridge(state);
    assert.equal(
      await realpath(selectedAgentContext.root),
      forkWorkspace,
      'a newly started agent did not receive the selected version-two folder',
    );
    const nativeEdit = 'edited-through-stable-folder.txt';
    await writeFile(
      join(stableFolder, nativeEdit),
      'ordinary editor work through the stable path\n',
    );
    const afterNativeEdit = await requestWorkspaceState();
    assert.equal(
      afterNativeEdit.root,
      forkWorkspace,
      'editing through the stable path changed the selected workspace',
    );
    assert.equal(
      afterNativeEdit.native_inventory_complete,
      true,
      'the packaged daemon did not finish inspecting the native edit',
    );
    assert.deepEqual(
      afterNativeEdit.native_untracked_files,
      [nativeEdit],
      'an ordinary edit through the stable folder was not surfaced as unsaved native work',
    );
    await assert.rejects(
      lstat(join(canonicalWorkspace, nativeEdit)),
      { code: 'ENOENT' },
      'editing the selected stable folder changed the prior version',
    );
    await assert.rejects(
      lstat(join(source, nativeEdit)),
      { code: 'ENOENT' },
      'editing the selected stable folder changed the original folder',
    );
    await writeFile(
      join(pinnedAgentContext.root, 'agent-pinned.txt'),
      'agent stayed on version one\n',
    );
    assert.equal(
      await readFile(join(stableFolder, 'notes.txt'), 'utf8'),
      'first saved version\n',
      'editing the prior agent folder changed the selected stable workspace',
    );
    return { selectedAgentContext, nativeEdit };
  });
  assert.equal(
    restarted.state.root,
    forkWorkspace,
    'restart did not upgrade the remembered workspace to its canonical path',
  );
  assert.equal(
    restarted.state.digest,
    forked.workspace.digest,
    'restart changed the forked workspace digest',
  );
  assert.equal(
    restarted.state.records,
    forked.workspace.records,
    'restart changed the forked record count',
  );
  assert.equal(await readFile(join(source, 'notes.txt'), 'utf8'), 'first saved version\n');
  await assertExecutable(
    join(source, 'run.sh'),
    'the saved-version journey changed the unmanaged original executable',
  );
  assertSeedRepositoryUnchanged(seedRepository);
  process.stdout.write(`${JSON.stringify({
    ...restarted.window,
    // The window probe has its own v1 schema. Write the composed envelope identity after that
    // trusted subrecord so object spread cannot silently relabel the final renderer proof.
    schema: 'mesh-rendered-app-proof/v6',
    screenshot: files.screenshotProof,
    app,
    executable,
    processes: 8,
    first_launch: empty.state,
    explicit_first_open: {
      workspace: canonicalWorkspace,
      private_store: empty.privateStore,
      app_managed_storage: true,
      records: empty.initialized.records,
      digest: empty.initialized.digest,
      imported_from: canonicalSource,
    },
    seed_repository: seedRepository === null ? null : {
      path: seedRepository.path,
      revision: seedRepository.revision,
      tree: seedRepository.tree,
      tracked_files: seedRepository.trackedFiles,
      restored_from_committed_archive: true,
      original_unchanged: true,
    },
    endpoint_ownership: {
      concurrent_process_refused: true,
      attention_forwarded: true,
      first_process_retained_endpoint: true,
    },
    auto_reopen: {
      workspace: forkWorkspace,
      records: restarted.state.records,
      digest: restarted.state.digest,
      stable_across_restart: true,
    },
    native_versions: {
      stable_folder: stableFolder,
      first_workspace: canonicalWorkspace,
      selected_workspace: forkWorkspace,
      stable_link_retargeted: true,
      native_working_edit_detected: restarted.result.nativeEdit,
      agent_path_stayed_pinned: true,
      executable_version_ran_natively: true,
      original_preserved: true,
    },
    pinned_agent_context: pinnedAgentContext,
    selected_agent_context: restarted.result.selectedAgentContext,
    private_export: seedRepository === null
      ? {
          destination: privateExport,
          text_bytes_preserved: true,
          executable_bytes_preserved: true,
          executable_mode_preserved: true,
          original_unchanged: true,
          original_destination_refused: true,
        }
      : {
          destination: privateExport,
          blocked_by_incomplete_review: true,
          destination_files_written: false,
          original_unchanged: true,
        },
    component_interface_mounted: true,
    renderer_controls_driven: true,
    renderer: {
      schema: 'mesh-packaged-renderer-proof/v5',
      nonce_bound: true,
      onboarding: {
        surface: empty.rendererProof.surface,
        mounted: empty.rendererProof.mounted,
        visible: empty.rendererProof.visible,
        interaction: empty.rendererProof.interaction,
        outcome: empty.rendererProof.outcome,
      },
      files: {
        surface: files.rendererProof.surface,
        mounted: files.rendererProof.mounted,
        visible: files.rendererProof.visible,
        interaction: files.rendererProof.interaction,
        outcome: files.rendererProof.outcome,
      },
      review: {
        surface: first.rendererProof.surface,
        mounted: first.rendererProof.mounted,
        visible: first.rendererProof.visible,
        interaction: first.rendererProof.interaction,
        outcome: first.rendererProof.outcome,
      },
      versions: {
        surface: versions.rendererProof.surface,
        mounted: versions.rendererProof.mounted,
        visible: versions.rendererProof.visible,
        interaction: versions.rendererProof.interaction,
        outcome: versions.rendererProof.outcome,
      },
      private_export: {
        surface: exported.rendererProof.surface,
        mounted: exported.rendererProof.mounted,
        visible: exported.rendererProof.visible,
        interaction: exported.rendererProof.interaction,
        outcome: exported.rendererProof.outcome,
      },
      agent_handoff: {
        surface: agentHandoff.rendererProof.surface,
        mounted: agentHandoff.rendererProof.mounted,
        visible: agentHandoff.rendererProof.visible,
        interaction: agentHandoff.rendererProof.interaction,
        outcome: agentHandoff.rendererProof.outcome,
      },
    },
    ipc: { version: IPC_VERSION, surface_version: IPC_VERSION },
  })}\n`);
  proofCompleted = true;
} finally {
  if (proofCompleted) {
    await rm(scratch, {
      recursive: true,
      force: true,
      maxRetries: 10,
      retryDelay: 100,
    });
  } else {
    console.error(`Packaged proof failed; private diagnostic workspace retained at ${scratch}`);
  }
}
