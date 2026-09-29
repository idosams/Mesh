import { useMemo } from "react";
import { Button } from "../atoms/button";
import { useTranslation } from "../lib/localization";
import { ArtifactReview } from "./artifact-review";
import { fleetSavedReviewModel, type SavedReview } from "./fleet-reviews";
import { reduceReviewWorkbench } from "../models/review-workbench";
import { reviewArtifactPreviewEnvelope } from "../models/review-artifact-preview";

type Selection = { objective: string; offer: string; correlation: string; lane: string; run: string; version: string; bundle: string; remote_version: string };
export type RemoteReviewPin = { key: string; selection: Selection; review: SavedReview | null; loading: boolean; error: string; view: { object: string | null; mode: "content" | "visual"; layout: "inline" | "split" }; artifact?: { generation: number; object: string; loading: boolean; error: string; envelope: unknown } };
export type RemoteReviewQueue = { loading: boolean; error: string; page?: { snapshot: number; next: number | null; rows: { sequence: number; offer: string; selection: Selection | null }[] } };
const send = (detail: Record<string, string>) => document.dispatchEvent(new CustomEvent("mesh:fleets-intent", { detail }));
function RemotePanel({ pin }: { pin: RemoteReviewPin }) {
  const t = useTranslation();
  const model = useMemo(() => {
    if (!pin.review?.content_complete) return null;
    try {
      const result = fleetSavedReviewModel(pin.review);
      return { ...result, workspaceName: "Remote result", versionLabel: "Received saved snapshot", mode: pin.view.mode, diffLayout: pin.view.layout,
        selectedChangeId: result.changes.some(c => c.id === pin.view.object) ? pin.view.object! : result.selectedChangeId };
    } catch { return null; }
  }, [pin.review, pin.view]);
  const artifact = pin.artifact?.object === model?.selectedChangeId ? pin.artifact : undefined;
  const preview = useMemo(() => {
    if (!model || !artifact?.envelope) return null;
    try { return reviewArtifactPreviewEnvelope(artifact.envelope, artifact.generation, pin.selection.bundle, model); } catch { return null; }
  }, [model, artifact, pin.selection.bundle]);
  return <article className="grid min-w-0 gap-2 rounded border border-border p-3" aria-label={t("Pinned remote review")}>
    <div className="flex flex-wrap items-center gap-2"><h4 className="font-semibold">{t("Received saved snapshot")}</h4><Button variant="quiet" onClick={() => send({ type: "remote-close", pin: pin.key })}>{t("Close review")}</Button></div>
    <p className="text-xs">{t("This is the received result tree. Comparison and import into the original project are not available here.")}</p>
    <details className="break-all text-xs"><summary>{t("Result identity")}</summary>{([ ["Fleet", pin.selection.objective], ["Lane", pin.selection.lane], ["Run", pin.selection.run], ["Saved operation", pin.selection.version], ["Review", pin.selection.bundle], ["Remote saved operation", pin.selection.remote_version], ["Offer", pin.selection.offer], ["Correlation", pin.selection.correlation] ]).map(([name, value]) => <p key={name}>{t(name)}: <bdi dir="ltr">{value}</bdi></p>)}</details>
    {pin.loading && <p role="status">{t("Reading the exact saved result…")}</p>}
    {pin.error && <p role="alert">{t(pin.error)}</p>}
    <Button variant="secondary" disabled={pin.loading} onClick={() => send({ type: "remote-retry", pin: pin.key })}>{t("Retry exact saved result")}</Button>
    {pin.review && !pin.review.content_complete && <p role="alert">{t("This review is incomplete or unavailable.")}</p>}
    {pin.review?.content_complete && !model && <p role="alert">{t("The saved comparison could not be safely displayed. Its exact selection is retained.")}</p>}
    {model && <ArtifactReview model={{ ...model, workspaceName: t(model.workspaceName), versionLabel: t(model.versionLabel), approvalReason: t(model.approvalReason) }}
      artifactPreview={preview} artifactPreviewLoading={Boolean(artifact?.loading)} artifactPreviewError={artifact?.error || (artifact?.envelope && !preview ? t("This exact saved preview could not be verified.") : null)}
      controls={{ countLabel: `${model.changes.length} ${t("changes in this saved review")}`, overflowLabel: null, canSetupApproval: false, setupApprovalLabel: t("Approval unavailable"), setupApprovalReason: t(model.approvalReason), canRecordReview: false, recordReviewLabel: t("Review already recorded"), recordReviewReason: t("This panel shows an existing immutable review."), earlierReviews: [] }}
      onIntent={intent => {
        if (intent.type === "load-artifact-preview" && !pin.error) send({ type: "remote-artifact", pin: pin.key, object: intent.changeId, page: String(intent.pageNumber) });
        if (["select-change", "change-mode", "change-diff-layout"].includes(intent.type)) {
          const next = reduceReviewWorkbench(model, intent);
          send({ type: "remote-view", pin: pin.key, object: next.selectedChangeId, mode: next.mode, layout: next.diffLayout });
        }
      }} />}
  </article>;
}
export function RemoteReviewPanels({ pins, notice }: { pins: RemoteReviewPin[]; notice: string }) {
  const t = useTranslation();
  if (!pins.length && !notice) return null;
  return <section className="grid gap-3" aria-label={t("Remote saved reviews")}><h3>{t("Remote saved reviews")}</h3>
    <p className="text-xs">{t("These panels stay fixed while agents work. Remote selections currently last for this app session; saved results remain in native history.")}</p>
    {notice && <p role="status">{t(notice)}</p>}<div className="grid items-start gap-4 xl:grid-cols-2">{pins.map(pin => <RemotePanel key={pin.key} pin={pin} />)}</div>
  </section>;
}
export function RemoteSavedResults({ objective, queue, available }: { objective: string; queue?: RemoteReviewQueue; available: boolean }) {
  const t = useTranslation();
  return <section className="grid gap-2" aria-label={t("Remote saved results")}>
    <Button variant="secondary" disabled={!available || queue?.loading} onClick={() => send({ type: "remote-results", objective })}>{t("Show remote saved results")}</Button>
    {queue && <><Button variant="quiet" onClick={() => send({ type: "remote-results-close", objective })}>{t("Close result list")}</Button>
      {queue.loading && <p role="status">{t("Reading saved results…")}</p>}{queue.error && <p role="alert">{t(queue.error)}</p>}
      {queue.page && <><p className="text-xs">{t("This page stays fixed. Refresh to discover newer results.")}</p>
        {!queue.page.rows.length && <p>{t("No retained remote results in this page.")}</p>}
        <ul className="grid max-h-60 gap-2 overflow-auto">{queue.page.rows.map(row => <li key={row.sequence} className="grid gap-1 border-t border-border py-2 text-xs">
          <bdi dir="ltr" className="break-all">{row.selection?.version ?? row.offer}</bdi>
          {row.selection ? <Button variant="secondary" disabled={!available || queue.loading || Boolean(queue.error)} onClick={() => send({ type: "remote-pin", objective, offer: row.offer, correlation: row.selection!.correlation })}>{t("Pin saved review")}</Button> : <p role="status">{t("Registration interrupted. This result is not ready for review.")}</p>}
        </li>)}</ul>
        {queue.page.next !== null && <Button variant="secondary" disabled={queue.loading || Boolean(queue.error)} onClick={() => send({ type: "remote-results-next", objective })}>{t("Next saved results")}</Button>}
      </>}
    </>}
  </section>;
}
