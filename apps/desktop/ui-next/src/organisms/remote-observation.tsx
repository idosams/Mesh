import { RemoteExecution, type RecordedExecution } from "./remote-execution";
import { RemoteCreationRequests, type CreationEntry, type CreationStatus, type CreationSource, type CreationHistories } from "./remote-creation";
import { RemoteReceiptAttempts, type ReceiptAttempt, type ReceivedResult } from "./remote-receipt-attempts";
import { RemoteResults, type RemoteResultPage } from "./remote-result-page";
import { RemoteConnectionProfiles, type RemoteProfiles } from "./remote-connection-profiles";
import { RemoteConnectionSetup, type RemoteSetupDraft, type RemoteSetupFleet, type RemoteSetupPreset } from "./remote-connection-setup";
import { useEffect, useState } from "react";
import { Button } from "../atoms/button";
import { useTranslation } from "../lib/localization";
type Selection = { id: string; host: string; worker: string; objective: string; lane: string; run: string };
type Projection = { execution?: RecordedExecution | null; originalRecovery?: { disposition: "initialization-recovered" } | null; inspection?: { observed: string; disposition: "unrecorded" | "verified" | "unavailable" } | null; creations?: CreationEntry[] | null; creationStatus?: CreationStatus | null; receiptAttempts?: ReceiptAttempt[] | null; received?: ReceivedResult | null; recovery?: { disposition: string } | null; profiles?: RemoteProfiles | null; preset?: RemoteSetupPreset | null; draft?: RemoteSetupDraft | null; selection: Selection | null; busy: boolean; available: boolean; error: string;
  status: null | { observed: string; admitted: boolean; launchRecorded: boolean; leaseUntil: string | null };
  results: RemoteResultPage | null };
const empty: Projection = { selection: null, status: null, results: null, busy: false, available: false, error: "" };
const timestamp = (value: string, fallback: string) => { const date = new Date(Number(value)); return Number.isFinite(date.getTime()) ? date.toISOString() : fallback; };
const send = (type: string) => document.dispatchEvent(new CustomEvent("mesh:remote-observation-intent", { detail: { type } }));
export function RemoteObservation({ fleets, projects, histories }: { fleets: RemoteSetupFleet[]; projects?: CreationSource[]; histories?: CreationHistories }) {
  const [projection, setProjection] = useState(empty);
  useEffect(() => {
    const update = (event: Event) => setProjection((event as CustomEvent<Projection>).detail);
    document.addEventListener("mesh:remote-observation-projection", update);
    document.dispatchEvent(new CustomEvent("mesh:remote-observation-visible"));
    return () => document.removeEventListener("mesh:remote-observation-projection", update);
  }, []);
  return <RemoteObservationView projection={projection} fleets={fleets} projects={projects} histories={histories} />;
}
export function RemoteObservationView({ projection: p, fleets = [], projects = [], histories = {} }: { projection: Projection; fleets?: RemoteSetupFleet[]; projects?: CreationSource[]; histories?: CreationHistories }) {
  const t = useTranslation(), disabled = p.busy || !p.available;
  return <details className="rounded border p-3"><summary className="cursor-pointer font-medium">{t("Inspect a remote worker")}</summary>
    <div className="mt-3 grid gap-3">
      <RemoteConnectionProfiles profiles={p.profiles ?? null} selected={Boolean(p.selection)} disabled={disabled} />
      <RemoteConnectionSetup preset={p.preset} draft={p.draft ?? null} fleets={fleets} disabled={disabled} projects={projects} histories={histories} />
      <RemoteCreationRequests entries={p.creations ?? null} status={p.creationStatus ?? null} disabled={disabled} />
      <p className="text-sm">{t("The active selection lasts until Mesh closes. Saved settings must be opened explicitly. Reading observations does not start agents or approve work.")}</p>
      <div className="flex flex-wrap gap-2"><Button disabled={disabled} onClick={() => send("choose")}>{t("Choose connection configuration")}</Button>
      {p.selection && <Button variant="secondary" disabled={disabled} onClick={() => send("forget")}>{t("Forget this selection")}</Button>}</div>
      {p.selection && <><dl className="grid gap-1 break-all text-sm">{[["Host", p.selection.host], ["Worker identity", p.selection.worker], ["Fleet", p.selection.objective], ["Lane", p.selection.lane], ["Attempt", p.selection.run]].map(([label, value]) => <div key={label}><dt className="font-medium">{t(label)}</dt><dd><bdi dir="ltr">{value}</bdi></dd></div>)}</dl>
      <div className="flex flex-wrap gap-2"><Button disabled={disabled} onClick={() => send("status")}>{t("Read worker status")}</Button><Button disabled={disabled} onClick={() => send("execution")}>{t("Read recorded execution")}</Button><Button disabled={disabled} onClick={() => send("results")}>{t("Find saved remote results")}</Button></div></>}
      {p.selection && <section className="grid gap-2 rounded border p-3" aria-label={t("Original input inspection")}>
        <p className="text-sm">{t("Check the original saved input on the worker. This can take time for large inputs and does not resume or restart work.")}</p>
        <Button variant="secondary" disabled={disabled} onClick={() => send("input-inspection")}>{t("Inspect original input")}</Button>
        {p.inspection && <div role="status">
          <p>{t(p.inspection.disposition === "verified" ? "The original input was verified at the observation time. This does not establish that an agent is running or can be restarted." : p.inspection.disposition === "unrecorded" ? "The worker has no retained input record. This does not prove that no work was started." : "The worker could not verify the original input. Preserve the existing attempt for reconciliation.")}</p>
          <p>{t("Observed at")}: <bdi dir="ltr">{timestamp(p.inspection.observed, t("Observation time unavailable"))}</bdi></p>
        </div>}
      </section>}
      {p.selection && <section className="grid gap-2 rounded border p-3" aria-label={t("Resume saved input transfer")}>
        <p className="text-sm">{t("Resume only the original interrupted input transfer. The worker must still retain its reservation. This does not create another attempt or extend its lease.")}</p>
        <Button variant="secondary" disabled={disabled} onClick={() => send("reconnect-input")}>{t("Resume saved input transfer")}</Button>
        {p.recovery && <p role="status">{t(p.recovery.disposition === "input-materialized" ? "The saved input is materialized on the worker. This does not establish that an agent is running." : "The worker retained the saved input. This does not establish that an agent is running.")}</p>}
      </section>}
      {p.selection && <section className="grid gap-2 rounded border p-3" aria-label={t("Recover original worker workspace")}>
        <p className="text-sm">{t("Recover the original saved input after interrupted setup. This may start its assigned agent once setup is recovered. Existing or uncertain execution cannot be restarted here.")}</p>
        <Button variant="secondary" disabled={disabled} onClick={() => send("recover-original")}>{t("Recover original worker workspace")}</Button>
        {p.originalRecovery && <p role="status">{t("The original workspace setup was recovered. Read worker status to check execution; recovery does not prove that the agent is running.")}</p>}
      </section>}
      {p.busy && <p role="status">{t("Working with the remote connection…")}</p>}
      {p.error && <p role="alert">{t(p.error)}</p>}
      {p.selection && p.execution && <RemoteExecution value={p.execution} />}
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
