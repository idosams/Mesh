const CONFIGURATION_KEYS = ['schema', 'nonce', 'surface', 'source', 'destination'];
const NONCE = /^[0-9a-f]{64}$/u;

function exactConfiguration(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)
    || JSON.stringify(Object.keys(value)) !== JSON.stringify(CONFIGURATION_KEYS)
    || value.schema !== 'mesh-renderer-proof-config/v2'
    || typeof value.nonce !== 'string'
    || !NONCE.test(value.nonce)
    || !['onboarding', 'files', 'review', 'versions', 'private-export', 'agent-handoff'].includes(value.surface)
    || (value.surface === 'onboarding'
      && (typeof value.source !== 'string' || !value.source || value.destination !== null))
    || ((value.surface === 'files' || value.surface === 'review' || value.surface === 'versions')
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

async function waitFor(read, delay, message, { deadlineMs = 40_000, maximumAttempts = 800 } = {}) {
  // A clean archive starts the freshly linked WKWebView immediately after the native build and
  // full desktop suite. WebKit may clamp a background window's 50 ms timers toward one second, so
  // bind the wait to elapsed wall time as well as attempts instead of silently stretching it to
  // several minutes.
  const deadline = Date.now() + deadlineMs;
  for (let attempt = 0; attempt < maximumAttempts && Date.now() < deadline; attempt += 1) {
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

function enabledButtonNamed(root, label) {
  return [...(root?.querySelectorAll('button') || [])]
    .find((button) => button.textContent?.trim() === label && !button.disabled) || null;
}

function buttonContaining(root, label) {
  return [...(root?.querySelectorAll('button') || [])]
    .find((button) => button.textContent?.includes(label)) || null;
}

function closedReviewUnavailableReason(status) {
  return /could not verify a complete bounded review/u.test(status)
    || /has concurrent saved heads/u.test(status);
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
  // A rejected empty folder must leave the same window ready for a valid import.
  const emptySource = `${configuration.source}-empty`;
  await waitFor(() => {
    if (controls.input.value !== emptySource) installInputValue(controls.input, emptySource, document);
    return !controls.preview.disabled && controls.input.value === emptySource;
  }, delay, 'the packaged onboarding did not accept the empty-folder proof path');
  controls.preview.focus({ preventScroll: true });
  controls.preview.click();
  await waitFor(() => {
    if (proofElement(controls.shadow, 'import-verified-preview')
      || buttonNamed(controls.shadow, 'Create workspace and open folder')) {
      throw new Error('the packaged onboarding offered confirmation for an empty folder');
    }
    const notice = proofElement(
      document.getElementById('mesh-app-next')?.shadowRoot || controls.shadow,
      'production-notice',
    );
    return visible(notice, getComputedStyle)
      && /no importable files or folders after exclusions/u.test(notice.textContent || '')
      && /Choose a folder containing project files/u.test(notice.textContent || '')
      && /no workspace was created/u.test(notice.textContent || '');
  }, delay, 'the packaged onboarding did not show actionable empty-folder refusal');
  await dependencies.invoke('renderer_proof_checkpoint', { code: 'onboarding-empty-refused' });
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
  const confirm = await waitFor(
    () => enabledButtonNamed(controls.shadow, 'Create workspace and open folder'),
    delay,
    'the packaged React onboarding did not expose exact import confirmation',
  );
  confirm.click();
  await waitFor(
    () => {
      const verified = proofElement(controls.shadow, 'import-verified-preview');
      const progress = proofElement(controls.shadow, 'import-confirmation-progress');
      const creating = buttonNamed(controls.shadow, 'Creating private workspace…');
      return verified?.getAttribute('aria-busy') === 'true'
        && visible(progress, getComputedStyle)
        && creating?.disabled === true
        ? progress
        : null;
    },
    delay,
    'the packaged React onboarding did not expose its bounded import progress state',
  );
  await waitFor(
    () => !proofElement(controls.shadow, 'import-verified-preview'),
    delay,
    'the packaged React onboarding did not finish the confirmed import',
    { deadlineMs: 900_000, maximumAttempts: 18_000 },
  );
  return {
    schema: 'mesh-renderer-proof/v1',
    nonce: configuration.nonce,
    surface: 'onboarding',
    mounted: true,
    visible: true,
    interaction: 'preview-path-confirm-import',
    outcome: 'import-completed-after-busy',
  };
}

export async function proveLanguageSelection({ document, invoke, getComputedStyle, delay }) {
  const shell = document.getElementById('mesh-app-next');
  for (const [locale, direction, label] of [['he', 'rtl', 'קבצים'], ['en', 'ltr', 'Files']]) {
    const picker = await waitFor(() => {
      const current = proofElement(shell?.shadowRoot, 'language-picker');
      return visible(current, getComputedStyle) && !current.disabled ? current : null;
    }, delay, 'the packaged Files language picker was unavailable');
    picker.value = locale;
    picker.dispatchEvent(new document.defaultView.Event('change', { bubbles: true }));
    await waitFor(() => {
      const navigation = buttonNamed(shell.shadowRoot, label);
      return document.documentElement.lang === locale
        && document.documentElement.dir === direction
        && document.defaultView.localStorage.getItem('mesh.ui.locale.v1') === locale
        && visible(navigation, getComputedStyle);
    }, delay, 'the packaged Files language selection did not update navigation, direction, and preference');
    if (locale === 'he') await invoke('renderer_proof_checkpoint', { code: 'files-hebrew-verified' });
  }
}

async function proveFiles(configuration, dependencies) {
  const { document, invoke, getComputedStyle, delay, now } = dependencies;
  await openReactPage('files', dependencies);
  const host = document.getElementById('workspace-files-next');
  const mounted = await waitFor(() => {
    const shadow = host?.shadowRoot;
    const explorer = proofElement(shadow, 'files-explorer');
    const tree = proofElement(shadow, 'files-tree');
    const filter = proofElement(shadow, 'files-filter');
    const folder = buttonContaining(shadow, 'assets');
    return visible(host, getComputedStyle)
      && visible(explorer, getComputedStyle)
      && visible(tree, getComputedStyle)
      && visible(filter, getComputedStyle)
      && visible(folder, getComputedStyle)
      && !folder.disabled
      ? { shadow, folder }
      : null;
  }, delay, 'the packaged React Files explorer did not mount visibly');
  await invoke('renderer_proof_checkpoint', { code: 'files-mounted' });

  mounted.folder.click();
  const file = await waitFor(
    () => {
      const candidate = buttonContaining(host.shadowRoot, 'mesh-proof.png');
      return visible(candidate, getComputedStyle) && !candidate.disabled ? candidate : null;
    },
    delay,
    'the packaged React Files explorer did not expand the nested proof folder',
  );
  await invoke('renderer_proof_checkpoint', { code: 'files-folder-expanded' });
  file.click();
  await waitFor(
    () => {
      const selected = proofElement(host.shadowRoot, 'files-selected-entry');
      return visible(selected, getComputedStyle)
        && selected.getAttribute('data-mesh-entry-path') === 'assets/mesh-proof.png'
        ? selected
        : null;
    },
    delay,
    'the packaged React Files explorer did not select the nested proof file',
  );
  await invoke('renderer_proof_checkpoint', { code: 'files-file-selected' });
  const screenshot = JSON.parse(await invoke('renderer_proof_capture_files_screenshot'));
  const screenshotKeys = screenshot && typeof screenshot === 'object' && !Array.isArray(screenshot)
    ? Object.keys(screenshot).sort()
    : [];
  const capturedScreenshot = screenshot?.captured === true
    && typeof screenshot.path === 'string'
    && screenshot.path.length > 0
    && screenshot.path.length <= 4_096
    && Number.isSafeInteger(screenshot.width)
    && screenshot.width >= 320
    && screenshot.width <= 8_192
    && Number.isSafeInteger(screenshot.height)
    && screenshot.height >= 240
    && screenshot.height <= 8_192
    && Number.isSafeInteger(screenshot.bytes)
    && screenshot.bytes >= 1_024
    && screenshot.bytes <= 16 * 1_024 * 1_024
    && typeof screenshot.sha256 === 'string'
    && /^[0-9a-f]{64}$/u.test(screenshot.sha256);
  const dormantScreenshot = screenshot?.captured === false
    && screenshot.path === null
    && screenshot.width === null
    && screenshot.height === null
    && screenshot.bytes === null
    && screenshot.sha256 === null;
  if (screenshotKeys.join(',') !== 'bytes,captured,height,path,schema,sha256,width'
    || screenshot.schema !== 'mesh.renderer-proof-screenshot/v1'
    || (!capturedScreenshot && !dormantScreenshot)) {
    throw new Error('the packaged React Files screenshot response was invalid');
  }

  const selectedPath = () => proofElement(host.shadowRoot, 'files-selected-entry')
    ?.getAttribute('data-mesh-entry-path');
  const readNotice = () => proofElement(
    document.getElementById('mesh-app-next')?.shadowRoot,
    'production-notice',
  );
  const completeSelectedAction = async ({ label, path, noticeText, exposureFailure, completionFailure }) => {
    await waitFor(
      () => selectedPath() === path ? enabledButtonNamed(host.shadowRoot, label) : null,
      delay,
      exposureFailure,
    );
    let attempts = 0;
    let retryAfter = 0;
    await waitFor(
      () => {
        const notice = readNotice();
        if (visible(notice, getComputedStyle) && notice.textContent === noticeText) return true;
        if (selectedPath() !== path) return false;
        const action = enabledButtonNamed(host.shadowRoot, label);
        if (action && attempts < 3 && now() >= retryAfter) {
          attempts += 1;
          retryAfter = now() + 1_000;
          action.click();
        }
        return false;
      },
      delay,
      completionFailure,
    );
  };
  await completeSelectedAction({
    label: 'Open',
    path: 'assets/mesh-proof.png',
    noticeText: 'Opened assets/mesh-proof.png with its default application.',
    exposureFailure: 'the packaged React Files explorer did not expose native file opening',
    completionFailure: 'the packaged React Files explorer did not open its exact selected file',
  });
  await invoke('renderer_proof_checkpoint', { code: 'files-file-opened' });

  await completeSelectedAction({
    label: 'Reveal',
    path: 'assets/mesh-proof.png',
    noticeText: 'Revealed assets/mesh-proof.png in Finder.',
    exposureFailure: 'the packaged React Files explorer did not expose native file reveal',
    completionFailure: 'the packaged React Files explorer did not reveal its exact selected file',
  });
  await invoke('renderer_proof_checkpoint', { code: 'files-file-revealed' });

  const folder = await waitFor(
    () => {
      const candidate = buttonContaining(host.shadowRoot, 'assets');
      return visible(candidate, getComputedStyle) && !candidate.disabled ? candidate : null;
    },
    delay,
    'the packaged React Files explorer lost its proof folder',
  );
  let folderSelectionAttempts = 0;
  let folderRetryAfter = 0;
  await waitFor(
    () => {
      if (selectedPath() === 'assets') return true;
      const currentFolder = buttonContaining(host.shadowRoot, 'assets');
      if (currentFolder && !currentFolder.disabled && folderSelectionAttempts < 3 && now() >= folderRetryAfter) {
        folderSelectionAttempts += 1;
        folderRetryAfter = now() + 1_000;
        currentFolder.click();
      }
      return false;
    },
    delay,
    'the packaged React Files explorer did not select its proof folder',
  );
  await completeSelectedAction({
    label: 'Open in Finder',
    path: 'assets',
    noticeText: 'Opened assets in Finder.',
    exposureFailure: 'the packaged React Files explorer did not expose selected-folder opening',
    completionFailure: 'the packaged React Files explorer did not open its exact selected folder',
  });
  await invoke('renderer_proof_checkpoint', { code: 'files-folder-opened' });

  await waitFor(
    () => enabledButtonNamed(host.shadowRoot, 'Open folder'),
    delay,
    'the packaged React Files explorer did not expose workspace-folder opening',
  );
  let workspaceOpenAttempts = 0;
  let workspaceRetryAfter = 0;
  await waitFor(
    () => {
      const notice = readNotice();
      if (visible(notice, getComputedStyle)
        && notice.textContent === 'Opened the current workspace folder in Finder.') return true;
      const action = enabledButtonNamed(host.shadowRoot, 'Open folder');
      if (action && workspaceOpenAttempts < 3 && now() >= workspaceRetryAfter) {
        workspaceOpenAttempts += 1;
        workspaceRetryAfter = now() + 1_000;
        action.click();
      }
      return false;
    },
    delay,
    'the packaged React Files explorer did not open the current workspace folder',
  );
  await invoke('renderer_proof_checkpoint', { code: 'files-workspace-opened' });
  await proveLanguageSelection(dependencies);
  return {
    schema: 'mesh-renderer-proof/v1',
    nonce: configuration.nonce,
    surface: 'files',
    mounted: true,
    visible: true,
    interaction: 'expand-select-open-reveal-folders',
    outcome: 'native-file-and-folder-actions-completed',
  };
}

async function proveReview(configuration, dependencies) {
  const { document, invoke, getComputedStyle, delay, now } = dependencies;
  await openReactPage('review', dependencies);
  const host = document.getElementById('review-workbench-next');
  const readReview = () => {
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
  };
  const initialReview = await waitFor(() => {
    const ready = readReview();
    if (ready) return { state: 'ready', ...ready };
    const shadow = host?.shadowRoot;
    const unavailable = proofElement(shadow, 'review-unavailable');
    return visible(host, getComputedStyle) && visible(unavailable, getComputedStyle)
      && !/has not verified the current workspace/u.test(unavailable.textContent || '')
      ? { state: 'unavailable', shadow, unavailable }
      : null;
  }, delay, 'the packaged React review workbench did not mount visibly');
  if (initialReview.state === 'unavailable') {
    const status = initialReview.unavailable.textContent || '';
    if (!/Review details are unavailable/u.test(status)
      || !closedReviewUnavailableReason(status)) {
      throw new Error('the packaged bounded review did not disclose its incomplete state');
    }
    const decisionLabels = new Set([
      'Set up approval',
      'Record reviewed version',
      'Confirm review complete',
      'Choose export folder',
      'Approve exact version',
      'Approve and create Git branch',
      'Create Git branch',
    ]);
    const enabledDecision = [...(initialReview.shadow?.querySelectorAll('button') || [])]
      .some((button) => visible(button, getComputedStyle)
        && !button.disabled
        && decisionLabels.has(button.textContent?.trim()));
    if (enabledDecision) {
      throw new Error('the packaged bounded review exposed an action without complete review authority');
    }
    await invoke('renderer_proof_checkpoint', { code: 'review-bounded-unavailable' });
    return {
      schema: 'mesh-renderer-proof/v1',
      nonce: configuration.nonce,
      surface: 'review',
      mounted: true,
      visible: true,
      interaction: 'bounded-incomplete-review-inspection',
      outcome: 'incomplete-review-disclosed-without-authority',
    };
  }
  await invoke('renderer_proof_checkpoint', { code: 'review-mounted' });
  const textChange = await waitFor(
    () => buttonContaining(readReview()?.shadow, 'agent-proof-result.txt'),
    delay,
    'the packaged React review workbench did not expose the packaged text change',
  );
  textChange.click();
  await waitFor(
    () => buttonContaining(readReview()?.shadow, 'agent-proof-result.txt')?.getAttribute('aria-pressed') === 'true',
    delay,
    'the packaged React review workbench did not select the packaged text change',
  );
  await invoke('renderer_proof_checkpoint', { code: 'review-text-selected' });
  if (readReview()?.content.getAttribute('aria-pressed') === 'true') {
    readReview()?.visual.click();
    await waitFor(
      () => readReview()?.visual.getAttribute('aria-pressed') === 'true'
        && readReview()?.content.getAttribute('aria-pressed') === 'false',
      delay,
      'the packaged React review workbench did not leave content changes',
    );
    await invoke('renderer_proof_checkpoint', { code: 'review-visual' });
  }
  readReview()?.content.click();
  const inline = await waitFor(() => {
    const current = readReview();
    if (current?.content.getAttribute('aria-pressed') !== 'true') return null;
    const layout = proofElement(current.shadow, 'content-diff-layout');
    const candidate = buttonNamed(layout, 'Inline');
    return visible(layout, getComputedStyle) && visible(candidate, getComputedStyle)
      ? candidate
      : null;
  }, delay, 'the packaged React review workbench did not switch to content changes');
  await invoke('renderer_proof_checkpoint', { code: 'review-content' });
  inline.click();
  await waitFor(
    () => buttonNamed(
      proofElement(readReview()?.shadow, 'content-diff-layout'),
      'Inline',
    )?.getAttribute('aria-pressed') === 'true',
    delay,
    'the packaged React review workbench did not switch to the inline layout',
  );
  await invoke('renderer_proof_checkpoint', { code: 'review-inline' });
  const imageChange = await waitFor(
    () => buttonContaining(readReview()?.shadow, 'agent-proof-result.png'),
    delay,
    'the packaged React review workbench did not expose the packaged image change',
  );
  imageChange.click();
  let imageSelectionAttempts = 1;
  let imageSelectionRetryAfter = now() + 1_000;
  await waitFor(
    () => {
      const currentImage = buttonContaining(readReview()?.shadow, 'agent-proof-result.png');
      const currentComparison = proofElement(readReview()?.shadow, 'selected-change-comparison');
      const imageChangeId = currentImage?.getAttribute('data-change-option');
      if (currentImage?.getAttribute('aria-pressed') === 'true'
        && typeof imageChangeId === 'string'
        && imageChangeId.length > 0
        && currentComparison?.getAttribute('data-mesh-change-id') === imageChangeId
        && currentComparison?.getAttribute('data-mesh-change-kind') === 'image') return true;
      if (currentImage && imageSelectionAttempts < 3 && now() >= imageSelectionRetryAfter) {
        imageSelectionAttempts += 1;
        imageSelectionRetryAfter = now() + 1_000;
        currentImage.click();
      }
      return false;
    },
    delay,
    'the packaged React review workbench did not select the packaged image change',
  );
  await invoke('renderer_proof_checkpoint', { code: 'review-image-selected' });
  const imageStillSelected = () => {
    const current = readReview();
    const image = buttonContaining(current?.shadow, 'agent-proof-result.png');
    const comparison = proofElement(current?.shadow, 'selected-change-comparison');
    const imageChangeId = image?.getAttribute('data-change-option');
    return image?.getAttribute('aria-pressed') === 'true'
      && typeof imageChangeId === 'string'
      && imageChangeId.length > 0
      && comparison?.getAttribute('data-mesh-change-id') === imageChangeId
      && comparison?.getAttribute('data-mesh-change-kind') === 'image';
  };
  let imageVisualAttempts = 1;
  let imageVisualRetryAfter = now() + 750;
  enabledButtonNamed(readReview()?.shadow, 'Visual')?.click();
  await waitFor(
    () => {
      if (!imageStillSelected()) return false;
      const currentVisual = enabledButtonNamed(readReview()?.shadow, 'Visual');
      if (currentVisual?.getAttribute('aria-pressed') === 'true') return true;
      if (currentVisual && imageVisualAttempts < 10 && now() >= imageVisualRetryAfter) {
        imageVisualAttempts += 1;
        imageVisualRetryAfter = now() + 750;
        currentVisual.click();
      }
      return false;
    },
    delay,
    'the packaged React review workbench did not switch the image change to Visual',
  );
  await invoke('renderer_proof_checkpoint', { code: 'review-image-visual' });
  const loadImage = await waitFor(
    () => enabledButtonNamed(readReview()?.shadow, 'Load visual comparison'),
    delay,
    'the packaged React review workbench did not expose exact image preview loading',
  );
  loadImage.click();
  const readNotice = () => proofElement(
    document.getElementById('mesh-app-next')?.shadowRoot,
    'production-notice',
  );
  let imagePreviewAttempts = 1;
  let imagePreviewRetryAfter = now() + 1_000;
  let imagePreviewSelectionAttempts = 0;
  let imagePreviewSelectionRetryAfter = now() + 1_000;
  let openSaved;
  try {
    openSaved = await waitFor(
      () => {
        const shadow = readReview()?.shadow;
        if (!imageStillSelected()) {
          const currentImage = buttonContaining(shadow, 'agent-proof-result.png');
          if (currentImage
            && imagePreviewSelectionAttempts < 3
            && now() >= imagePreviewSelectionRetryAfter) {
            imagePreviewSelectionAttempts += 1;
            imagePreviewSelectionRetryAfter = now() + 1_000;
            imagePreviewAttempts = 0;
            imagePreviewRetryAfter = now() + 1_000;
            currentImage.click();
          }
          return null;
        }
        const currentVisual = enabledButtonNamed(shadow, 'Visual');
        if (currentVisual?.getAttribute('aria-pressed') !== 'true') {
          currentVisual?.click();
          return null;
        }
        const admitted = enabledButtonNamed(shadow, 'Open in default app');
        if (admitted) return admitted;
        const retry = enabledButtonNamed(shadow, 'Try visual comparison again')
          || enabledButtonNamed(shadow, 'Load visual comparison');
        if (retry && imagePreviewAttempts < 3 && now() >= imagePreviewRetryAfter) {
          imagePreviewAttempts += 1;
          imagePreviewRetryAfter = now() + 1_000;
          retry.click();
        }
        return null;
      },
      delay,
      'the packaged React review workbench did not expose a supported exact saved side',
    );
  } catch (error) {
    const shadow = readReview()?.shadow;
    const alertText = [...(shadow?.querySelectorAll('[role="alert"]') || [])]
      .filter((alert) => visible(alert, getComputedStyle))
      .map((alert) => alert.textContent?.trim() || '')
      .join(' ');
    if (alertText.includes('macOS could not render a visual preview')) {
      throw new Error('the packaged React review workbench native image renderer refused the exact saved side');
    }
    if (alertText.includes('visual preview did not match the exact reviewed artifact')) {
      throw new Error('the packaged React review workbench rejected the exact image preview envelope');
    }
    if (buttonNamed(shadow, 'Rendering exact versions…')?.disabled) {
      throw new Error('the packaged React review workbench left the exact image preview pending');
    }
    if (!imageStillSelected()) {
      throw new Error('the packaged React review workbench lost the exact image selection while previewing');
    }
    if (enabledButtonNamed(shadow, 'Reload visual comparison')) {
      throw new Error('the packaged React review workbench loaded the image preview without native open evidence');
    }
    if (enabledButtonNamed(shadow, 'Try visual comparison again')) {
      throw new Error('the packaged React review workbench returned an unclassified image preview refusal');
    }
    if (enabledButtonNamed(shadow, 'Load visual comparison')) {
      throw new Error('the packaged React review workbench returned the image preview to its idle state');
    }
    throw error;
  }
  await invoke('renderer_proof_checkpoint', { code: 'review-image-preview' });
  const completeNativeSavedAction = async (label, success, failure) => {
    let action = await waitFor(
      () => imageStillSelected() && enabledButtonNamed(readReview()?.shadow, label),
      delay,
      `the packaged React review workbench did not expose ${label}`,
    );
    let actionAttempts = 1;
    let actionRetryAfter = now() + 1_000;
    action.click();
    await waitFor(() => {
      if (!imageStillSelected()) return false;
      const notice = readNotice();
      const currentNotice = notice?.textContent || '';
      if (visible(notice, getComputedStyle) && success.test(currentNotice)) return true;
      action = enabledButtonNamed(readReview()?.shadow, label);
      if (action && actionAttempts < 3 && now() >= actionRetryAfter) {
        actionAttempts += 1;
        actionRetryAfter = now() + 1_000;
        action.click();
      }
      return false;
    }, delay, failure);
  };
  await completeNativeSavedAction(
    'Open in default app',
    /^Opened the exact (before|after) saved copy in its default application\.$/u,
    'the packaged React review workbench did not open an exact saved side in its default application',
  );
  await invoke('renderer_proof_checkpoint', { code: 'review-saved-open' });
  await completeNativeSavedAction(
    'Reveal in Finder',
    /^Revealed the exact (before|after) saved copy in Finder\.$/u,
    'the packaged React review workbench did not reveal an exact saved side in Finder',
  );
  await invoke('renderer_proof_checkpoint', { code: 'review-saved-reveal' });
  return {
    schema: 'mesh-renderer-proof/v1',
    nonce: configuration.nonce,
    surface: 'review',
    mounted: true,
    visible: true,
    interaction: 'content-inline-native-open-reveal',
    outcome: 'saved-side-native-launches-completed',
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
  let clickedGeneration = Number(
    proofElement(controls.shadow, 'current-mounted')?.getAttribute('data-mesh-generation'),
  );
  controls.finish.click();
  await invoke('renderer_proof_checkpoint', { code: 'agent-handoff-finish-clicked' });
  const confirmationHost = document.getElementById('confirmation-dialog-next');
  const accept = await waitFor(() => {
    const shadow = confirmationHost?.shadowRoot;
    const backdrop = proofElement(shadow, 'confirmation-backdrop');
    const button = proofElement(shadow, 'confirmation-accept');
    const accepted = visible(confirmationHost, getComputedStyle)
      && visible(backdrop, getComputedStyle)
      && visible(button, getComputedStyle)
      && !button.disabled
      ? button
      : null;
    if (accepted) return accepted;
    // Live-agent inspection may commit a newer Current projection after this proof read its
    // Finish control but before the coordinator accepts that generation's intent. React normally
    // retains the same keyed button node, so retry only after the mounted projection advances.
    const currentShadow = currentHost?.shadowRoot;
    const mounted = proofElement(currentShadow, 'current-mounted');
    const committedGeneration = Number(mounted?.getAttribute('data-mesh-generation'));
    const replacement = proofElement(currentShadow, 'current-finish-agent');
    if (visible(currentHost, getComputedStyle)
      && Number.isSafeInteger(committedGeneration)
      && committedGeneration > clickedGeneration
      && visible(replacement, getComputedStyle)
      && !replacement.disabled) {
      clickedGeneration = committedGeneration;
      replacement.focus({ preventScroll: true });
      replacement.click();
    }
    return null;
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
  }, delay, 'the packaged agent handoff did not finish, rescan, and save its result', {
    // Finish performs one complete post-release native scan and may authenticate every stable
    // changed file before it can publish success. Keep only this final large-workspace stage on
    // the same bounded fifteen-minute deadline as the user-visible operation.
    deadlineMs: 900_000,
    maximumAttempts: 18_000,
  });
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
  const entry = await waitFor(() => {
    const shadow = host?.shadowRoot;
    const mounted = proofElement(shadow, 'review-mounted');
    const candidate = buttonNamed(shadow, 'Choose export folder');
    if (visible(host, getComputedStyle)
      && visible(mounted, getComputedStyle)
      && visible(candidate, getComputedStyle)
      && !candidate.disabled) return { state: 'ready', choose: candidate };
    const unavailable = proofElement(shadow, 'review-unavailable');
    return visible(host, getComputedStyle) && visible(unavailable, getComputedStyle)
      && !/has not verified the current workspace/u.test(unavailable.textContent || '')
      ? { state: 'unavailable', shadow, unavailable }
      : null;
  }, delay, 'the packaged private export controls did not mount visibly');
  if (entry.state === 'unavailable') {
    const status = entry.unavailable.textContent || '';
    if (!/Review details are unavailable/u.test(status)
      || !closedReviewUnavailableReason(status)
      || buttonNamed(entry.shadow, 'Choose export folder')) {
      throw new Error('the packaged private export did not remain blocked by its incomplete review');
    }
    await invoke('renderer_proof_checkpoint', { code: 'private-export-bounded-blocked' });
    return {
      schema: 'mesh-renderer-proof/v1',
      nonce: configuration.nonce,
      surface: 'private-export',
      mounted: true,
      visible: true,
      interaction: 'bounded-review-private-export-refusal',
      outcome: 'private-export-blocked-without-complete-review',
    };
  }
  const choose = entry.choose;
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
    () => {
      const current = readControls();
      return current
        && current.selected.textContent === configuration.destination
        && current.confirm.disabled
        && visible(current.notice, getComputedStyle)
        && (current.notice.textContent || '').startsWith(
          `Saved changes, moves, and deletions are applied to ${configuration.destination}.`,
        );
    },
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
    : configuration.surface === 'files'
      ? await proveFiles(configuration, dependencies)
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
  if (message.includes('empty-folder') || message.includes('confirmation for an empty folder')) return 'onboarding-preview';
  if (message.includes('onboarding did not mount')) return 'onboarding-mount';
  if (message.includes('did not accept the proof path')) return 'onboarding-path';
  if (message.includes('did not render the verified preview')) return 'onboarding-preview';
  if (message.includes('did not expose exact import confirmation')
    || message.includes('did not expose its bounded import progress state')
    || message.includes('did not finish the confirmed import')) return 'onboarding-confirm';
  if (message.includes('Files language')) return 'files-navigation';
  if (message.includes('Files explorer did not mount')) return 'files-mount';
  if (message.includes('Files explorer did not expand')
    || message.includes('Files explorer did not select')
    || message.includes('Files explorer lost its proof folder')) return 'files-navigation';
  if (message.includes('Files explorer did not expose native file opening')
    || message.includes('Files explorer did not open its exact selected file')) return 'files-native-open';
  if (message.includes('Files explorer did not expose native file reveal')
    || message.includes('Files explorer did not reveal its exact selected file')) return 'files-native-reveal';
  if (message.includes('Files explorer did not expose selected-folder opening')
    || message.includes('Files explorer did not open its exact selected folder')
    || message.includes('Files explorer did not expose workspace-folder opening')
    || message.includes('Files explorer did not open the current workspace folder')) return 'files-folder-open';
  if (message.includes('Files screenshot') || message.includes('Files WebKit snapshot')) return 'files-screenshot';
  if (message.includes('review workbench did not mount')) return 'review-mount';
  if (message.includes('did not leave content changes')) return 'review-content';
  if (message.includes('did not switch to content changes')) return 'review-content';
  if (message.includes('did not switch to the inline layout')) return 'review-inline';
  if (message.includes('packaged text change')) return 'review-content';
  if (message.includes('packaged image change')) return 'review-image-visual';
  if (message.includes('image change to Visual')) return 'review-image-visual';
  if (message.includes('native image renderer refused')) return 'review-image-preview-native';
  if (message.includes('rejected the exact image preview envelope')) return 'review-image-preview-envelope';
  if (message.includes('left the exact image preview pending')) return 'review-image-preview-pending';
  if (message.includes('lost the exact image selection')) return 'review-image-selection-lost';
  if (message.includes('without native open evidence')) return 'review-image-preview-evidence';
  if (message.includes('unclassified image preview refusal')) return 'review-image-preview-refused';
  if (message.includes('image preview to its idle state')) return 'review-image-preview-idle';
  if (message.includes('image preview loading')) return 'review-image-preview';
  if (message.includes('did not expose a supported exact saved side')) return 'review-image-preview';
  if (message.includes('did not open an exact saved side')) return 'review-native-open';
  if (message.includes('did not expose exact saved-side reveal')) return 'review-native-reveal';
  if (message.includes('did not reveal an exact saved side')) return 'review-native-reveal';
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
