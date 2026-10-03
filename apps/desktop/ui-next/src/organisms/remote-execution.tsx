import { useTranslation } from "../lib/localization";
export type RecordedExecution = { observed: string; admitted: boolean; launchRecorded: boolean; recorded: null | { revision: string; state: string } };
const labels: Record<string, string> = {
  unrecorded: "Launch recorded; progress unknown",
  "setup-incomplete": "Worker setup incomplete",
  launching: "Agent launch recorded",
  running: "Agent activity recorded",
  waiting: "Waiting for input or dependencies",
  reconciling: "Execution needs reconciliation",
  stopping: "Stop requested; termination unconfirmed",
  succeeded: "Successful completion recorded",
  failed: "Failed completion recorded",
  cancelled: "Cancellation completion recorded",
};
export function RemoteExecution({ value }: { value: RecordedExecution }) {
  const t = useTranslation(), date = new Date(Number(value.observed));
  const observed = Number.isFinite(date.getTime()) ? date.toISOString() : t("Observation time unavailable");
  return <section className="grid gap-1 rounded border p-3 text-sm" aria-label={t("Last recorded execution")}>
    <h3 className="font-medium">{t("Last recorded execution")}</h3>
    <p role="status">{t(value.recorded ? labels[value.recorded.state] ?? "Execution state unavailable" : value.admitted ? "No launch record was returned." : "The worker has no admission record for this assignment.")}</p>
    <p>{t("Observed at")}: <bdi dir="ltr">{observed}</bdi></p>
    {value.recorded && <p>{t("Recorded revision")}: <bdi dir="ltr">{value.recorded.revision}</bdi></p>}
    <p>{t("This is saved execution history. It does not confirm current process activity, release capacity, or authorize another attempt.")}</p>
  </section>;
}
