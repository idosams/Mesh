const check = valid => { if (!valid) throw new Error('Review changes could not be verified'); };
const keys = (value, expected) => value && typeof value === 'object' && !Array.isArray(value) && Object.keys(value).sort().join(',') === expected;
export const reviewChangeMessage = message => typeof message === 'string' && message.trim().length > 0
  && new TextEncoder().encode(message).length <= 8192
  && !/[\u0000-\u0008\u000b-\u001f\u007f-\u009f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(message);
function parse(raw, selection, receipt) {
  check(typeof raw !== 'string' || raw.length <= 2 * 1024 * 1024);
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  check(keys(value, receipt ? 'change,objective,request,schema,selection' : 'activity,objective,schema,selection')
    && value.schema === (receipt ? 'mesh.fleet-review-change-receipt/v1' : 'mesh.fleet-review-changes/v2')
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
  check(keys(activity, 'changes,responses') && Array.isArray(activity.changes) && activity.changes.length <= 32
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
  return { rows, responses };
}
export function savedReviewChanges(raw, selection) { return savedReviewChangeActivity(raw, selection).rows; }
export function reviewChangeReceipt(raw, selection, pending) {
  const value = parse(raw, selection, true);
  check(value.request === pending.request);
  const row = entry(value.change, selection); check(row.message === pending.message); return row;
}
