// Public inputs/projections only. Private paths and request identities are native-owned.
const require = condition => { if (!condition) throw new Error('Remote creation could not be verified'); };
const hex = (s,n=64) => typeof s === 'string' && new RegExp(`^[a-f0-9]{${n}}$`).test(s);
const parse = raw => typeof raw === 'string' ? JSON.parse(raw) : raw;
const text = s => typeof s === 'string' && s.trim().length > 0 && new TextEncoder().encode(s).length <= 8192 && !/[\u0000\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(s);
const peerName = s => typeof s === 'string' && /^[A-Za-z0-9][A-Za-z0-9._-]{0,252}$/.test(s);
function limits(value) {
  require(value && ['lanes','concurrency','depth','retries'].every(k=>Number.isSafeInteger(value[k]))
    && value.lanes >= 1 && value.lanes <= 1024 && value.concurrency >= 1 && value.concurrency <= Math.min(64,value.lanes)
    && value.depth >= 0 && value.depth <= 32 && value.retries === 0);
  return Object.fromEntries(['lanes','concurrency','depth','retries'].map(k=>[k,value[k]]));
}
export function creationInput(value) {
  const c=value?.connection;
  require(c && peerName(c.host) && peerName(c.account) && c.account.length <= 64 && hex(c.worker)
    && /^[1-9][0-9]{0,4}$/.test(String(c.port)) && Number(c.port) <= 65535
    && hex(value.project) && hex(value.version) && text(value.goal) && ['codex','claude'].includes(value.provider));
  return {connection:{host:c.host,account:c.account,port:Number(c.port),worker:c.worker},project:value.project,version:value.version,goal:value.goal,provider:value.provider,limits:limits(value.limits)};
}
export function creationEntry(value) {
  require(hex(value?.request,32) && hex(value.worker) && hex(value.project) && hex(value.version) && text(value.goal)
    && peerName(value.host) && ['codex','claude'].includes(value.provider)
    && typeof value.lease_until_ms === 'string' && /^[1-9][0-9]{0,19}$/.test(value.lease_until_ms)
    && BigInt(value.lease_until_ms) <= 18446744073709551615n);
  return Object.freeze({...Object.fromEntries(['request','project','version','goal','provider','host','worker','lease_until_ms'].map(k=>[k,value[k]])),limits:Object.freeze(limits(value.limits))});
}
export function creationList(raw) {
  const value=parse(raw);require(value?.schema==='mesh.remote-creation-list/v1' && Array.isArray(value.entries) && value.entries.length<=64);
  const entries=value.entries.map(creationEntry);require(new Set(entries.map(e=>e.request)).size===entries.length);return entries;
}
export function creationPrepared(raw,draft,input) {
  const value=parse(raw);require(value?.schema==='mesh.remote-creation-prepared/v1' && value.draft===draft && value.entry?.request===draft.slice(0,32));
  const entry=creationEntry(value.entry);
  require(['project','version','goal','provider'].every(k=>entry[k]===input[k]) && entry.host===input.connection.host && entry.worker===input.connection.worker && JSON.stringify(entry.limits)===JSON.stringify(input.limits));return entry;
}
export function creationOutcome(raw,action,request) {
  const value=parse(raw);require(value?.request===request);
  if(action==='send') {require(value.schema==='mesh.remote-creation-sent/v1' && ['input-materialized','input-retained'].includes(value.disposition)); return {request,kind:'sent',disposition:value.disposition};}
  require(value.schema==='mesh.remote-creation-inspected/v1' && typeof value.allocated==='boolean' && (value.allocated || value.selection===null));
  return {request,kind:value.allocated?'allocated':'ready',selection:value.selection};
}
