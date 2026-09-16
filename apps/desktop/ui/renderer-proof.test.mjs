import assert from 'node:assert/strict';
import test from 'node:test';

import { rendererProofFailureCode, runRendererProof } from './renderer-proof.js';

const nonce = 'ab'.repeat(32);

class Node {
  constructor({ text = '', proof = null, pressed = null } = {}) {
    this.textContent = text;
    this.value = '';
    this.disabled = false;
    this.children = [];
    this.proof = proof;
    this.pressed = pressed;
    this.shadowRoot = null;
    this.dataset = {};
    this.classList = { contains: () => false };
    this.onClick = null;
    this.onDispatch = null;
    this.inert = false;
    this.attributes = new Map();
  }

  querySelector(selector) {
    return this.querySelectorAll(selector)[0] || null;
  }

  querySelectorAll(selector) {
    const all = [this, ...this.children.flatMap((child) => child.querySelectorAll('*'))];
    if (selector === '*') return all;
    const proof = selector.match(/^\[data-mesh-proof="(.+)"\]$/u)?.[1];
    if (proof) return all.filter((node) => node.proof === proof);
    if (selector === 'button') return all.filter((node) => node.isButton);
    return [];
  }

  getClientRects() {
    return [{}];
  }

  getAttribute(name) {
    if (name === 'aria-pressed' && this.pressed !== null) return String(this.pressed);
    if (name === 'data-mesh-generation' && this.generation !== undefined) return String(this.generation);
    return this.attributes.get(name) ?? null;
  }

  setAttribute(name, value) {
    this.attributes.set(name, String(value));
  }

  dispatchEvent(event) {
    this.onDispatch?.(event);
  }

  focus() {}

  click() {
    this.onClick?.();
  }
}

function button(text, onClick = null) {
  const node = new Node({ text });
  node.isButton = true;
  node.onClick = onClick;
  return node;
}

function productionNoticeShell(notice, pageLabel) {
  const shell = new Node();
  shell.setAttribute('data-mesh-react-shell-active', 'true');
  shell.shadowRoot = new Node();
  shell.shadowRoot.children = [notice, button(pageLabel)];
  return shell;
}

function documentWith(host) {
  return {
    getElementById: (id) => id === 'mesh-app-next' ? null : host,
    defaultView: { Event: globalThis.Event, confirm: () => false },
  };
}

function invokeFor(configuration, reports, checkpoints = []) {
  return async (command, parameters = {}) => {
    if (command === 'renderer_proof_configuration') return JSON.stringify(configuration);
    if (command === 'renderer_proof_report') {
      reports.push(JSON.parse(parameters.report));
      return null;
    }
    if (command === 'renderer_proof_checkpoint') {
      checkpoints.push(parameters.code);
      return parameters.code;
    }
    throw new Error(`unexpected command ${command}`);
  };
}

const visible = () => ({ display: 'block', visibility: 'visible' });

test('onboarding proof requires a real preview click and verified React projection', async () => {
  const reports = [];
  const host = new Node();
  const shadow = new Node();
  host.shadowRoot = shadow;
  const select = new Node({ proof: 'import-select' });
  const input = new Node({ proof: 'import-path' });
  const preview = button('Preview folder', () => {
    assert.equal(
      shadow.activeElement,
      preview,
      'the proof must model the focused user action that owns review-heading focus',
    );
    assert.equal(input.value, '/tmp/proof-source');
    const heading = new Node({ proof: 'import-review-heading' });
    shadow.children = [new Node({ proof: 'import-verified-preview' }), heading];
    shadow.activeElement = heading;
  });
  preview.focus = () => { shadow.activeElement = preview; };
  preview.proof = 'import-preview';
  shadow.children = [select, input, preview];

  await runRendererProof({
    document: documentWith(host),
    invoke: invokeFor({
      schema: 'mesh-renderer-proof-config/v2', nonce, surface: 'onboarding',
      source: '/tmp/proof-source', destination: null,
    }, reports),
    getComputedStyle: visible,
    delay: async () => {},
  });

  assert.deepEqual(reports, [{
    schema: 'mesh-renderer-proof/v1', nonce, surface: 'onboarding', mounted: true,
    visible: true, interaction: 'preview-path', outcome: 'verified-preview',
  }]);
});

test('packaged proof reaches a separated React page after the shell commit handshake', async () => {
  const reports = [];
  const contentHost = new Node();
  const contentShadow = new Node();
  contentHost.shadowRoot = contentShadow;
  const select = new Node({ proof: 'import-select' });
  const input = new Node({ proof: 'import-path' });
  const preview = button('Preview folder', () => {
    const heading = new Node({ proof: 'import-review-heading' });
    contentShadow.children = [new Node({ proof: 'import-verified-preview' }), heading];
    contentShadow.activeElement = heading;
  });
  preview.focus = () => { contentShadow.activeElement = preview; };
  preview.proof = 'import-preview';
  contentShadow.children = [select, input, preview];
  const appHost = new Node();
  appHost.setAttribute('data-mesh-react-shell-active', 'true');
  const appShadow = new Node();
  appHost.shadowRoot = appShadow;
  let pageOpen = false;
  const importPage = button('Import', () => { pageOpen = true; });
  appShadow.children = [importPage];
  const document = {
    getElementById: (id) => id === 'mesh-app-next' ? appHost : contentHost,
    defaultView: { Event: globalThis.Event, confirm: () => false },
  };
  await runRendererProof({
    document,
    invoke: invokeFor({
      schema: 'mesh-renderer-proof-config/v2', nonce, surface: 'onboarding',
      source: '/tmp/proof-source', destination: null,
    }, reports),
    getComputedStyle: (element) => ({
      display: element === contentHost && !pageOpen ? 'none' : 'block',
      visibility: 'visible',
    }),
    delay: async () => {},
  });
  assert.equal(pageOpen, true);
  assert.equal(reports[0].outcome, 'verified-preview');
});

test('packaged proof waits for the React shell commit without timer polling', async () => {
  const reports = [];
  const contentHost = new Node();
  const contentShadow = new Node();
  contentHost.shadowRoot = contentShadow;
  const input = new Node({ proof: 'import-path' });
  const preview = button('Preview folder', () => {
    const heading = new Node({ proof: 'import-review-heading' });
    contentShadow.children = [new Node({ proof: 'import-verified-preview' }), heading];
    contentShadow.activeElement = heading;
  });
  preview.focus = () => { contentShadow.activeElement = preview; };
  preview.proof = 'import-preview';
  contentShadow.children = [new Node({ proof: 'import-select' }), input, preview];
  const appHost = new Node();
  appHost.shadowRoot = new Node();
  let pageOpen = false;
  appHost.shadowRoot.children = [button('Import', () => { pageOpen = true; })];
  const events = new EventTarget();
  const document = {
    getElementById: (id) => id === 'mesh-app-next' ? appHost : contentHost,
    addEventListener: events.addEventListener.bind(events),
    removeEventListener: events.removeEventListener.bind(events),
    dispatchEvent: events.dispatchEvent.bind(events),
    defaultView: { Event: globalThis.Event, confirm: () => false },
  };
  const proving = runRendererProof({
    document,
    invoke: invokeFor({
      schema: 'mesh-renderer-proof-config/v2', nonce, surface: 'onboarding',
      source: '/tmp/proof-source', destination: null,
    }, reports),
    getComputedStyle: (element) => ({
      display: element === contentHost && !pageOpen ? 'none' : 'block',
      visibility: 'visible',
    }),
    delay: async () => {},
  });
  await Promise.resolve();
  appHost.setAttribute('data-mesh-react-shell-active', 'true');
  document.dispatchEvent(new Event('mesh:react-shell-committed'));
  await proving;
  assert.equal(pageOpen, true);
  assert.equal(reports[0].outcome, 'verified-preview');
});

test('a bounded cold WKWebView start may take longer than the former ten-second proof window', async () => {
  const reports = [];
  const host = new Node();
  const shadow = new Node();
  host.shadowRoot = shadow;
  const select = new Node({ proof: 'import-select' });
  const input = new Node({ proof: 'import-path' });
  const preview = button('Preview folder', () => {
    const heading = new Node({ proof: 'import-review-heading' });
    shadow.children = [new Node({ proof: 'import-verified-preview' }), heading];
    shadow.activeElement = heading;
  });
  preview.proof = 'import-preview';
  shadow.children = [select, input, preview];
  let delays = 0;

  await runRendererProof({
    document: documentWith(host),
    invoke: invokeFor({
      schema: 'mesh-renderer-proof-config/v2', nonce, surface: 'onboarding',
      source: '/tmp/proof-source', destination: null,
    }, reports),
    getComputedStyle: (element) => ({
      display: element === host && delays < 250 ? 'none' : 'block',
      visibility: 'visible',
    }),
    delay: async () => { delays += 1; },
  });

  assert.ok(delays >= 250);
  assert.equal(reports[0].outcome, 'verified-preview');
});

test('review proof drives Content changes and Inline before reporting', async () => {
  const reports = [];
  const checkpoints = [];
  const host = new Node();
  const shadow = new Node();
  host.shadowRoot = shadow;
  const mounted = new Node({ proof: 'review-mounted' });
  const comparison = new Node({ proof: 'comparison-view' });
  const interactions = [];
  const visual = button('Visual', () => {
    interactions.push('Visual');
    visual.pressed = true;
    content.pressed = false;
  });
  visual.pressed = false;
  const content = button('Content changes', () => {
    interactions.push('Content changes');
    visual.pressed = false;
    content.pressed = true;
    const layout = new Node({ proof: 'content-diff-layout' });
    const inline = button('Inline', () => {
      interactions.push('Inline');
      inline.pressed = true;
    });
    inline.pressed = false;
    layout.children = [inline];
    shadow.children.push(layout);
  });
  content.pressed = true;
  comparison.children = [visual, content];
  shadow.children = [mounted, comparison];

  await runRendererProof({
    document: documentWith(host),
    invoke: invokeFor({
      schema: 'mesh-renderer-proof-config/v2', nonce, surface: 'review',
      source: null, destination: null,
    }, reports, checkpoints),
    getComputedStyle: visible,
    delay: async () => {},
  });

  assert.deepEqual(reports[0], {
    schema: 'mesh-renderer-proof/v1', nonce, surface: 'review', mounted: true,
    visible: true, interaction: 'content-inline', outcome: 'content-inline-selected',
  });
  assert.deepEqual(interactions, ['Visual', 'Content changes', 'Inline']);
  assert.deepEqual(checkpoints, [
    'review-mounted',
    'review-visual',
    'review-content',
    'review-inline',
  ]);
});

test('review proof classifies a failed Visual transition as review interaction, not configuration', () => {
  assert.equal(
    rendererProofFailureCode(new Error('the packaged React review workbench did not leave content changes')),
    'review-content',
  );
});

test('workspace versions proof selects a saved point and waits for an actionable verified preview', async () => {
  const reports = [];
  const host = new Node();
  const shadow = new Node();
  host.shadowRoot = shadow;
  const mounted = new Node({ proof: 'workspace-versions' });
  const choice = button('Current saved workspace', () => {
    const preview = new Node({ proof: 'workspace-version-preview-ready' });
    const open = button('Open in working folder');
    open.proof = 'workspace-version-open';
    open.disabled = false;
    shadow.children = [mounted, choice, preview, open];
  });
  choice.proof = 'workspace-version-choice';
  shadow.children = [mounted, choice];

  await runRendererProof({
    document: documentWith(host),
    invoke: invokeFor({
      schema: 'mesh-renderer-proof-config/v2', nonce, surface: 'versions',
      source: null, destination: null,
    }, reports),
    getComputedStyle: visible,
    delay: async () => {},
  });

  assert.deepEqual(reports, [{
    schema: 'mesh-renderer-proof/v1', nonce, surface: 'versions', mounted: true,
    visible: true, interaction: 'select-saved-point', outcome: 'verified-preview-ready',
  }]);
});

test('workspace versions proof retries a generation-stale click before selection begins', async () => {
  const reports = [];
  const host = new Node();
  const shadow = new Node();
  host.shadowRoot = shadow;
  const mounted = new Node({ proof: 'workspace-versions' });
  let clicks = 0;
  const choice = button('Current saved workspace', () => {
    clicks += 1;
    if (clicks === 1) return;
    choice.getAttribute = (name) => name === 'aria-checked' ? 'true' : null;
    const preview = new Node({ proof: 'workspace-version-preview-ready' });
    const open = button('Open in working folder');
    open.proof = 'workspace-version-open';
    shadow.children = [mounted, choice, preview, open];
  });
  choice.proof = 'workspace-version-choice';
  shadow.children = [mounted, choice];

  await runRendererProof({
    document: documentWith(host),
    invoke: invokeFor({
      schema: 'mesh-renderer-proof-config/v2', nonce, surface: 'versions',
      source: null, destination: null,
    }, reports),
    getComputedStyle: visible,
    delay: async () => {},
    now: (() => {
      let time = 0;
      return () => { time += 1_000; return time; };
    })(),
  });

  assert.equal(clicks, 2);
  assert.equal(reports[0].outcome, 'verified-preview-ready');
});

test('workspace versions proof waits for coordinator selection authority before clicking', async () => {
  const reports = [];
  const host = new Node();
  const shadow = new Node();
  host.shadowRoot = shadow;
  const mounted = new Node({ proof: 'workspace-versions' });
  let unavailableClicks = 0;
  const unavailableChoice = button('Current saved workspace', () => { unavailableClicks += 1; });
  unavailableChoice.proof = 'workspace-version-choice';
  unavailableChoice.disabled = true;
  shadow.children = [mounted, unavailableChoice];
  let delays = 0;

  await runRendererProof({
    document: documentWith(host),
    invoke: invokeFor({
      schema: 'mesh-renderer-proof-config/v2', nonce, surface: 'versions',
      source: null, destination: null,
    }, reports),
    getComputedStyle: visible,
    delay: async () => {
      delays += 1;
      if (delays !== 2) return;
      const choice = button('Current saved workspace', () => {
        const preview = new Node({ proof: 'workspace-version-preview-ready' });
        const open = button('Open in working folder');
        open.proof = 'workspace-version-open';
        shadow.children = [mounted, choice, preview, open];
      });
      choice.proof = 'workspace-version-choice';
      shadow.children = [mounted, choice];
    },
  });

  assert.equal(unavailableClicks, 0, 'the proof clicked a radio without coordinator authority');
  assert.equal(reports[0].outcome, 'verified-preview-ready');
});

test('workspace versions proof retries a selected point after verification churn reports an error', async () => {
  const reports = [];
  const host = new Node();
  const shadow = new Node();
  host.shadowRoot = shadow;
  const mounted = new Node({ proof: 'workspace-versions' });
  let clicks = 0;
  const choice = button('Current saved workspace', () => {
    clicks += 1;
    choice.getAttribute = (name) => name === 'aria-checked' ? 'true' : null;
    if (clicks === 1) {
      shadow.children = [mounted, choice, new Node({ proof: 'workspace-version-preview-error' })];
      return;
    }
    const preview = new Node({ proof: 'workspace-version-preview-ready' });
    const open = button('Open in working folder');
    open.proof = 'workspace-version-open';
    shadow.children = [mounted, choice, preview, open];
  });
  choice.proof = 'workspace-version-choice';
  shadow.children = [mounted, choice];

  await runRendererProof({
    document: documentWith(host),
    invoke: invokeFor({
      schema: 'mesh-renderer-proof-config/v2', nonce, surface: 'versions',
      source: null, destination: null,
    }, reports),
    getComputedStyle: visible,
    delay: async () => {},
    now: (() => {
      let time = 0;
      return () => { time += 1_000; return time; };
    })(),
  });

  assert.equal(clicks, 2, 'the proof left a selected point in a failed verification state');
  assert.equal(reports[0].outcome, 'verified-preview-ready');
});

test('workspace versions proof refuses a preview whose native open action is still disabled', async () => {
  const reports = [];
  const host = new Node();
  const shadow = new Node();
  host.shadowRoot = shadow;
  const mounted = new Node({ proof: 'workspace-versions' });
  const choice = button('Current saved workspace', () => {
    const preview = new Node({ proof: 'workspace-version-preview-ready' });
    const open = button('Open in working folder');
    open.proof = 'workspace-version-open';
    open.disabled = true;
    shadow.children = [mounted, choice, preview, open];
  });
  choice.proof = 'workspace-version-choice';
  shadow.children = [mounted, choice];

  await assert.rejects(runRendererProof({
    document: documentWith(host),
    invoke: invokeFor({
      schema: 'mesh-renderer-proof-config/v2', nonce, surface: 'versions',
      source: null, destination: null,
    }, reports),
    getComputedStyle: visible,
    delay: async () => {},
  }), /did not reach an actionable verified preview/);
  assert.deepEqual(reports, []);
});

test('private export proof refuses the original and completes the confirmed private copy', async () => {
  const reports = [];
  const checkpoints = [];
  const host = new Node();
  const shadow = new Node();
  host.shadowRoot = shadow;
  const mounted = new Node({ proof: 'review-mounted' });
  const choose = button('Choose export folder');
  shadow.children = [mounted, choose];

  const destinationHost = new Node();
  const destinationShadow = new Node();
  destinationHost.shadowRoot = destinationShadow;

  const source = '/tmp/mesh-app-proof/source';
  const destination = '/tmp/mesh-app-proof/home/private-export';
  const destinationMounted = new Node({ proof: 'workspace-destination' });
  const notice = new Node({
    text: 'That is the original project folder. Choose a different ordinary folder.',
    proof: 'production-notice',
  });
  const productionHost = productionNoticeShell(notice, 'Review');
  let generations = 0;
  let blockedTransitionWaits = 0;
  const installDestinationGeneration = (value = '', planText = null) => {
    generations += 1;
    destinationHost.inert = true;
    destinationHost.setAttribute('aria-busy', 'true');
    destinationMounted.generation = generations;
    const selected = new Node({ text: value, proof: 'destination-selected' });
    const chooseDestination = button('Choose folder', () => {
      installDestinationGeneration(destination);
    });
    chooseDestination.proof = 'destination-choose';
    const preview = button('Preview saved workspace', () => {
      installDestinationGeneration(selected.textContent, `Destination folder: ${selected.textContent}`);
    });
    preview.proof = 'destination-preview-all';
    preview.disabled = !value || value === source;
    const confirm = button('Update changed files', () => {
      notice.textContent = `Saved changes, moves, and deletions are applied to ${destination}.`;
    });
    confirm.proof = 'destination-confirm-all';
    confirm.disabled = planText === null;
    const hint = new Node({
      text: preview.disabled
      ? 'Choose another destination for a private export.'
        : 'Preview the saved workspace.',
      proof: 'destination-hint',
    });
    destinationShadow.children = [destinationMounted, selected, chooseDestination, preview, confirm, hint];
    if (planText !== null) {
      destinationShadow.children.push(new Node({ text: planText, proof: 'destination-plan' }));
    }
  };
  installDestinationGeneration();
  choose.onClick = () => installDestinationGeneration();
  const elements = new Map([
    ['mesh-app-next', productionHost],
    ['review-workbench-next', host],
    ['workspace-destination-next', destinationHost],
  ]);
  const document = {
    getElementById: (id) => elements.get(id) || null,
    defaultView: { Event: globalThis.Event, confirm: () => false },
  };

  await runRendererProof({
    document,
    invoke: invokeFor({
      schema: 'mesh-renderer-proof-config/v2', nonce, surface: 'private-export',
      source, destination,
    }, reports, checkpoints),
    getComputedStyle: visible,
    delay: async () => {
      if (destinationHost.inert) blockedTransitionWaits += 1;
      destinationHost.inert = false;
      destinationHost.setAttribute('aria-busy', 'false');
    },
  });

  assert.equal(document.defaultView.confirm(), false, 'the proof must restore normal confirmation');
  assert.ok(blockedTransitionWaits >= 1, 'the proof clicked a destination control before its host became actionable');
  assert.ok(generations >= 3, 'the proof did not cross replacement React generations');
  assert.deepEqual(checkpoints, [
    'private-export-start',
    'private-export-mounted',
    'private-export-clicked',
    'private-export-destination-mounted',
    'private-export-destination-visible',
    'private-export-original-refused',
    'private-export-projection-target-empty',
    'private-export-react-generation-stale',
    'private-export-target-ready',
    'private-export-target-accepted',
    'private-export-preview-enabled',
    'private-export-preview-ready',
  ]);
  assert.deepEqual(reports, [{
    schema: 'mesh-renderer-proof/v1', nonce, surface: 'private-export', mounted: true,
    visible: true, interaction: 'refuse-original-then-confirm-private',
    outcome: 'private-export-completed',
  }]);
});

test('agent handoff proof drives Start, confirmed Finish, and the saved rescan result', async () => {
  const reports = [];
  const checkpoints = [];
  const lifecycle = [];
  const currentHost = new Node();
  const currentShadow = new Node();
  currentHost.shadowRoot = currentShadow;
  const confirmationHost = new Node();
  const confirmationShadow = new Node();
  confirmationHost.shadowRoot = confirmationShadow;
  const notice = new Node({ proof: 'production-notice' });
  const productionHost = productionNoticeShell(notice, 'Current');
  const mounted = new Node({ proof: 'current-mounted' });
  const installAvailable = () => {
    const start = button('Start Codex', () => {
      lifecycle.push('start-react-intent');
      installAssigned();
    });
    start.proof = 'current-start-codex';
    currentShadow.children = [
      mounted,
      new Node({ proof: 'current-agent-available' }),
      start,
    ];
  };
  const installAssigned = () => {
    const finish = button('Finish agent handoff', () => {
      lifecycle.push('finish-react-intent');
      const accept = button('Finish agent handoff', () => {
        lifecycle.push('confirmation-accepted');
        confirmationShadow.children = [];
        notice.textContent = 'Agent folder released after a complete native inspection. 1 changed file was authenticated and saved privately.';
        notice.setAttribute('data-mesh-agent-proof', 'agent-handoff-rescanned');
        lifecycle.push('authenticated-rescan-notice-committed');
        installAvailable();
      });
      accept.proof = 'confirmation-accept';
      confirmationShadow.children = [
        new Node({ proof: 'confirmation-backdrop' }),
        accept,
      ];
    });
    finish.proof = 'current-finish-agent';
    currentShadow.children = [
      mounted,
      new Node({ proof: 'current-agent-assigned' }),
      finish,
    ];
  };
  installAvailable();
  const elements = new Map([
    ['mesh-app-next', productionHost],
    ['workspace-current-next', currentHost],
    ['confirmation-dialog-next', confirmationHost],
  ]);

  await runRendererProof({
    document: {
      getElementById: (id) => {
        assert.equal(
          ['open-codex', 'open-agent-terminal', 'copy-agent-path', 'start-isolated-agent', 'finish-agent'].includes(id),
          false,
          `the packaged proof reached the retained hidden lifecycle proxy ${id}`,
        );
        return elements.get(id) || null;
      },
      defaultView: { Event: globalThis.Event, confirm: () => false },
    },
    invoke: invokeFor({
      schema: 'mesh-renderer-proof-config/v2', nonce, surface: 'agent-handoff',
      source: '/tmp/mesh-app-proof/workspace-versions/proof.mesh/mounts', destination: null,
    }, reports, checkpoints),
    getComputedStyle: visible,
    delay: async () => {},
  });

  assert.deepEqual(reports, [{
    schema: 'mesh-renderer-proof/v1', nonce, surface: 'agent-handoff', mounted: true,
    visible: true, interaction: 'start-finish-rescan', outcome: 'agent-handoff-completed',
  }]);
  assert.deepEqual(checkpoints, [
    'agent-handoff-start-clicked',
    'agent-handoff-assigned',
    'agent-handoff-finish-clicked',
    'agent-handoff-confirmed',
    'agent-handoff-complete',
  ]);
  assert.deepEqual(lifecycle, [
    'start-react-intent',
    'finish-react-intent',
    'confirmation-accepted',
    'authenticated-rescan-notice-committed',
  ]);
});

test('agent handoff proof fails closed when React does not commit the Finish confirmation', async () => {
  const reports = [];
  const checkpoints = [];
  const currentHost = new Node();
  const currentShadow = new Node();
  currentHost.shadowRoot = currentShadow;
  const mounted = new Node({ proof: 'current-mounted' });
  const start = button('Start Codex', () => {
    const finish = button('Finish agent handoff', () => {});
    finish.proof = 'current-finish-agent';
    currentShadow.children = [mounted, new Node({ proof: 'current-agent-assigned' }), finish];
  });
  start.proof = 'current-start-codex';
  currentShadow.children = [mounted, new Node({ proof: 'current-agent-available' }), start];
  const elements = new Map([
    ['mesh-app-next', productionNoticeShell(new Node({ proof: 'production-notice' }), 'Current')],
    ['workspace-current-next', currentHost],
    ['confirmation-dialog-next', new Node()],
  ]);

  await assert.rejects(runRendererProof({
    document: {
      getElementById: (id) => elements.get(id) || null,
      defaultView: { Event: globalThis.Event, confirm: () => false },
    },
    invoke: invokeFor({
      schema: 'mesh-renderer-proof-config/v2', nonce, surface: 'agent-handoff',
      source: '/tmp/mesh-app-proof/workspace-versions/proof.mesh/mounts', destination: null,
    }, reports, checkpoints),
    getComputedStyle: visible,
    delay: async () => {},
  }), /did not render its Finish confirmation/);

  assert.deepEqual(checkpoints, [
    'agent-handoff-start-clicked',
    'agent-handoff-assigned',
    'agent-handoff-finish-clicked',
  ]);
  assert.deepEqual(reports, []);
});

test('disabled or incomplete proof sessions never report success', async () => {
  const reports = [];
  await assert.rejects(
    runRendererProof({
      document: documentWith(new Node()),
      invoke: async () => { throw new Error('packaged renderer proof is not enabled'); },
      getComputedStyle: visible,
      delay: async () => {},
    }),
    /not enabled/,
  );
  assert.deepEqual(reports, []);
});
