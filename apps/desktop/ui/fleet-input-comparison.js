// Read-only projection validation. Native code owns history, content verification and authority.
const check = value => { if (!value) throw new Error('Starting-version comparison could not be verified'); };
const hex = (value, length) => typeof value === 'string' && new RegExp(`^[a-f0-9]{${length}}$`).test(value);
const count = value => Number.isSafeInteger(value) && value >= 0;
const safe = value => typeof value === 'string' && !/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u0080-\u009f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(value);
function side(value, selected) {
  if (value === null) return null;
  check(value && safe(value.path) && value.path.length > 0 && value.path.length <= 4096
    && !value.path.startsWith('/') && !value.path.split('/').some(part => ['', '.', '..'].includes(part)));
  check(['file', 'folder'].includes(value.kind));
  if (value.kind === 'folder') check(value.digest === null && value.bytes === null && value.executable === null && value.content_state === 'folder' && value.text === null);
  else {
    check(hex(value.digest, 64) && count(value.bytes) && typeof value.executable === 'boolean');
    check((selected ? ['text', 'too-large', 'binary-or-unsafe-text'] : ['not-requested']).includes(value.content_state));
    if (value.content_state === 'text') check(typeof value.text === 'string' && value.text.length <= 262144 && safe(value.text) && value.bytes <= 262144 && new TextEncoder().encode(value.text).length === value.bytes);
    else check(value.text === null);
    if (value.content_state === 'too-large') check(value.bytes > 262144);
    if (value.content_state === 'binary-or-unsafe-text') check(value.bytes <= 262144);
  }
  return { path: value.path, kind: value.kind, digest: value.digest, bytes: value.bytes, executable: value.executable, state: value.content_state, text: value.text };
}
export function fleetInputComparison(raw, pin, after = null, selected = null) {
  check(typeof raw !== 'string' || raw.length <= 4 * 1024 * 1024);
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  const selection = pin.selection, page = value?.input?.comparison;
  check(value?.schema === 'mesh.fleet-starting-comparison/v1' && value.objective === selection.objective && value.approval_authority === false
    && value.selection && Object.keys(value.selection).sort().join(',') === 'bundle,checkpoint,lane,version'
    && ['lane', 'checkpoint', 'version', 'bundle'].every(field => value.selection[field] === selection[field])
    && hex(pin.startingInput, 64) && value.input.source_version === pin.startingInput);
  return immutableFleetComparison(page, selection.version, after, selected, pin.input?.page);
}

// Shared bounded content validator; callers must first verify their distinct native envelope.
export function immutableFleetComparison(page, version, after = null, selected = null, prior = null) {
  check(page && hex(page.base, 64) && page.target === version && page.order === 'object-id'
    && page.approval_authority === false && page.after === after && page.selected === selected
    && (after === null || hex(after, 32)) && (selected === null || hex(selected, 32)) && !(after && selected)
    && count(page.total) && Array.isArray(page.changes) && page.changes.length <= 200 && page.total >= page.changes.length);
  if (prior) check(page.base === prior.base && page.total === prior.total);
  const hasCounts = page.file_total !== undefined || page.folder_total !== undefined;
  if (hasCounts) check(count(page.file_total) && count(page.folder_total)
    && page.file_total <= page.total && page.folder_total <= page.total
    && page.file_total + page.folder_total === page.total);
  const fileTotal = hasCounts ? page.file_total : null, folderTotal = hasCounts ? page.folder_total : null;
  if (prior?.fileTotal != null) check(fileTotal === prior.fileTotal && folderTotal === prior.folderTotal);
  let previous = after;
  const changes = page.changes.map(change => {
    check(hex(change?.object, 32) && (previous === null || change.object > previous)); previous = change.object;
    const before = side(change.before, selected !== null), later = side(change.after, selected !== null);
    check(before !== null || later !== null);
    const effect = !before ? 'added' : !later ? 'removed' : before.path !== later.path ? 'moved-or-modified' : 'modified';
    check(change.effect === effect);
    return { object: change.object, effect, before, after: later };
  });
  if (hasCounts) {
    const files = changes.filter(row => row.before?.kind === 'file' || row.after?.kind === 'file').length;
    check(files <= fileTotal && changes.length - files <= folderTotal);
    if (after === null && selected === null && page.next_after === null) check(files === fileTotal && changes.length - files === folderTotal);
  }
  if (selected !== null) check(changes.length === 1 && changes[0].object === selected && page.next_after === null);
  else {
    check(page.next_after === null || (changes.length === 200 && page.next_after === changes.at(-1).object && page.total > 200));
    check(after !== null || page.next_after !== null || changes.length === page.total);
  }
  return { base: page.base, target: page.target, total: page.total, fileTotal, folderTotal, after, selected, changes, nextAfter: page.next_after };
}
