import test from 'node:test';
import assert from 'node:assert/strict';
import {createRemoteProjectWorkflow,remoteProjectOutbox,remoteProjectResult} from './remote-project-workflow.js';
const hex=(n,size=64)=>n.repeat(size);
const selected={objective:`fleet-${hex('a')}`,offer:hex('b'),correlation:hex('c')};
const input=action=>({schema:'mesh.desktop-remote-project-request/v1',project:hex('d'),...selected,request:hex('e',32),expected_main:null,action});
const candidate=e=>({schema:'mesh.fleet-project-candidate/v1',candidate:`candidate-${hex('1')}`,project:e.project,content_digest:hex('2'),files:1,directories:0,bytes:5,state:'staged',approval_authority:false,
 provenance:{schema:'mesh.remote-project-candidate/v1',objective:e.objective,selection:{offer:e.offer,correlation:e.correlation,lane:'lane',run:'run',version:hex('a'),bundle:hex('b'),remote_version:hex('c')},evidence:hex('3'),correspondence:hex('4'),source_project:e.project,source_version:hex('5'),expected_main:e.expected_main,attribution:'authenticated-remote-result',approval_authority:false}});
const outcome=e=>({schema:'mesh.fleet-project-import/v1',candidate:candidate(e),receipt_digest:hex('6'),target:hex('7'),state:'imported',approval_authority:false});
const review=e=>({schema:'mesh.fleet-project-import-review/v1',import:outcome(e),expected_main:e.expected_main,base_is_current:true,approval_authority:false,review:{bundle:hex('8'),target:hex('7'),reviewed_head:hex('9'),presentation:hex('a'),complete:true,unavailable:null,changes:[],changes_not_listed:0,operations_not_listed:0,author_attribution:'unknown',approval_authority:false}});
function fixture(){
 let stored={schema:'mesh.remote-project-outbox/v1',revision:'0',entries:[]},failure=null,actions=[],saves=0;const imported=new Set();
 const invoke=async(command,args)=>{
  if(command==='load_remote_project_outbox')return structuredClone(stored);
  if(command==='save_remote_project_outbox'){
   if(failure==='save-before')throw Error('before');const next=remoteProjectOutbox(args.snapshot);assert.equal(next.revision,stored.revision);stored={...next,revision:String(BigInt(stored.revision)+1n)};saves++;
   if(failure==='save-after')throw Error('lost');return structuredClone(stored);
  }
  if(command==='remote_fleet_project_context')return {schema:'mesh.remote-project-context/v1',...args,project:hex('d'),input:hex('5'),observed_main:null,approval_authority:false};
  if(command==='remote_fleet_project'){
   const e=JSON.parse(args.selection);actions.push(e);assert.ok(stored.entries.some(v=>v.request===e.request),'must retain before dispatch');
   let result;if(e.action==='stage')result=candidate(e);else if(e.action==='import'){imported.add(e.request);result=outcome(e);}else if(e.action==='inspect_import')result=imported.has(e.request)?outcome(e):null;else result=review(e);
   if(failure==='action-after')throw Error('lost response');return {schema:'mesh.desktop-remote-project-result/v1',selection:e,result};
  }
  throw Error(`unexpected ${command}`);
 };
 return {invoke,fail:v=>failure=v,stored:()=>stored,actions,imports:()=>imported.size,saves:()=>saves};
}
const make=f=>createRemoteProjectWorkflow({invoke:f.invoke,changed(){},requestId:()=>hex('e',32)});
const state=h=>h.snapshot().remoteProjectWorkflow;
test('failed and lost persistence acknowledgment cannot dispatch or replace exact retry inputs',async()=>{
 for(const failure of ['save-before','save-after']){
  const f=fixture(),h=make(f);await h.load();f.fail(failure);await h.prepare(selected);assert.equal(f.actions.length,0);assert.equal(state(h).entries.length,1);
  await h.prepare(selected);assert.equal(state(h).entries[0].input.request,hex('e',32));f.fail(null);await h.retry(hex('e',32));assert.equal(f.actions.length,1);assert.equal(f.actions[0].action,'stage');assert.equal(f.stored().entries[0].action,'inspect_import');
 }
});
test('lost import response survives restart without replay; explicit read recovers and opens exact review',async()=>{
 const f=fixture();let h=make(f);await h.load();await h.prepare(selected);f.fail('action-after');await h.retry(hex('e',32),'import');assert.equal(f.imports(),1);assert.equal(f.stored().entries[0].action,'import');h.dispose();
 const before=f.actions.length;h=make(f);f.fail(null);await h.load();assert.equal(f.actions.length,before);await h.retry(hex('e',32),'read');assert.equal(f.actions.at(-1).action,'inspect_import');assert.equal(state(h).entries[0].outcome.state,'imported');assert.equal(f.imports(),1);
 await h.retry(hex('e',32),'review');assert.equal(state(h).entries[0].review.bundle,hex('8'));assert.equal(state(h).entries[0].review.target,hex('7'));assert.equal(f.stored().entries[0].action,'inspect_review');
 await h.forget(hex('e',32));assert.equal(f.stored().entries.length,0);assert.equal(f.imports(),1);
});
test('reopening a staged request only reads on explicit request before allowing save',async()=>{
 const f=fixture();let h=make(f);await h.load();await h.prepare(selected);h.dispose();h=make(f);await h.load();const n=f.actions.length;await h.retry(hex('e',32),'import');assert.equal(f.actions.length,n);
 await h.retry(hex('e',32),'read');assert.equal(state(h).entries[0].readConfirmed,true);await h.retry(hex('e',32),'import');assert.equal(f.imports(),1);
});
test('closed replies reject changed main, identity and approval and duplicate retry inputs',()=>{
 const e=input('stage');const valid={schema:'mesh.desktop-remote-project-result/v1',selection:e,result:candidate(e)};remoteProjectResult(valid,e);
 for(const change of [v=>v.selection.expected_main=hex('f'),v=>v.result.provenance.selection.offer=hex('f'),v=>v.result.approval_authority=true]){const v=structuredClone(valid);change(v);assert.throws(()=>remoteProjectResult(v,e));}
 assert.throws(()=>remoteProjectOutbox({schema:'mesh.remote-project-outbox/v1',revision:'0',entries:[e,{...e,action:'import'}]}));
});
test('disposal after durable save does not submit work',async()=>{
 const f=fixture();let release;const h=createRemoteProjectWorkflow({changed(){},requestId:()=>hex('e',32),invoke:async(c,a)=>{const result=await f.invoke(c,a);if(c==='save_remote_project_outbox')await new Promise(r=>release=r);return result;}});
 await h.load();const operation=h.prepare(selected);while(!release)await new Promise(r=>setImmediate(r));h.dispose();release();await operation;assert.equal(f.actions.length,0);assert.equal(f.stored().entries.length,1);
});

test('independent project imports proceed without overwriting each other and keep their own main',async()=>{
 const f=fixture();let n=0;const h=createRemoteProjectWorkflow({invoke:f.invoke,changed(){},requestId:()=>String(++n).repeat(32)});
 await h.load();await h.prepare(selected);await h.prepare({...selected,correlation:hex('f')});
 await Promise.all([h.retry('1'.repeat(32),'import'),h.retry('2'.repeat(32),'import')]);
 assert.equal(f.imports(),2);assert.equal(f.stored().entries.length,2);assert.ok(state(h).entries.every(e=>e.outcome.state==='imported'));
 assert.deepEqual(new Set(f.stored().entries.map(e=>e.correlation)),new Set([selected.correlation,hex('f')]));
});
test('a changed completed outcome is refused and the previous verified result remains visible',async()=>{
 const f=fixture();let corrupt=false;const h=createRemoteProjectWorkflow({changed(){},requestId:()=>hex('e',32),invoke:async(c,a)=>{const v=await f.invoke(c,a);if(c==='remote_fleet_project'&&corrupt&&v.result?.target)v.result.target=hex('f');return v;}});
 await h.load();await h.prepare(selected);await h.retry(hex('e',32),'import');corrupt=true;await h.retry(hex('e',32),'read');assert.equal(state(h).entries[0].outcome.target,hex('7'));assert.ok(state(h).entries[0].error);assert.equal(f.stored().entries.length,1);
});
