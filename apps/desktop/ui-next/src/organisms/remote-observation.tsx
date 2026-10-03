import { RemoteReceiptAttempts, type ReceiptAttempt, type ReceivedResult } from "./remote-receipt-attempts";
import { RemoteResults, type RemoteResultPage } from "./remote-result-page";
import { RemoteConnectionProfiles, type RemoteProfiles } from "./remote-connection-profiles";
import { RemoteConnectionSetup, type RemoteSetupDraft, type RemoteSetupFleet, type RemoteSetupPreset } from "./remote-connection-setup";
import { useEffect, useState } from "react";
import { Button } from "../atoms/button";
import { useTranslation } from "../lib/localization";
type Selection = { id: string; host: string; worker: string; objective: string; lane: string; run: string };
type Projection = { receiptAttempts?: ReceiptAttempt[] | null; received?: ReceivedResult | null; recovery?: { disposition: string } | null; profiles?: RemoteProfiles | null; preset?: RemoteSetupPreset | null; draft?: RemoteSetupDraft | null; selection: Selection | null; busy: boolean; available: boolean; error: string;
  status: null | { observed: string; admitted: boolean; launchRecorded: boolean; leaseUntil: string | null };
  results: RemoteResultPage | null };
const empty: Projection = { selection: null, status: null, results: null, busy: false, available: false, error: "" };
const timestamp = (value: string, fallback: string) => { const date = new Date(Number(value)); return Number.isFinite(date.getTime()) ? date.toISOString() : fallback; };
const send = (type: string) => document.dispatchEvent(new CustomEvent("mesh:remote-observation-intent", { detail: { type } }));
export function RemoteObservation({ fleets }: { fleets: RemoteSetupFleet[] }) {
  const [projection, setProjection] = useState(empty);
  useEffect(() => {
    const update = (event: Event) => setProjection((event as CustomEvent<Projection>).detail);
    document.addEventListener("mesh:remote-observation-projection", update);
    document.dispatchEvent(new CustomEvent("mesh:remote-observation-visible"));
    return () => document.removeEventListener("mesh:remote-observation-projection", update);
  }, []);
  return <RemoteObservationView projection={projection} fleets={fleets} />;
}
export function RemoteObservationView({ projection: p, fleets = [] }: { projection: Projection; fleets?: RemoteSetupFleet[] }) {
  const t = useTranslation(), disabled = p.busy || !p.available;
  return <details className="rounded border p-3"><summary className="cursor-pointer font-medium">{t("Inspect a remote worker")}</summary>
    <div className="mt-3 grid gap-3">
      <RemoteConnectionProfiles profiles={p.profiles ?? null} selected={Boolean(p.selection)} disabled={disabled} />
      <RemoteConnectionSetup preset={p.preset} draft={p.draft ?? null} fleets={fleets} disabled={disabled} />
      <p className="text-sm">{t("The active selection lasts until Mesh closes. Saved settings must be opened explicitly. Reading observations does not start agents or approve work.")}</p>
      <div className="flex flex-wrap gap-2"><Button disabled={disabled} onClick={() => send("choose")}>{t("Choose connection configuration")}</Button>
      {p.selection && <Button variant="secondary" disabled={disabled} onClick={() => send("forget")}>{t("Forget this selection")}</Button>}</div>
      {p.selection && <><dl className="grid gap-1 break-all text-sm">{[["Host", p.selection.host], ["Worker identity", p.selection.worker], ["Fleet", p.selection.objective], ["Lane", p.selection.lane], ["Attempt", p.selection.run]].map(([label, value]) => <div key={label}><dt className="font-medium">{t(label)}</dt><dd><bdi dir="ltr">{value}</bdi></dd></div>)}</dl>
      <div className="flex flex-wrap gap-2"><Button disabled={disabled} onClick={() => send("status")}>{t("Read worker status")}</Button><Button disabled={disabled} onClick={() => send("results")}>{t("Find saved remote results")}</Button></div></>}
      {p.selection && <section className="grid gap-2 rounded border p-3" aria-label={t("Resume saved input transfer")}>
        <p className="text-sm">{t("Resume only the original interrupted input transfer. The worker must still retain its reservation. This does not create another attempt or extend its lease.")}</p>
        <Button variant="secondary" disabled={disabled} onClick={() => send("reconnect-input")}>{t("Resume saved input transfer")}</Button>
        {p.recovery && <p role="status">{t(p.recovery.disposition === "input-materialized" ? "The saved input is materialized on the worker. This does not establish that an agent is running." : "The worker retained the saved input. This does not establish that an agent is running.")}</p>}
      </section>}
      {p.busy && <p role="status">{t("Working with the remote connection…")}</p>}
      {p.error && <p role="alert">{t(p.error)}</p>}
      {p.status && <div className="grid gap-1 text-sm" aria-label={t("Last verified worker observation")}>
        <p>{t(p.status.admitted ? "The worker recorded this assignment." : "The worker has no admission record for this assignment.")}</p>
        <p>{t(p.status.launchRecorded ? "A launch record exists. This does not establish that the process is still running." : "No launch record was returned.")}</p>
        <p>{t("Observed at")}: <bdi dir="ltr">{timestamp(p.status.observed, t("Observation time unavailable"))}</bdi></p>
        {p.status.leaseUntil && <p>{t("Lease ends at")}: <bdi dir="ltr">{timestamp(p.status.leaseUntil, t("Observation time unavailable"))}</bdi></p>}
      </div>}
      {p.selection && <RemoteReceiptAttempts attempts={p.receiptAttempts ?? null} received={p.received ?? null} disabled={disabled} />}
      {p.results && <RemoteResults page={p.results} disabled={disabled} />}
    </div>
  </details>;
}
