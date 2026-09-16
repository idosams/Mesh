const CONFIGURATION_KEYS = ['schema', 'nonce', 'surface', 'source', 'destination'];
const NONCE = /^[0-9a-f]{64}$/u;

function exactConfiguration(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)
    || JSON.stringify(Object.keys(value)) !== JSON.stringify(CONFIGURATION_KEYS)
    || value.schema !== 'mesh-renderer-proof-config/v2'
    || typeof value.nonce !== 'string'
    || !NONCE.test(value.nonce)
    || !['onboarding', 'review', 'versions', 'private-export', 'agent-handoff'].includes(value.surface)
    || (value.surface === 'onboarding'
      && (typeof value.source !== 'string' || !value.source || value.destination !== null))
    || ((value.surface === 'review' || value.surface === 'versions')
      && (value.source !== null || value.destination !== null))
    || (value.surface === 'agent-handoff'
      && (typeof value.source !== 'string' || !value.source || value.destination !== null))
    || (value.surface === 'private-export'
      && (typeof value.source !== 'string'
        || !value.source
        || typeof value.destination !== 'string'
        || !value.destination
        || value.source === value.destination))) {
    throw new Error('packaged renderer proof configuration was invalid');
  }
  return value;
}

function visible(element, getComputedStyle) {
  if (!element || element.classList?.contains('hidden') || element.getClientRects().length === 0) {
    return false;
  }
  const style = getComputedStyle(element);
  return style.display !== 'none' && style.visibility !== 'hidden';
}

function interactableHost(element) {
  return Boolean(element)
    && element.inert !== true
    && element.getAttribute?.('aria-busy') !== 'true';
}

async function waitFor(read, delay, message) {
  // A clean archive starts the freshly linked WKWebView immediately after the native build and
  // full desktop suite. WebKit may clamp a background window's 50 ms timers toward one second, so
  // bind the wait to elapsed wall time as well as attempts instead of silently stretching it to
  // several minutes.
  const deadline = Date.now() + 20_000;
  for (let attempt = 0; attempt < 400 && Date.now() < deadline; attempt += 1) {
    const value = read();
    if (value) return value;
    await delay(50);
  }
  throw new Error(message);
}

function proofElement(root, name) {
  return root?.querySelector(`[data-mesh-proof="${name}"]`) || null;
}

function buttonNamed(root, label) {
  return [...(root?.querySelectorAll('button') || [])]
    .find((button) => button.textContent?.trim() === label) || null;
}

async function openReactPage(page, dependencies) {
  const { document } = dependencies;
  const shell = document.getElementById('mesh-app-next');
  if (!shell) return;
  const labels = {
    workspaces: 'Workspaces', import: 'Import',
    current: 'Current', files: 'Files', changes: 'Changes', review: 'Review',
    versions: 'Versions', update: 'Update destination', restore: 'Restore',
  };
  const readControl = () => {
    if (shell.getAttribute?.('data-mesh-react-shell-active') !== 'true') return null;
    return labels[page] ? buttonNamed(shell.shadowRoot, labels[page]) : null;
  };
  let control = readControl();
  while (!control) {
    if (typeof document.addEventListener !== 'function') {
      throw new Error(`the packaged React ${page} page was not reachable`);
    }
    control = await new Promise((resolve) => {
      const events = ['mesh:react-shell-committed', 'mesh:workspace-chrome-mounted'];
      const cleanup = () => events.forEach((name) => document.removeEventListener(name, accept));
      const accept = () => {
        const candidate = readControl();
        if (!candidate) return;
        cleanup();
        resolve(candidate);
      };
      events.forEach((name) => document.addEventListener(name, accept));
      accept();
    });
  }
  control.click();
}

function installInputValue(input, value, document) {
  const descriptor = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(input), 'value');
  if (descriptor?.set) descriptor.set.call(input, value);
  else input.value = value;
  const InputEvent = document.defaultView?.Event || globalThis.Event;
  input.dispatchEvent(new InputEvent('input', { bubbles: true }));
  input.dispatchEvent(new InputEvent('change', { bubbles: true }));
}

async function proveOnboarding(configuration, dependencies) {
  const { document, getComputedStyle, delay } = dependencies;
  await openReactPage('import', dependencies);
  const host = document.getElementById('import-workbench-next');
  const controls = await waitFor(() => {
    const shadow = host?.shadowRoot;
    const mounted = proofElement(shadow, 'import-select');
    const input = proofElement(shadow, 'import-path');
    const preview = proofElement(shadow, 'import-preview');
    return visible(host, getComputedStyle)
      && visible(mounted, getComputedStyle)
      && visible(input, getComputedStyle)
      && visible(preview, getComputedStyle)
      ? { shadow, input, preview }
      : null;
  }, delay, 'the packaged React onboarding did not mount visibly');
  await waitFor(
    () => {
      // Startup may refresh the same select-phase projection while the native workspace check is
      // finishing. Reapply the user-equivalent edit if that committed render replaced its state.
      if (controls.input.value !== configuration.source) {
        installInputValue(controls.input, configuration.source, document);
      }
      return !controls.preview.disabled && controls.input.value === configuration.source;
    },
    delay,
    'the packaged React onboarding did not accept the proof path',
  );
  // The import workbench transfers focus to the verified review only when the initiating control
  // still owns it. Model that real keyboard/user action before dispatching the synthetic click.
  controls.preview.focus({ preventScroll: true });
  controls.preview.click();
  await waitFor(
    () => {
      const verified = proofElement(controls.shadow, 'import-verified-preview');
      const heading = proofElement(controls.shadow, 'import-review-heading');
      return visible(host, getComputedStyle)
        && visible(verified, getComputedStyle)
        && visible(heading, getComputedStyle)
        ? verified
        : null;
    },
    delay,
    'the packaged React onboarding did not mount the verified preview',
  );
  await waitFor(
    () => {
      const heading = proofElement(controls.shadow, 'import-review-heading');
      return visible(heading, getComputedStyle) && controls.shadow.activeElement === heading
        ? heading
        : null;
    },
    delay,
    'the packaged React onboarding did not render the verified preview with keyboard focus',
  );
  return {
    schema: 'mesh-renderer-proof/v1',
    nonce: configuration.nonce,
    surface: 'onboarding',
    mounted: true,
    visible: true,
    interaction: 'preview-path',
    outcome: 'verified-preview',
  };
}

async function proveReview(configuration, dependencies) {
  const { document, invoke, getComputedStyle, delay } = dependencies;
  await openReactPage('review', dependencies);
  const host = document.getElementById('review-workbench-next');
  const review = await waitFor(() => {
    const shadow = host?.shadowRoot;
    const mounted = proofElement(shadow, 'review-mounted');
    const comparison = proofElement(shadow, 'comparison-view');
    const visual = buttonNamed(comparison, 'Visual');
    const content = buttonNamed(comparison, 'Content changes');
    return visible(host, getComputedStyle)
      && visible(mounted, getComputedStyle)
      && visible(comparison, getComputedStyle)
      && visible(visual, getComputedStyle)
      && visible(content, getComputedStyle)
      ? { shadow, visual, content }
      : null;
  }, delay, 'the packaged React review workbench did not mount visibly');
  await invoke('renderer_proof_checkpoint', { code: 'review-mounted' });
  if (review.content.getAttribute('aria-pressed') === 'true') {
    review.visual.click();
    await waitFor(
      () => review.visual.getAttribute('aria-pressed') === 'true'
        && review.content.getAttribute('aria-pressed') === 'false',
      delay,
      'the packaged React review workbench did not leave content changes',
    );
    await invoke('renderer_proof_checkpoint', { code: 'review-visual' });
  }
  review.content.click();
  const inline = await waitFor(() => {
    if (review.content.getAttribute('aria-pressed') !== 'true') return null;
    const layout = proofElement(review.shadow, 'content-diff-layout');
    const candidate = buttonNamed(layout, 'Inline');
    return visible(layout, getComputedStyle) && visible(candidate, getComputedStyle)
      ? candidate
      : null;
  }, delay, 'the packaged React review workbench did not switch to content changes');
  await invoke('renderer_proof_checkpoint', { code: 'review-content' });
  inline.click();
  await waitFor(
    () => inline.getAttribute('aria-pressed') === 'true',
    delay,
    'the packaged React review workbench did not switch to the inline layout',
  );
  await invoke('renderer_proof_checkpoint', { code: 'review-inline' });
  return {
    schema: 'mesh-renderer-proof/v1',
    nonce: configuration.nonce,
    surface: 'review',
    mounted: true,
    visible: true,
    interaction: 'content-inline',
    outcome: 'content-inline-selected',
  };
}

async function proveAgentHandoff(configuration, dependencies) {
  const { document, invoke, getComputedStyle, delay } = dependencies;
  await openReactPage('current', dependencies);
  const currentHost = document.getElementById('workspace-current-next');
  let controls = await waitFor(() => {
    const shadow = currentHost?.shadowRoot;
    const mounted = proofElement(shadow, 'current-mounted');
    const available = proofElement(shadow, 'current-agent-available');
    const start = proofElement(shadow, 'current-start-codex');
    return visible(currentHost, getComputedStyle)
      && visible(mounted, getComputedStyle)
      && visible(available, getComputedStyle)
      && visible(start, getComputedStyle)
      && !start.disabled
      ? { shadow, start }
      : null;
  }, delay, 'the packaged agent handoff did not mount an available Current surface');
  controls.start.focus({ preventScroll: true });
  controls.start.click();
  await invoke('renderer_proof_checkpoint', { code: 'agent-handoff-start-clicked' });
  controls = await waitFor(() => {
    const shadow = currentHost?.shadowRoot;
    const assigned = proofElement(shadow, 'current-agent-assigned');
    const finish = proofElement(shadow, 'current-finish-agent');
    return visible(currentHost, getComputedStyle)
      && visible(assigned, getComputedStyle)
      && visible(finish, getComputedStyle)
      && !finish.disabled
      ? { shadow, finish }
      : null;
  }, delay, 'the packaged agent handoff did not become assigned after launch');
  await invoke('renderer_proof_checkpoint', { code: 'agent-handoff-assigned' });
  controls.finish.focus({ preventScroll: true });
  controls.finish.click();
  await invoke('renderer_proof_checkpoint', { code: 'agent-handoff-finish-clicked' });
  const confirmationHost = document.getElementById('confirmation-dialog-next');
  const accept = await waitFor(() => {
    const shadow = confirmationHost?.shadowRoot;
    const backdrop = proofElement(shadow, 'confirmation-backdrop');
    const button = proofElement(shadow, 'confirmation-accept');
    return visible(confirmationHost, getComputedStyle)
      && visible(backdrop, getComputedStyle)
      && visible(button, getComputedStyle)
      && !button.disabled
      ? button
      : null;
  }, delay, 'the packaged agent handoff did not render its Finish confirmation');
  accept.click();
  await invoke('renderer_proof_checkpoint', { code: 'agent-handoff-confirmed' });
  await waitFor(() => {
    const shadow = currentHost?.shadowRoot;
    const available = proofElement(shadow, 'current-agent-available');
    const start = proofElement(shadow, 'current-start-codex');
    const notice = proofElement(
      document.getElementById('mesh-app-next')?.shadowRoot,
      'production-notice',
    );
    return visible(currentHost, getComputedStyle)
      && visible(available, getComputedStyle)
      && visible(start, getComputedStyle)
      && !start.disabled
      && visible(notice, getComputedStyle)
      && notice?.getAttribute('data-mesh-agent-proof') === 'agent-handoff-rescanned'
      && /Agent folder released.*authenticated and saved privately/u.test(notice.textContent || '')
      ? available
      : null;
  }, delay, 'the packaged agent handoff did not finish, rescan, and save its result');
  await invoke('renderer_proof_checkpoint', { code: 'agent-handoff-complete' });
  return {
    schema: 'mesh-renderer-proof/v1',
    nonce: configuration.nonce,
    surface: 'agent-handoff',
    mounted: true,
    visible: true,
    interaction: 'start-finish-rescan',
    outcome: 'agent-handoff-completed',
  };
}

async function proveVersions(configuration, dependencies) {
  const { document, invoke, getComputedStyle, delay, now } = dependencies;
  await openReactPage('versions', dependencies);
  await invoke('renderer_proof_checkpoint', { code: 'versions-start' });
  const host = document.getElementById('workspace-versions-next');
  const controls = await waitFor(() => {
    const shadow = host?.shadowRoot;
    const mounted = proofElement(shadow, 'workspace-versions');
    const choice = proofElement(shadow, 'workspace-version-choice');
    return visible(host, getComputedStyle)
      && visible(mounted, getComputedStyle)
      && visible(choice, getComputedStyle)
      && !choice.disabled
      ? { shadow, choice }
      : null;
  }, delay, 'the packaged React workspace versions did not mount visibly');
  await invoke('renderer_proof_checkpoint', { code: 'versions-mounted' });
  controls.choice.click();
  await invoke('renderer_proof_checkpoint', { code: 'versions-clicked' });
  let retryAfter = now() + 1_000;
  await waitFor(() => {
    const preview = proofElement(controls.shadow, 'workspace-version-preview-ready');
    const open = proofElement(controls.shadow, 'workspace-version-open');
    const ready = visible(host, getComputedStyle)
      && visible(preview, getComputedStyle)
      && visible(open, getComputedStyle)
      && !open.disabled;
    if (ready) return preview;

    // Startup and the five-second native scan can replace the coordinator projection between
    // finding this button and dispatching its generation-bound intent. Re-acquire and retry only
    // while React still reports that no saved point is selected. Once selection is accepted the
    // preview may legitimately take time, and another click would restart that verification.
    const currentTime = now();
    const currentChoice = proofElement(controls.shadow, 'workspace-version-choice');
    const previewError = proofElement(controls.shadow, 'workspace-version-preview-error');
    if (currentTime >= retryAfter
      && visible(host, getComputedStyle)
      && visible(currentChoice, getComputedStyle)
      && !currentChoice.disabled
      && (currentChoice.getAttribute('aria-checked') !== 'true'
        || visible(previewError, getComputedStyle))) {
      currentChoice.click();
      retryAfter = currentTime + 1_000;
    }
    return null;
  }, delay, 'the packaged React workspace versions did not reach an actionable verified preview');
  return {
    schema: 'mesh-renderer-proof/v1',
    nonce: configuration.nonce,
    surface: 'versions',
    mounted: true,
    visible: true,
    interaction: 'select-saved-point',
    outcome: 'verified-preview-ready',
  };
}

async function provePrivateExport(configuration, dependencies) {
  const { document, invoke, getComputedStyle, delay } = dependencies;
  await openReactPage('review', dependencies);
  let projectedDestination = null;
  let mountedDestinationGeneration = null;
  let destinationRejected = false;
  document.addEventListener?.('mesh:workspace-destination-projection', (event) => {
    projectedDestination = event?.detail?.destination?.destination ?? null;
  });
  document.addEventListener?.('mesh:workspace-destination-mounted', (event) => {
    mountedDestinationGeneration = event?.detail?.generation ?? null;
  });
  document.addEventListener?.('mesh:workspace-destination-rejected', () => {
    destinationRejected = true;
  });
  await invoke('renderer_proof_checkpoint', { code: 'private-export-start' });
  const host = document.getElementById('review-workbench-next');
  const choose = await waitFor(() => {
    const shadow = host?.shadowRoot;
    const mounted = proofElement(shadow, 'review-mounted');
    const candidate = buttonNamed(shadow, 'Choose export folder');
    return visible(host, getComputedStyle)
      && visible(mounted, getComputedStyle)
      && visible(candidate, getComputedStyle)
      && !candidate.disabled
      ? candidate
      : null;
  }, delay, 'the packaged private export controls did not mount visibly');
  await invoke('renderer_proof_checkpoint', { code: 'private-export-mounted' });
  const destinationHost = document.getElementById('workspace-destination-next');
  const priorDestinationGeneration = proofElement(
    destinationHost?.shadowRoot,
    'workspace-destination',
  )?.getAttribute('data-mesh-generation') || null;
  choose.click();
  await invoke('renderer_proof_checkpoint', { code: 'private-export-clicked' });

  const destinationShadow = await waitFor(() => {
    const shadow = destinationHost?.shadowRoot;
    const mounted = proofElement(shadow, 'workspace-destination');
    const generation = mounted?.getAttribute('data-mesh-generation');
    return mounted && generation && generation !== priorDestinationGeneration ? shadow : null;
  }, delay, 'the packaged private export controls did not mount in the React destination');
  await invoke('renderer_proof_checkpoint', { code: 'private-export-destination-mounted' });
  const readControls = () => {
    const mounted = proofElement(destinationShadow, 'workspace-destination');
    const selected = proofElement(destinationShadow, 'destination-selected');
    const chooseDestination = proofElement(destinationShadow, 'destination-choose');
    const preview = proofElement(destinationShadow, 'destination-preview-all');
    const confirm = proofElement(destinationShadow, 'destination-confirm-all');
    const hint = proofElement(destinationShadow, 'destination-hint');
    const notice = proofElement(
      document.getElementById('mesh-app-next')?.shadowRoot,
      'production-notice',
    );
    return visible(destinationHost, getComputedStyle)
      && interactableHost(destinationHost)
      && visible(mounted, getComputedStyle)
      && visible(selected, getComputedStyle)
      && visible(chooseDestination, getComputedStyle)
      && visible(preview, getComputedStyle)
      && visible(confirm, getComputedStyle)
      && visible(hint, getComputedStyle)
      && notice
      ? { shadow: destinationShadow, selected, chooseDestination, preview, confirm, hint, notice }
      : null;
  };
  let controls = await waitFor(
    readControls,
    delay,
    'the packaged private export controls did not mount visibly in the React destination',
  );
  await invoke('renderer_proof_checkpoint', { code: 'private-export-destination-visible' });

  await waitFor(
    () => {
      const current = readControls();
      if (!current
        || (current.selected.textContent !== '' && current.selected.textContent !== configuration.source)
        || !current.preview.disabled
        || current.chooseDestination.disabled
        || !/original project folder|original folder|different ordinary folder/u.test(current.notice.textContent || '')) return false;
      controls = current;
      return true;
    },
    delay,
    'the packaged private export did not refuse the original project',
  );
  await invoke('renderer_proof_checkpoint', { code: 'private-export-original-refused' });

  controls = await waitFor(readControls, delay, 'the packaged private export controls became unavailable');
  const refusalGeneration = proofElement(
    destinationShadow,
    'workspace-destination',
  )?.getAttribute('data-mesh-generation');
  controls.chooseDestination.click();
  await delay(500);
  await invoke('renderer_proof_checkpoint', { code: projectedDestination === configuration.destination
    ? 'private-export-projection-target-ready'
    : 'private-export-projection-target-empty' });
  await invoke('renderer_proof_checkpoint', { code: mountedDestinationGeneration !== null
    && String(mountedDestinationGeneration) !== refusalGeneration
    ? 'private-export-react-generation-advanced'
    : 'private-export-react-generation-stale' });
  if (destinationRejected) {
    await invoke('renderer_proof_checkpoint', { code: 'private-export-react-rejected' });
  }
  const observedAfterPicker = readControls();
  await invoke('renderer_proof_checkpoint', { code: !observedAfterPicker || observedAfterPicker.selected.textContent === ''
    ? 'private-export-target-empty'
    : observedAfterPicker.selected.textContent === configuration.source
      ? 'private-export-target-source'
      : observedAfterPicker.selected.textContent === configuration.destination
        ? 'private-export-target-ready'
        : 'private-export-target-other' });
  await waitFor(
    () => {
      const current = readControls();
      const generation = proofElement(
        destinationShadow,
        'workspace-destination',
      )?.getAttribute('data-mesh-generation');
      if (!current
        || !generation
        || generation === refusalGeneration
        || current.selected.textContent !== configuration.destination) return false;
      controls = current;
      return true;
    },
    delay,
    'the packaged private export did not display the confined destination',
  );
  await invoke('renderer_proof_checkpoint', { code: 'private-export-target-accepted' });
  await waitFor(
    () => {
      const current = readControls();
      if (!current || current.preview.disabled) return false;
      controls = current;
      return true;
    },
    delay,
    'the packaged private export did not enable the confined destination preview',
  );
  await invoke('renderer_proof_checkpoint', { code: 'private-export-preview-enabled' });
  controls.preview.click();
  const ready = await waitFor(
    () => {
      const current = readControls();
      const plan = proofElement(destinationShadow, 'destination-plan');
      return visible(destinationHost, getComputedStyle)
        && current
        && visible(plan, getComputedStyle)
        && plan.textContent?.includes(configuration.destination)
        && !current.confirm.disabled
        ? current
        : null;
    },
    delay,
    'the packaged private export did not render an actionable whole-workspace preview',
  );
  controls = ready;
  await invoke('renderer_proof_checkpoint', { code: 'private-export-preview-ready' });

  // The application asks the nonce-bound native proof runtime to accept this exact destination.
  // That command is single-use and unavailable unless Rust confined the entire session to the
  // verifier-owned /tmp home. Normal launches retain the ordinary human confirmation dialog.
  controls.confirm.click();
  await waitFor(
    () => visible(controls.notice, getComputedStyle)
      && /^Saved changes, moves, and deletions are applied to /u.test(controls.notice.textContent || ''),
    delay,
    'the packaged private export did not complete the confirmed private copy',
  );
  return {
    schema: 'mesh-renderer-proof/v1',
    nonce: configuration.nonce,
    surface: 'private-export',
    mounted: true,
    visible: true,
    interaction: 'refuse-original-then-confirm-private',
    outcome: 'private-export-completed',
  };
}

export async function runRendererProof({
  document,
  invoke,
  getComputedStyle,
  delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds)),
  now = () => Date.now(),
}) {
  const configuration = exactConfiguration(JSON.parse(await invoke('renderer_proof_configuration')));
  const dependencies = { document, invoke, getComputedStyle, delay, now };
  const report = configuration.surface === 'onboarding'
    ? await proveOnboarding(configuration, dependencies)
    : configuration.surface === 'review'
      ? await proveReview(configuration, dependencies)
      : configuration.surface === 'versions'
        ? await proveVersions(configuration, dependencies)
        : configuration.surface === 'private-export'
          ? await provePrivateExport(configuration, dependencies)
          : await proveAgentHandoff(configuration, dependencies);
  await invoke('renderer_proof_report', { report: JSON.stringify(report) });
  return report;
}

export function rendererProofFailureCode(error) {
  const message = error instanceof Error ? error.message : '';
  if (message.includes('onboarding did not mount')) return 'onboarding-mount';
  if (message.includes('did not accept the proof path')) return 'onboarding-path';
  if (message.includes('did not render the verified preview')) return 'onboarding-preview';
  if (message.includes('review workbench did not mount')) return 'review-mount';
  if (message.includes('did not leave content changes')) return 'review-content';
  if (message.includes('did not switch to content changes')) return 'review-content';
  if (message.includes('did not switch to the inline layout')) return 'review-inline';
  if (message.includes('workspace versions did not mount')) return 'versions-mount';
  if (message.includes('workspace versions did not reach')) return 'versions-preview';
  if (message.includes('private export controls did not mount')) return 'private-export-mount';
  if (message.includes('private export did not refuse')) return 'private-export-original';
  if (message.includes('private export')) return 'private-export-complete';
  if (message.includes('did not mount an available Current')) return 'agent-handoff-mount';
  if (message.includes('did not become assigned')) return 'agent-handoff-start';
  if (message.includes('agent handoff')) return 'agent-handoff-finish';
  return 'configuration';
}

if (typeof window !== 'undefined' && window.__TAURI__?.core?.invoke) {
  runRendererProof({
    document: window.document,
    invoke: window.__TAURI__.core.invoke,
    getComputedStyle: window.getComputedStyle.bind(window),
  }).catch((error) => {
    // A normal launch has no nonce and must remain behaviorally identical. In a proof launch the
    // outer verifier owns the deadline and fails closed when no exact success report is emitted.
    window.__TAURI__.core.invoke('renderer_proof_failure', { code: rendererProofFailureCode(error) }).catch(() => {});
  });
}
