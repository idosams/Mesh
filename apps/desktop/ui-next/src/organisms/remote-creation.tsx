import { useState } from "react";
import { Button } from "../atoms/button";
import { useTranslation } from "../lib/localization";
export type CreationSource = {id:string;root:string;savedVersion:string|null;detached?:boolean};
export type CreationHistories = Record<string,{versions:string[]}>;
export type CreationEntry = {request:string;project:string;version:string;goal:string;provider:string;host:string;worker:string;lease_until_ms:string;limits:{lanes:number;concurrency:number;depth:number;retries:number}};
export type CreationStatus = {request:string;kind:string;disposition?:string};
const send=(detail:Record<string,unknown>)=>document.dispatchEvent(new CustomEvent("mesh:remote-observation-intent",{detail}));
export function RemoteCreationForm({connection,ready,disabled,projects,histories}:{connection:{host:string;account:string;port:string;worker:string};ready:boolean;disabled:boolean;projects:CreationSource[];histories:CreationHistories}) {
  const t=useTranslation();const [project,setProject]=useState("");const [version,setVersion]=useState("");const [goal,setGoal]=useState("");const [provider,setProvider]=useState("codex");
  const [lanes,setLanes]=useState("4"),[concurrency,setConcurrency]=useState("2"),[depth,setDepth]=useState("1");
  const source=projects.find(p=>p.id===project&&!p.detached);
  const versions=[...new Set([...(source?.savedVersion?[source.savedVersion]:[]),...(histories[project]?.versions??[])])];
  const numbers=[lanes,concurrency,depth].every(s=>/^(0|[1-9][0-9]{0,3})$/.test(s))&&Number(lanes)>=1&&Number(lanes)<=1024&&Number(concurrency)>=1&&Number(concurrency)<=Math.min(64,Number(lanes))&&Number(depth)<=32;
  const valid=ready&&source&&versions.includes(version)&&goal.trim().length>0&&new TextEncoder().encode(goal).length<=8192&&numbers;
  return <section className="grid gap-3 rounded border p-3" aria-label={t("Prepare new remote work")}><h4 className="font-medium">{t("Prepare new remote work")}</h4>
    <p className="text-sm">{t("Choose saved work and retain its exact request before sending input. No existing attempt is needed. The fixed lease lasts fifteen minutes from preparation.")}</p>
    <label className="grid gap-1">{t("Project")}<select className="min-h-11 rounded border bg-background p-2" disabled={disabled} value={project} onChange={e=>{setProject(e.target.value);setVersion("");}}><option value="">{t("Choose project")}</option>{projects.filter(p=>!p.detached&&p.savedVersion).map(p=><option key={p.id} value={p.id}>{p.root}</option>)}</select></label>
    <label className="grid gap-1">{t("Starting saved version")}<select className="min-h-11 rounded border bg-background p-2" dir="ltr" disabled={disabled||!source} value={version} onChange={e=>setVersion(e.target.value)}><option value="">{t("Choose exact saved version")}</option>{versions.map(v=><option key={v} value={v}>{v}</option>)}</select></label>
    <label className="grid gap-1">{t("What should the agents accomplish?")}<textarea className="min-h-24 rounded border bg-background p-2" dir="auto" disabled={disabled} value={goal} maxLength={8192} onChange={e=>setGoal(e.target.value)}/></label>
    <label className="grid gap-1">{t("Coordinator provider")}<select className="min-h-11 rounded border bg-background p-2" disabled={disabled} value={provider} onChange={e=>setProvider(e.target.value)}><option value="codex">Codex</option><option value="claude">Claude</option></select></label>
    <div className="grid gap-2 sm:grid-cols-3">{[{label:"Maximum lanes, including coordinator",value:lanes,set:setLanes,max:1024,min:1},{label:"Agents running at once",value:concurrency,set:setConcurrency,max:64,min:1},{label:"Delegation depth",value:depth,set:setDepth,max:32,min:0}].map(f=><label key={f.label} className="grid gap-1">{t(f.label)}<input className="min-h-11 rounded border bg-background p-2" type="number" disabled={disabled} value={f.value} min={f.min} max={f.max} onChange={e=>f.set(e.target.value)}/></label>)}</div>
    <Button disabled={disabled||!valid} onClick={()=>send({type:"creation-prepare",input:{connection,project,version,goal,provider,limits:{lanes:Number(lanes),concurrency:Number(concurrency),depth:Number(depth),retries:0}}})}>{t("Save remote creation request")}</Button>
    <p className="text-xs">{t("To prepare a different request, clear and reselect the native setup files. Existing saved requests remain available.")}</p>
  </section>;
}
export function RemoteCreationRequests({entries,status,disabled}:{entries:CreationEntry[]|null;status:CreationStatus|null;disabled:boolean}) {
  const t=useTranslation();return <section className="grid gap-3 rounded border p-3" aria-label={t("Saved remote creation requests")}><h4 className="font-medium">{t("Saved remote creation requests")}</h4>
    <p className="text-sm">{t("Sending input authorizes the configured worker to begin this attempt. A delivery receipt alone does not prove the agent started successfully.")}</p>
    <Button variant="secondary" disabled={disabled} onClick={()=>send({type:"creation-list"})}>{t("Load saved requests")}</Button>
    {entries?.map(e=><article key={e.request} className="grid gap-2 border-t pt-2"><p dir="auto" className="whitespace-pre-wrap">{e.goal}</p><p><bdi dir="ltr">{e.host}</bdi> · <bdi dir="ltr">{e.provider}</bdi></p><details className="break-all text-xs"><summary>{t("Request")}</summary><p><bdi dir="ltr">{e.request}</bdi></p><p>{t("Worker identity")}: <bdi dir="ltr">{e.worker}</bdi></p></details><p className="break-all">{t("Version")}: <bdi dir="ltr">{e.version}</bdi></p><p>{e.limits.lanes} {t("lanes")} · {e.limits.concurrency} {t("agents at once")} · {t("depth")} {e.limits.depth}</p><p>{t("Lease ends at")}: <bdi dir="ltr">{Number(e.lease_until_ms)<=8640000000000000?new Date(Number(e.lease_until_ms)).toISOString():e.lease_until_ms}</bdi></p>
      <Button variant="secondary" disabled={disabled} onClick={()=>send({type:"creation-inspect",request:e.request})}>{t("Inspect original creation attempt")}</Button>
      <Button disabled={disabled||status?.request!==e.request||!["prepared","ready"].includes(status.kind)||Number(e.lease_until_ms)<=Date.now()} onClick={()=>send({type:"creation-send",request:e.request})}>{t("Send saved input to worker")}</Button>
      {status?.request===e.request&&<p role="status">{t(status.kind==="sent"?"Input delivery was confirmed. This does not establish that an agent is running.":status.kind==="allocated"?"A retained fleet exists. Inspect its selected attempt and resume only the original input transfer if needed.":"The request is retained. Sending input is a separate explicit action.")}</p>}
    </article>)}
    {entries?.length===0&&<p>{t("No saved remote creation requests were returned.")}</p>}
  </section>;
}
