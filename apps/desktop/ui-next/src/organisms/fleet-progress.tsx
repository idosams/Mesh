import { useTranslation } from "../lib/localization";
import { Button } from "../atoms/button";
import { FleetInputComparison, type InputComparison } from "./fleet-input-comparison";
export type ProgressQueue = { loading: boolean; error: string; page: null | { total: number; latest: string | null; starting: string | null; versions: { version: string; ordinal: number }[]; nextAfter: string | null } };
export type ProgressPin = { key: string; selection: { objective: string; lane: string; version: string }; layout: "inline" | "split"; input: InputComparison };
const send = (detail: Record<string, unknown>) => document.dispatchEvent(new CustomEvent("mesh:fleets-intent", { detail }));
const actions: Record<string, string> = { "input-review": "progress-first", "retry-input": "progress-retry", "input-file": "progress-file", "input-page": "progress-page", "input-layout": "progress-layout" };
export function ProgressPanels({ pins, notice }: { pins: ProgressPin[]; notice: string }) {
  const t = useTranslation();
  if (!pins.length && !notice) return null;
  return <section aria-label={t("Pinned saved progress")} className="grid gap-3">
    <h3 className="font-semibold">{t("Pinned saved progress")}</h3>
    <p className="text-sm">{t("Each panel stays on its selected save while agents keep working. These panels are kept for this session only.")}</p>
    {notice && <p role="status">{t(notice)}</p>}
    <div className="grid items-start gap-4 xl:grid-cols-2">{pins.map(pin => <article key={pin.key} className="grid min-w-0 gap-2 rounded border border-border p-3" aria-label={`${t("Saved progress")} ${pin.key}`}>
      <h4 className="font-semibold">{t("Saved progress")} {pin.key}</h4>
      <details className="break-all text-xs"><summary>{t("Exact saved version")}</summary><p>{pin.selection.version}</p><p>{pin.selection.lane}</p><p>{pin.selection.objective}</p></details>
      <Button variant="quiet" onClick={() => send({ type: "progress-close", pin: pin.key })}>{t("Close progress panel")}</Button>
      <FleetInputComparison pin={pin.key} input={pin.input} layout={pin.layout} onIntent={detail => { const type = actions[detail.type]; if (type) send({ ...detail, type }); }} />
    </article>)}</div>
  </section>;
}
export function SavedProgress({ objective, lane, queue, available }: { objective: string; lane: string; queue?: ProgressQueue; available: boolean }) {
  const t = useTranslation();
  return <section className="grid gap-2 text-sm" aria-label={t("Saved progress")}>
    <Button variant="secondary" disabled={!available || queue?.loading} onClick={() => send({ type: "progress-list", objective, lane })}>{t(queue ? "Refresh saved progress" : "Browse saved progress")}</Button>
    {queue && <>
      <Button variant="quiet" onClick={() => send({ type: "progress-close-list", objective, lane })}>{t("Close progress list")}</Button>
      {queue.loading && <p role="status">{t("Reading saved progress…")}</p>}
      {queue.error && <p role="alert">{t(queue.error)}</p>}
      {queue.page && <><p>{queue.page.total} {t("recorded versions")}</p>
        <p className="text-xs">{t("Recorded progress may be an intermediate save. It is not a completed handoff or approval.")}</p>
        {queue.page.latest && <details className="break-all text-xs"><summary>{t("Latest acknowledged save")}</summary>{queue.page.latest}</details>}
        {!queue.page.starting && <p>{t("The original starting version is unavailable for comparison.")}</p>}
        <ul className="grid max-h-56 gap-2 overflow-auto">{queue.page.versions.map(row => <li key={row.version} className="break-all">
          <Button variant="quiet" disabled={queue.loading || Boolean(queue.error) || !queue.page?.starting} onClick={() => send({ type: "progress-pin", objective, lane, version: row.version })}>{t("Pin saved version")} {row.ordinal} · {row.version.slice(0, 12)}</Button>
        </li>)}</ul>
        {queue.page.nextAfter && <Button variant="secondary" disabled={queue.loading || Boolean(queue.error)} onClick={() => send({ type: "progress-next", objective, lane, after: queue.page!.nextAfter })}>{t("Next saved versions")}</Button>}
      </>}
    </>}
  </section>;
}
