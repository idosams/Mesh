// Presentation validation only. Native history owns file identities, content and main authority.
const check = value => { if (!value) throw new Error('Fixed project comparison could not be verified'); };
const hex = (value, size = 64) => typeof value === 'string' && new RegExp(`^[a-f0-9]{${size}}$`).test(value);
const id = value => typeof value === 'string' && /^[A-Za-z0-9_.:-]{1,128}$/.test(value);
const count = value => Number.isSafeInteger(value) && value >= 0;
const keys = (value, fields) => value && !Array.isArray(value) && Object.keys(value).sort().join(',') === fields.split(',').sort().join(',');
const safe = value => typeof value === 'string' && !/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(value);
export const candidatePath = value => safe(value) && value.length > 0 && new TextEncoder().encode(value).length <= 4096 && !/[\r\n\t]/u.test(value) && !value.startsWith('/') && !value.split('/').some(part => ['', '.', '..'].includes(part));
const parse = raw => { check(typeof raw !== 'string' || raw.length <= 8 * 1024 * 1024); return typeof raw === 'string' ? JSON.parse(raw) : raw; };
const selectionMatches = (value, selection) => keys(value, 'lane,checkpoint,version,bundle') && ['lane', 'checkpoint', 'version', 'bundle'].every(key => value[key] === selection[key]);
function main(value) {
  check(value === null || (keys(value, 'head,bundle,target') && [value.head, value.bundle, value.target].every(value => hex(value))));
  return value;
}
export function projectMappingMain(raw, pin, project) {
  const value = parse(raw), mapping = value?.mapping;
  check(keys(value, 'schema,objective,selection,mapping,approval_authority') && value.schema === 'mesh.fleet-project-mapping/v2'
    && value.objective === pin.selection.objective && selectionMatches(value.selection, pin.selection) && value.approval_authority === false
    && mapping?.source_project === project && hex(project) && mapping.scope === 'recorded-input-ancestry');
  // Only the native verified main observation is retained. Correspondence is read separately.
  return main(mapping.observed_main)?.head ?? null;
}
function envelope(raw, pin, pending, schema) {
  const value = parse(raw);
  check(keys(value, 'schema,project,objective,selection,request,expected_main,result') && value.schema === schema
    && value.project === pending.project && value.objective === pin.selection.objective && selectionMatches(value.selection, pin.selection)
    && value.request === pending.request && value.expected_main === pending.expected_main);
  return value.result;
}
function provenance(value, pin, pending) {
  check(keys(value, 'schema,objective,selection,source_project,source_version,lineage,expected_main,origin,attribution,approval_authority')
    && value.schema === 'mesh.fleet-candidate-provenance/v1' && value.objective === pin.selection.objective
    && selectionMatches(value.selection, pin.selection) && value.source_project === pending.project && hex(value.source_version)
    && value.expected_main === pending.expected_main && value.approval_authority === false && value.attribution === 'recorded-agent-checkpoint'
    && keys(value.origin, 'actor,session,run,generation') && Object.values(value.origin).every(id)
    && Array.isArray(value.lineage) && value.lineage.length > 0 && value.lineage.length <= 33);
  const lanes = new Set(); let input = value.source_version;
  for (const step of value.lineage) {
    check(keys(step, 'lane,source_version,starting_version,result_version') && id(step.lane) && !lanes.has(step.lane)
      && [step.source_version, step.starting_version, step.result_version].every(value => hex(value)) && step.source_version === input);
    lanes.add(step.lane); input = step.result_version;
  }
  const leaf = value.lineage.at(-1);
  check(leaf.lane === pin.selection.lane && leaf.result_version === pin.selection.version && leaf.source_version === pin.startingInput);
}
export function projectCandidateReceipt(raw, pin, pending) {
  const value = envelope(raw, pin, pending, 'mesh.desktop-fleet-candidate/v1');
  check(keys(value, 'schema,candidate,project,provenance,content_digest,files,directories,bytes,state,approval_authority')
    && value.schema === 'mesh.fleet-project-candidate/v1' && /^candidate-[a-f0-9]{64}$/.test(value.candidate)
    && value.project === pending.project && hex(value.content_digest) && [value.files, value.directories, value.bytes].every(count)
    && value.state === 'staged' && value.approval_authority === false);
  provenance(value.provenance, pin, pending);
  return { candidate: value.candidate, contentDigest: value.content_digest };
}
function side(value, path, selected) {
  if (value === null) return null;
  check(keys(value, 'entry,content_state,text') && keys(value.entry, 'kind,bytes,digest,executable'));
  const entry = value.entry, state = value.content_state;
  if (entry.kind === 'folder') check(entry.bytes === null && entry.digest === null && entry.executable === null && state === 'folder' && value.text === null);
  else {
    check(entry.kind === 'file' && count(entry.bytes) && hex(entry.digest) && typeof entry.executable === 'boolean'
      && (selected ? ['text', 'too-large', 'binary-or-unsafe-text'] : ['not-requested']).includes(state));
    if (state === 'text') check(safe(value.text) && value.text.length <= 262144 && entry.bytes <= 262144 && new TextEncoder().encode(value.text).length === entry.bytes);
    else check(value.text === null);
    if (state === 'too-large') check(entry.bytes > 262144);
    if (state === 'binary-or-unsafe-text') check(entry.bytes <= 262144);
  }
  return { ...entry, path, state, text: value.text };
}
function precedes(left, right) {
  const a = [...left], b = [...right];
  for (let i = 0; i < Math.min(a.length, b.length); i++) if (a[i] !== b[i]) return a[i].codePointAt(0) < b[i].codePointAt(0);
  return a.length < b.length;
}
export function projectCandidateReview(raw, pin, pending, after = null, selected = null, prior = null, receipt = null) {
  const value = envelope(raw, pin, pending, 'mesh.desktop-fleet-candidate-review/v1'), context = value?.context, page = value?.comparison;
  check(keys(value, 'schema,review,context,observed_main,provenance,base_is_current,comparison,approval_authority')
    && value.schema === 'mesh.fleet-project-candidate-review/v1' && hex(value.review) && value.approval_authority === false
    && keys(context, 'schema,scope,project,candidate,candidate_receipt_digest,base_head,base_version,target_version,content_digest')
    && context.schema === 'mesh.fleet-project-review-context/v1' && context.scope === 'whole-project-snapshot'
    && context.project === pending.project && /^candidate-[a-f0-9]{64}$/.test(context.candidate)
    && hex(context.candidate_receipt_digest) && hex(context.content_digest) && context.base_head === pending.expected_main
    && (pending.expected_main === null ? context.base_version === null : hex(context.base_version)) && context.target_version === pin.selection.version);
  provenance(value.provenance, pin, pending);
  const observedMain = main(value.observed_main);
  check(value.base_is_current === ((observedMain?.head ?? null) === pending.expected_main));
  if (receipt) check(receipt.candidate === context.candidate && receipt.contentDigest === context.content_digest);
  if (prior) check(value.review === prior.review && Object.keys(context).every(key => context[key] === prior.context[key]));
  check(keys(page, 'order,after,selected,total,changes,next_after') && page.order === 'path' && page.after === after && page.selected === selected
    && (after === null || candidatePath(after)) && (selected === null || candidatePath(selected)) && !(after && selected)
    && count(page.total) && Array.isArray(page.changes) && page.changes.length <= 200 && page.total >= page.changes.length);
  if (prior) check(page.total === prior.total);
  let previous = after;
  const changes = page.changes.map(change => {
    check(keys(change, 'path,change,before,after') && candidatePath(change.path) && (previous === null || precedes(previous, change.path))); previous = change.path;
    const before = side(change.before, change.path, selected !== null), later = side(change.after, change.path, selected !== null);
    check(before !== null || later !== null);
    const effect = !before ? 'added' : !later ? 'removed' : before.kind !== later.kind ? 'type-changed'
      : before.digest === later.digest && before.bytes === later.bytes && before.executable !== later.executable ? 'mode-changed' : 'modified';
    check(effect === change.change);
    return { path: change.path, effect, before, after: later };
  });
  if (selected !== null) check(changes.length === 1 && changes[0].path === selected && page.next_after === null);
  else {
    check(page.next_after === null || (changes.length === 200 && page.next_after === changes.at(-1).path && page.total > 200));
    check(after !== null || page.next_after !== null || changes.length === page.total);
  }
  return { review: value.review, context: { ...context }, baseIsCurrent: value.base_is_current, observedMain,
    total: page.total, after, selected, changes, nextAfter: page.next_after };
}
