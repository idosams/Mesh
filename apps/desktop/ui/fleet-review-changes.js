const check = valid => { if (!valid) throw new Error('Review changes could not be verified'); };
const keys = (value, expected) => value && typeof value === 'object' && !Array.isArray(value) && Object.keys(value).sort().join(',') === expected;
export const reviewChangeMessage = message => typeof message === 'string' && message.trim().length > 0
  && new TextEncoder().encode(message).length <= 8192
  && !/[\u0000-\u0008\u000b-\u001f\u007f-\u009f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(message);
function parse(raw, selection, receipt) {
  check(typeof raw !== 'string' || raw.length <= 2 * 1024 * 1024);
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  check(keys(value, receipt ? 'change,objective,request,schema,selection' : 'changes,objective,schema,selection')
    && value.schema === (receipt ? 'mesh.fleet-review-change-receipt/v1' : 'mesh.fleet-review-changes/v1')
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
export function savedReviewChanges(raw, selection) {
  const value = parse(raw, selection, false);
  check(Array.isArray(value.changes) && value.changes.length <= 32);
  const rows = value.changes.map(row => entry(row, selection));
  check(new Set(rows.map(row => row.id)).size === rows.length);
  return rows;
}
export function reviewChangeReceipt(raw, selection, pending) {
  const value = parse(raw, selection, true);
  check(value.request === pending.request);
  const row = entry(value.change, selection); check(row.message === pending.message); return row;
}
