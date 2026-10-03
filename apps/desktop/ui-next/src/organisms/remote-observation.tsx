import { RemoteConnectionSetup, type RemoteSetupDraft, type RemoteSetupFleet } from "./remote-connection-setup";
import { useEffect, useState } from "react";
import { Button } from "../atoms/button";
import { useTranslation } from "../lib/localization";
type Selection = { id: string; host: string; worker: string; objective: string; lane: string; run: string };
type Projection = { draft?: RemoteSetupDraft | null; selection: Selection | null; busy: boolean; available: boolean; error: string;
  status: null | { observed: string; admitted: boolean; launchRecorded: boolean; leaseUntil: string | null };
  results: null | { available: boolean; count: number; revision: string | null; hasMore: boolean } };
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
      <RemoteConnectionSetup draft={p.draft ?? null} fleets={fleets} disabled={disabled} />
      <p className="text-sm">{t("Connection settings last until Mesh closes. Reading observations does not start agents or approve work.")}</p>
      <div className="flex flex-wrap gap-2"><Button disabled={disabled} onClick={() => send("choose")}>{t("Choose connection configuration")}</Button>
      {p.selection && <Button variant="secondary" disabled={disabled} onClick={() => send("forget")}>{t("Forget this selection")}</Button>}</div>
      {p.selection && <><dl className="grid gap-1 break-all text-sm">{[["Host", p.selection.host], ["Worker identity", p.selection.worker], ["Fleet", p.selection.objective], ["Lane", p.selection.lane], ["Attempt", p.selection.run]].map(([label, value]) => <div key={label}><dt className="font-medium">{t(label)}</dt><dd><bdi dir="ltr">{value}</bdi></dd></div>)}</dl>
      <div className="flex flex-wrap gap-2"><Button disabled={disabled} onClick={() => send("status")}>{t("Read worker status")}</Button><Button disabled={disabled} onClick={() => send("results")}>{t("Find saved remote results")}</Button></div></>}
      {p.busy && <p role="status">{t("Reading native connection information…")}</p>}
      {p.error && <p role="alert">{t(p.error)}</p>}
      {p.status && <div className="grid gap-1 text-sm" aria-label={t("Last verified worker observation")}>
        <p>{t(p.status.admitted ? "The worker recorded this assignment." : "The worker has no admission record for this assignment.")}</p>
        <p>{t(p.status.launchRecorded ? "A launch record exists. This does not establish that the process is still running." : "No launch record was returned.")}</p>
        <p>{t("Observed at")}: <bdi dir="ltr">{timestamp(p.status.observed, t("Observation time unavailable"))}</bdi></p>
        {p.status.leaseUntil && <p>{t("Lease ends at")}: <bdi dir="ltr">{timestamp(p.status.leaseUntil, t("Observation time unavailable"))}</bdi></p>}
      </div>}
      {p.results && <div className="text-sm" role="status">{!p.results.available ? t("Saved result history is unavailable for this assignment.") : <><p>{t("Saved result offers in this page")}: {p.results.count}</p><p>{t("Result discovery does not download or accept the work. Use the existing result recovery flow to receive it.")}</p>{p.results.hasMore && <p>{t("More saved results exist beyond this first page.")}</p>}</>}</div>}
    </div>
  </details>;
}
