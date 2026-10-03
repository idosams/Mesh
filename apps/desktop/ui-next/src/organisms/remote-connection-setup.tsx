import { useState } from "react";
import { Button } from "../atoms/button";
import { useTranslation } from "../lib/localization";
export type RemoteSetupDraft = { id: string; installation: boolean; identity: boolean; hosts: boolean };
export type RemoteSetupFleet = { objective: string; ownership: string; lanes: { id: string; run: { id: string; state: string } | null }[] };
const send = (detail: Record<string, unknown>) => document.dispatchEvent(new CustomEvent("mesh:remote-observation-intent", { detail }));
export function RemoteConnectionSetup({ draft, fleets, disabled }: { draft: RemoteSetupDraft | null; fleets: RemoteSetupFleet[]; disabled: boolean }) {
  const t = useTranslation();
  const [input, setInput] = useState({ host: "", account: "", port: "22", worker: "", objective: "", lane: "", run: "" });
  const fleet = fleets.find(item => item.objective === input.objective && item.ownership !== "unavailable");
  const lane = fleet?.lanes.find(item => item.id === input.lane);
  const valid = [input.host, input.account].every(value => /^[A-Za-z0-9][A-Za-z0-9._-]{0,252}$/.test(value)) && input.account.length <= 64
    && /^[1-9][0-9]{0,4}$/.test(input.port) && Number(input.port) <= 65535 && /^[a-f0-9]{64}$/.test(input.worker)
    && Boolean(lane?.run && lane.run.id === input.run) && draft?.installation && draft?.identity && draft?.hosts;
  return <details className="rounded border p-3"><summary className="cursor-pointer font-medium">{t("Set up a worker connection")}</summary><div className="mt-3 grid gap-3">
    <p className="text-sm">{t("Use an existing coordinator identity and SSH files. Mesh does not create keys or enroll host trust. Settings apply to this session only.")}</p>
    <div className="grid gap-2">{([{ part: "installation", label: "Choose coordinator identity folder" }, { part: "identity", label: "Choose private SSH identity" }, { part: "hosts", label: "Choose trusted hosts file" }] as const).map(({ part, label }) => <div className="flex flex-wrap items-center gap-2" key={part}><Button variant="secondary" disabled={disabled} onClick={() => send({ type: "pick-setup", part })}>{t(label)}</Button><span role="status">{t(draft?.[part] ? "Selected" : "Not selected")}</span></div>)}</div>
    <Button variant="secondary" disabled={disabled} onClick={() => send({ type: "clear-setup" })}>{t("Clear selected setup files")}</Button>
    <div className="grid gap-3 sm:grid-cols-2">{([{ key: "host", label: "Worker address", max: 253 }, { key: "account", label: "Connection account", max: 64 }, { key: "port", label: "Connection port", max: 5 }, { key: "worker", label: "Worker public identity", max: 64 }] as const).map(({ key, label, max }) => <label className="grid gap-1 text-sm" key={key}>{t(label)}<input dir="ltr" className="min-h-11 rounded border bg-background p-2" autoComplete="off" maxLength={max} disabled={disabled} value={input[key]} onChange={event => setInput({ ...input, [key]: event.target.value })} /></label>)}</div>
    <label className="grid gap-1 text-sm">{t("Fleet")}<select dir="ltr" className="min-h-11 rounded border bg-background p-2" disabled={disabled} value={input.objective} onChange={event => setInput({ ...input, objective: event.target.value, lane: "", run: "" })}><option value="">{t("Choose fleet")}</option>{fleets.filter(item => item.ownership !== "unavailable").map(item => <option key={item.objective} value={item.objective}>{item.objective}</option>)}</select></label>
    <label className="grid gap-1 text-sm">{t("Lane")}<select dir="ltr" className="min-h-11 rounded border bg-background p-2" disabled={disabled || !fleet} value={input.lane} onChange={event => { const next = fleet?.lanes.find(item => item.id === event.target.value); setInput({ ...input, lane: next?.id ?? "", run: next?.run?.id ?? "" }); }}><option value="">{t("Choose lane with an existing attempt")}</option>{fleet?.lanes.filter(item => item.run).map(item => <option key={item.id} value={item.id}>{item.id}</option>)}</select></label>
    {lane?.run && <p className="text-sm">{t("Attempt")}: <bdi dir="ltr">{lane.run.id}</bdi></p>}
    <Button disabled={disabled || !valid} onClick={() => send({ type: "configure", input })}>{t("Use these connection settings")}</Button>
    <p className="text-xs">{t("Using settings does not contact the worker. Read its status explicitly after setup; recorded local attempts may not have a remote assignment.")}</p>
  </div></details>;
}
