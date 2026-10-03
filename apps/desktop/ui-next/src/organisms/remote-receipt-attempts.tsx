import { Button } from "../atoms/button";
import { useTranslation } from "../lib/localization";
export type ReceiptAttempt = { offer: string; checkpoint: string; version: string };
export type ReceivedResult = { offer: string; correlation: string; version: string; review: string; objective: string };
const send = (type: string, offer?: string) => document.dispatchEvent(new CustomEvent("mesh:remote-observation-intent", { detail: { type, offer } }));
export function RemoteReceiptAttempts({ attempts, received, disabled }: { attempts: ReceiptAttempt[] | null; received: ReceivedResult | null; disabled: boolean }) {
  const t = useTranslation();
  return <section className="grid gap-2 rounded border p-3" aria-label={t("Saved downloads")}>
    <Button variant="secondary" disabled={disabled} onClick={() => send("receipt-list")}>{t("Load saved downloads")}</Button>
    <p className="text-sm">{t("Recover only the saved download attempt. A completed copy is checked again; interrupted work may resume. This does not start agents or approve work.")}</p>
    {attempts?.length === 0 && <p role="status">{t("No saved downloads for this connection.")}</p>}
    {attempts && <ul className="grid gap-2">{attempts.map(entry => <li className="grid gap-1 rounded border p-2" key={entry.offer}>
      <span className="break-all text-sm"><bdi dir="ltr">{entry.checkpoint}</bdi></span>
      <Button variant="secondary" disabled={disabled} onClick={() => send("recover-result", entry.offer)}>{t("Check or resume saved download")}</Button>
    </li>)}</ul>}
    {received && <div className="grid gap-2" role="status">
      <p>{t("A private saved copy is ready for review. It has not been approved or applied to the original project.")}</p>
      <Button disabled={disabled} onClick={() => document.dispatchEvent(new CustomEvent("mesh:fleets-intent", { detail: { type: "remote-results", objective: received.objective } }))}>{t("Show downloaded reviews")}</Button>
    </div>}
  </section>;
}
