const check = valid => { if (!valid) throw new Error('Review changes could not be verified'); };
const keys = (value, expected) => value && typeof value === 'object' && !Array.isArray(value) && Object.keys(value).sort().join(',') === expected;
export const reviewChangeMessage = message => typeof message === 'string' && message.trim().length > 0
  && new TextEncoder().encode(message).length <= 8192
  && !/[\u0000-\u0008\u000b-\u001f\u007f-\u009f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(message);
function parse(raw, selection, receipt) {
  check(typeof raw !== 'string' || raw.length <= 2 * 1024 * 1024);
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  check(keys(value, receipt ? 'change,objective,request,schema,selection' : 'activity,objective,schema,selection')
    && value.schema === (receipt ? 'mesh.fleet-review-change-receipt/v1' : 'mesh.fleet-review-changes/v3')
    && value.objective === selection.objective && keys(value.selection, 'bundle,checkpoint,lane,version')
    && ['lane', 'checkpoint', 'version', 'bundle'].every(field => value.selection[field] === selection[field]));
  return value;
}
function entry(value, selection) {
  check(keys(value, 'approval_authority,bundle,checkpoint,id,lane,message,status,version')
    && typeof value.id === 'string' && /^review-change-[a-f0-9]{64}$/.test(value.id)
    && ['lane', 'checkpoint', 'version', 'bundle'].every(field => value[field] === selection[field])
    && reviewChangeMessage(value.message) && value.status === 'recorded' && value.approval_authority === false);
  return value;
}
export function savedReviewChangeActivity(raw, selection) {
  const value = parse(raw, selection, false), activity = value.activity;
  check(keys(activity, 'changes,decisions,responses') && Array.isArray(activity.changes) && activity.changes.length <= 32
    && Array.isArray(activity.responses) && activity.responses.length <= 256);
  const rows = activity.changes.map(row => entry(row, selection));
  check(new Set(rows.map(row => row.id)).size === rows.length);
  const seen = new Set(), counts = new Map();
  const responses = activity.responses.map(response => {
    check(keys(response, 'approval_authority,bundle,checkpoint,lane,request,status,version')
      && rows.some(row => row.id === response.request) && response.lane === selection.lane
      && typeof response.checkpoint === 'string' && /^[A-Za-z0-9_.:-]{1,128}$/.test(response.checkpoint)
      && response.checkpoint !== selection.checkpoint && typeof response.version === 'string' && /^[a-f0-9]{64}$/.test(response.version)
      && response.version !== selection.version && typeof response.bundle === 'string' && /^[a-f0-9]{64}$/.test(response.bundle)
      && response.status === 'proposed' && response.approval_authority === false);
    const id = `${response.request}/${response.checkpoint}`;
    const count = (counts.get(response.request) ?? 0) + 1;
    check(!seen.has(id) && count <= 8); seen.add(id); counts.set(response.request, count);
    return response;
  });
  check(Array.isArray(activity.decisions) && activity.decisions.length === rows.length);
  const decisions = activity.decisions.map(value => reviewDecision(value, value?.request));
  check(new Set(decisions.map(value => value.request)).size === rows.length
    && decisions.every(value => rows.some(row => row.id === value.request)
      && (value.checkpoint === null || responses.some(response => response.request === value.request
        && response.checkpoint === value.checkpoint && response.version === value.version && response.bundle === value.bundle))));
  return { rows, responses, decisions };
}
export function savedReviewChanges(raw, selection) { return savedReviewChangeActivity(raw, selection).rows; }
export function reviewChangeReceipt(raw, selection, pending) {
  const value = parse(raw, selection, true);
  check(value.request === pending.request);
  const row = entry(value.change, selection); check(row.message === pending.message); return row;
}

function reviewDecision(value, request) {
  check(keys(value, 'approval_authority,bundle,checkpoint,request,revision,status,version')
    && typeof request === 'string' && /^review-change-[a-f0-9]{64}$/.test(request) && value.request === request
    && Number.isSafeInteger(value.revision) && value.revision >= 0 && value.revision <= 64
    && value.approval_authority === false);
  if (value.status === 'open') check(value.checkpoint === null && value.version === null && value.bundle === null);
  else check(value.status === 'addressed' && value.revision > 0 && typeof value.checkpoint === 'string'
    && /^[A-Za-z0-9_.:-]{1,128}$/.test(value.checkpoint) && typeof value.version === 'string' && /^[a-f0-9]{64}$/.test(value.version)
    && typeof value.bundle === 'string' && /^[a-f0-9]{64}$/.test(value.bundle));
  return value;
}
export function reviewDecisionReceipt(raw, selection, pending) {
  check(typeof raw !== 'string' || raw.length <= 16384);
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  check(keys(value, 'objective,operation,outcome,request,schema,selection')
    && value.schema === 'mesh.fleet-review-decision/v1' && value.objective === selection.objective
    && keys(value.selection, 'bundle,checkpoint,lane,version')
    && ['lane', 'checkpoint', 'version', 'bundle'].every(field => value.selection[field] === selection[field])
    && value.operation === pending.operation && value.request === pending.request
    && keys(value.outcome, 'cancelled,current,receipt') && typeof value.outcome.cancelled === 'boolean');
  const current = reviewDecision(value.outcome.current, pending.request);
  check(current.revision >= pending.expectedRevision);
  if (value.outcome.cancelled) check(value.outcome.receipt === null);
  else {
    const receipt = reviewDecision(value.outcome.receipt, pending.request);
    check(receipt.revision === pending.expectedRevision + 1 && receipt.checkpoint === pending.proposedCheckpoint
      && receipt.version === pending.version && receipt.bundle === pending.bundle && current.revision >= receipt.revision
      && (current.revision !== receipt.revision || (current.checkpoint === receipt.checkpoint && current.version === receipt.version && current.bundle === receipt.bundle)));
  }
  return { current, cancelled: value.outcome.cancelled };
}
