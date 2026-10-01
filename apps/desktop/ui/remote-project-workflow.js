import { attachedReview } from './attached-projects.js';
const check = v => { if (!v) throw new Error('Remote project inputs could not be verified'); };
const hex = (v,n=64) => typeof v === 'string' && new RegExp(`^[a-f0-9]{${n}}$`).test(v);
const keys = (v,names) => v && !Array.isArray(v) && Object.keys(v).sort().join(',') === names.split(',').sort().join(',');
const parse = raw => { check(typeof raw !== 'string' || new TextEncoder().encode(raw).length <= 8*1024*1024); return typeof raw === 'string' ? JSON.parse(raw) : raw; };
const equal = (a,b) => JSON.stringify(a) === JSON.stringify(b);
const actions = ['stage','inspect_import','import','inspect_review','create_review'];
export function remoteProjectInput(v) {
  check(keys(v,'schema,project,objective,offer,correlation,request,expected_main,action') && v.schema === 'mesh.desktop-remote-project-request/v1'
    && hex(v.project) && /^fleet-[a-f0-9]{64}$/.test(v.objective) && hex(v.offer) && hex(v.correlation) && hex(v.request,32)
    && (v.expected_main === null || hex(v.expected_main)) && actions.includes(v.action));
  return Object.fromEntries(['schema','project','objective','offer','correlation','request','expected_main','action'].map(k=>[k,v[k]]));
}
export function remoteProjectOutbox(raw) {
  check(typeof raw !== 'string' || new TextEncoder().encode(raw).length <= 131072);
  const v=parse(raw);
  check(keys(v,'schema,revision,entries') && v.schema==='mesh.remote-project-outbox/v1' && /^(0|[1-9][0-9]{0,19})$/.test(v.revision)
    && typeof v.revision==='string' && BigInt(v.revision)<=18446744073709551615n && Array.isArray(v.entries) && v.entries.length<=8);
  const seen=new Set(); const entries=v.entries.map(e=>{const next=remoteProjectInput(e);check(!seen.has(next.request));seen.add(next.request);return next;});
  return {schema:v.schema,revision:v.revision,entries};
}
function context(raw,s) {
  const v=parse(raw);check(keys(v,'schema,objective,offer,correlation,project,input,observed_main,approval_authority') && v.schema==='mesh.remote-project-context/v1'
    && ['objective','offer','correlation'].every(k=>v[k]===s[k]) && hex(v.project) && hex(v.input) && v.approval_authority===false);
  check(v.observed_main===null || (keys(v.observed_main,'head,bundle,target') && ['head','bundle','target'].every(k=>hex(v.observed_main[k]))));
  return v;
}
function candidate(v,e) {
  check(keys(v,'schema,candidate,project,provenance,content_digest,files,directories,bytes,state,approval_authority') && v.schema==='mesh.fleet-project-candidate/v1'
    && /^candidate-[a-f0-9]{64}$/.test(v.candidate) && v.project===e.project && hex(v.content_digest) && v.state==='staged' && v.approval_authority===false
    && ['files','directories','bytes'].every(k=>Number.isSafeInteger(v[k])&&v[k]>=0));
  const p=v.provenance;check(keys(p,'schema,objective,selection,evidence,correspondence,source_project,source_version,expected_main,attribution,approval_authority')
    && p.schema==='mesh.remote-project-candidate/v1' && p.objective===e.objective && p.source_project===e.project && p.expected_main===e.expected_main
    && p.selection?.offer===e.offer && p.selection?.correlation===e.correlation && hex(p.evidence) && hex(p.correspondence) && hex(p.source_version)
    && p.attribution==='authenticated-remote-result' && p.approval_authority===false);
  check(keys(p.selection,'offer,correlation,lane,run,version,bundle,remote_version') && ['offer','correlation','version','bundle','remote_version'].every(k=>hex(p.selection[k]))
    && ['lane','run'].every(k=>typeof p.selection[k]==='string' && /^[A-Za-z0-9_.:-]{1,128}$/.test(p.selection[k])));
  return v;
}
function imported(v,e) {
  if(v===null)return null;
  check(keys(v,'schema,candidate,receipt_digest,target,state,approval_authority') && v.schema==='mesh.fleet-project-import/v1' && hex(v.receipt_digest) && hex(v.target)
    && ['pending','imported'].includes(v.state) && v.approval_authority===false);candidate(v.candidate,e);return v;
}
export function remoteProjectResult(raw,e) {
  const v=parse(raw);check(keys(v,'schema,selection,result') && v.schema==='mesh.desktop-remote-project-result/v1' && equal(remoteProjectInput(v.selection),e));
  if(e.action==='stage')return {candidate:candidate(v.result,e),outcome:null,review:null};
  if(['import','inspect_import'].includes(e.action))return {outcome:imported(v.result,e)};
  const r=v.result;check(keys(r,'schema,import,review,expected_main,base_is_current,approval_authority') && r.schema==='mesh.fleet-project-import-review/v1'
    && r.expected_main===e.expected_main && typeof r.base_is_current==='boolean' && r.approval_authority===false);
  const outcome=imported(r.import,e);check(outcome?.state==='imported');
  return {outcome,baseIsCurrent:r.base_is_current,review:r.review===null?null:attachedReview({schema:'mesh.desktop-attachment-review/v1',project:e.project,review:r.review},e.project,outcome.target)};
}
const sameSelection=(a,b)=>['objective','offer','correlation'].every(k=>a[k]===b[k]);
export function createRemoteProjectWorkflow({invoke,changed,requestId=()=>globalThis.crypto.randomUUID().replaceAll('-','')}) {
  let entries=[],states={},loaded=false,error='',disposed=false,active=Promise.resolve(),preparing=false;
  const publish=()=>{if(!disposed)changed();};
  const serial=fn=>{const next=active.catch(()=>{}).then(fn);active=next;return next;};
  async function read() {const saved=remoteProjectOutbox(await invoke('load_remote_project_outbox'));if(disposed)throw new Error('Closed');entries=saved.entries;loaded=true;return saved;}
  async function retain(entry,previous=null) {
    return serial(async()=>{
      check(!disposed);const stored=await read();const prior=stored.entries.find(v=>v.request===entry.request);
      if(prior && equal(prior,entry))return;
      if(previous)check(prior && equal(prior,previous));else check(!prior && !stored.entries.some(v=>sameSelection(v,entry)));
      const next=stored.entries.filter(v=>v.request!==entry.request).concat(entry);check(next.length<=8);
      const saved=remoteProjectOutbox(await invoke('save_remote_project_outbox',{snapshot:JSON.stringify({...stored,entries:next})}));
      check(equal(saved.entries,next) && BigInt(saved.revision)===BigInt(stored.revision)+1n);check(!disposed);entries=saved.entries;
    });
  }
  async function execute(entry, retained=entry) {
    if(disposed || states[entry.request]?.busy)return;
    states={...states,[entry.request]:{...states[entry.request],input:entry,busy:true,error:''}};publish();
    try {
      await retain(retained,entries.find(v=>v.request===entry.request)??null);check(!disposed);
      const result=remoteProjectResult(await invoke('remote_fleet_project',{selection:JSON.stringify(entry)}),entry);check(!disposed);
      const prior=states[entry.request]?.outcome;
      if(prior && 'outcome' in result)check(result.outcome && result.outcome.target===prior.target && result.outcome.receipt_digest===prior.receipt_digest && !(prior.state==='imported'&&result.outcome.state!=='imported'));
      states={...states,[entry.request]:{...states[entry.request],...result,readConfirmed:entry.action==='inspect_import'||states[entry.request]?.readConfirmed}};
      const action=entry.action==='stage'||entry.action==='import'?'inspect_import':entry.action==='create_review'?'inspect_review':entry.action;
      if(action!==entry.action)await retain({...entry,action},entry);
    } catch {if(!disposed)states={...states,[entry.request]:{...states[entry.request],error:'This exact project action could not be confirmed. Its retry inputs are retained; retry or read its status.'}};}
    finally {if(!disposed){states={...states,[entry.request]:{...states[entry.request],busy:false}};publish();}}
  }
  return {
    snapshot:()=>({remoteProjectWorkflow:{entries:entries.map(e=>({...states[e.request],input:e})).concat(Object.values(states).filter(s=>s.input&&!entries.some(e=>e.request===s.input.request))),loaded,error,preparing}}),
    async load(){if(disposed||typeof invoke!=='function')return;try{await serial(read);error='';}catch{error='Pending remote project actions could not be loaded. Retry loading before continuing.';}publish();},
    async prepare(selection){
      if(disposed||preparing||!loaded||error)return;
      if(entries.some(e=>sameSelection(e,selection))||Object.values(states).some(s=>s.input&&sameSelection(s.input,selection))){publish();return;}
      if(entries.length>=8){error='Finish or remove a pending project action before preparing another.';publish();return;}
      preparing=true;publish();
      try {const c=context(await invoke('remote_fleet_project_context',Object.fromEntries(['objective','offer','correlation'].map(k=>[k,selection[k]]))),selection);check(!disposed);
        const e=remoteProjectInput({schema:'mesh.desktop-remote-project-request/v1',project:c.project,objective:c.objective,offer:c.offer,correlation:c.correlation,request:requestId(),expected_main:c.observed_main?.head??null,action:'stage'});
        // Keep even an unacknowledged initial save in memory; execution always retries persistence first.
        states={...states,[e.request]:{input:e,busy:false,error:''}};await execute(e);
      }catch{if(!disposed)error='The original project could not be verified. Refresh before preparing this result.';}
      finally{preparing=false;publish();}
    },
    async retry(request,mode='retry') {
      if(disposed||states[request]?.busy||!loaded)return;
      const entry=entries.find(e=>e.request===request)??states[request]?.input;if(!entry)return;
      const action=mode==='retry'?entry.action:mode==='read'?'inspect_import':mode==='import'?'import':mode==='review'?'create_review':null;
      if(!action)return;
      if(mode==='import'&&!states[request]?.readConfirmed&&!states[request]?.candidate&&!['pending','imported'].includes(states[request]?.outcome?.state))return;
      if(mode==='review'&&states[request]?.outcome?.state!=='imported')return;
      await execute({...entry,action},mode==='read'?entry:{...entry,action});
    },
    async forget(request){if(disposed||states[request]?.busy)return;try{await serial(async()=>{const old=await read();const next=old.entries.filter(e=>e.request!==request);if(next.length===old.entries.length)return;
      const saved=remoteProjectOutbox(await invoke('save_remote_project_outbox',{snapshot:JSON.stringify({...old,entries:next})}));check(equal(saved.entries,next)&&BigInt(saved.revision)===BigInt(old.revision)+1n);entries=saved.entries;});delete states[request];error='';}catch{error='Retry inputs could not be removed. Existing work is retained.';}publish();},
    dispose(){disposed=true;},
  };
}
