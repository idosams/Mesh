// Real mounted controls and native projections only. The enclosing verifier edits its own fixture.
export function pinnedSelections(projection) {
  return JSON.stringify(projection.pins.map(pin => ({
    key: pin.key, project: pin.project, base: pin.selector.base, target: pin.selector.target,
    after: pin.selector.after, path: pin.selector.path,
  })));
}

export async function proveAttachedProjects(configuration, dependencies) {
  const { document, invoke, delay, openReactPage, installInputValue, waitFor, visible, getComputedStyle } = dependencies;
  let projection;
  const update = event => { projection = event.detail; };
  document.addEventListener('mesh:attachments-projection', update);
  const wait = (read, reason) => waitFor(read, delay, `attached proof: ${reason}`);
  const root = () => document.getElementById('import-workbench-next')?.shadowRoot
    ?.querySelector('[data-mesh-proof="attached-projects"]');
  const project = () => projection?.projects.find(item => item.root === configuration.source);
  const card = () => [...(root()?.querySelectorAll('article') || [])]
    .find(item => [...item.querySelectorAll('p > bdi')].some(path => path.textContent === configuration.source)
      && item.querySelector('h4')?.textContent === 'Existing project');
  const click = async (scope, label) => {
    const button = await wait(() => [...(scope()?.querySelectorAll('button') || [])]
      .find(item => item.textContent.trim() === label && !item.disabled && visible(item, getComputedStyle)), label);
    button.focus({ preventScroll: true }); button.click();
  };
  const checkpoint = code => invoke('renderer_proof_checkpoint', { code });
  const ready = () => projection?.available && !projection.busy && !projection.error;
  const history = async () => {
    await click(card, 'Show latest versions');
    return wait(() => ready() && projection.histories[project().id]?.versions, 'saved version list');
  };
  const row = version => () => [...(card()?.querySelectorAll('ol > li') || [])]
    .find(item => item.querySelector('button > bdi')?.textContent === version);
  const compareAndPin = async (base, target, count) => {
    await click(row(base), 'Use as base');
    await click(row(target), 'Compare with base');
    await wait(() => ready() && projection.comparisons[project().id]?.base === base
      && projection.comparisons[project().id]?.target === target, 'exact comparison');
    const file = await wait(() => [...(card()?.querySelectorAll('[aria-label="Saved version comparison"] button') || [])]
      .find(item => item.textContent.includes('notes.txt') && !item.disabled), 'saved text control');
    file.click();
    const expectedBefore = count === 1 ? 'first external version\n' : 'second external version\n';
    const expectedAfter = count === 1 ? 'second external version\n' : 'third external version\n';
    await wait(() => ready() && projection.comparisons[project().id]?.file?.before?.text === expectedBefore
      && projection.comparisons[project().id]?.file?.after?.text === expectedAfter, 'immutable saved text bytes');
    await click(card, 'Pin comparison alongside others');
    await wait(() => projection.pins.length === count && projection.pinStatus === 'saved'
      && !projection.pinError && projection.pins.every(pin => pin.comparison), 'durable comparison pins');
    await wait(() => {
      const shown = root()?.querySelector('[aria-label="Pinned comparisons"]');
      const text = [...(shown?.querySelectorAll('pre') || [])].map(item => item.textContent);
      return visible(shown, getComputedStyle)
        && shown.querySelectorAll('[aria-label="Saved version comparison"]').length === count
        && text.includes(expectedBefore) && text.includes(expectedAfter);
    }, 'exact saved text visibly rendered in parallel pins');
  };
  try {
    await openReactPage('import', dependencies);
    await wait(() => visible(root(), getComputedStyle) && ready(), 'mounted attachment controls');
    const restart = configuration.surface === 'attached-projects-restart';
    if (restart) {
      await wait(() => project()?.recovery === 'restored-stopped' && projection.pins.length === 2
        && projection.pinStatus === 'saved' && projection.pins.every(pin => pin.comparison), 'restored history and pins');
      const pins = pinnedSelections(projection);
      const previous = project().savedVersion;
      if (projection.projects.some(item => !['stopped', 'failed'].includes(item.phase))) {
        throw new Error('attached proof: restart resumed capture without intent');
      }
      await click(card, 'Resume capture');
      await wait(() => project().phase === 'waiting', 'explicit resume');
      await checkpoint('attached-resumed');
      await wait(() => project().savedVersion && project().savedVersion !== previous, 'external edit after restart');
      await click(card, 'Detach Mesh');
      await wait(() => project().detached && ready(), 'explicit detach');
      if (pinnedSelections(projection) !== pins) throw new Error('attached proof: restart changed pinned selections');
      await click(card, 'Show latest versions');
      await wait(() => projection.histories[project().id]?.versions.length >= 5, 'history after detach');
    } else {
      const input = await wait(() => [...root().querySelectorAll('label')]
        .find(item => item.textContent.includes('Existing project path'))?.querySelector('input'), 'attachment path');
      installInputValue(input, configuration.source, document);
      await click(root, 'Attach existing project');
      await wait(() => project()?.savedVersion && ready(), 'first saved version');
      const first = project().savedVersion;
      await checkpoint('attached-initial');
      await wait(() => project().savedVersion !== first && project().phase === 'waiting', 'second external version');
      const second = project().savedVersion;
      await history();
      await compareAndPin(first, second, 1);
      const firstPin = pinnedSelections(projection);
      await checkpoint('attached-second');
      await wait(() => project().savedVersion !== second && project().phase === 'waiting', 'third external version');
      const third = project().savedVersion;
      if (pinnedSelections(projection) !== firstPin) throw new Error('attached proof: capture changed first pin');
      await history();
      await compareAndPin(second, third, 2);
      const pins = pinnedSelections(projection);
      await checkpoint('attached-parallel');
      await wait(() => project().savedVersion !== third && project().phase === 'waiting', 'fourth external version');
      if (pinnedSelections(projection) !== pins) throw new Error('attached proof: capture changed parallel pins');
      await history();
      await click(row(first), 'Create line from this version');
      await wait(() => ready() && projection.projects.some(item => item.lane?.sourceProject === project().id
        && item.lane.sourceVersion === first && !item.lane.unavailable), 'independent line from original saved input');
      if (pinnedSelections(projection) !== pins) throw new Error('attached proof: allocation changed parallel pins');
    }
    return { schema: 'mesh-renderer-proof/v1', nonce: configuration.nonce, surface: configuration.surface,
      mounted: true, visible: true, interaction: restart ? 'restore-resume-detach' : 'attach-capture-pin-fork',
      outcome: restart ? 'saved-comparisons-survive-restart' : 'parallel-saved-comparisons-retained' };
  } finally { document.removeEventListener('mesh:attachments-projection', update); }
}
