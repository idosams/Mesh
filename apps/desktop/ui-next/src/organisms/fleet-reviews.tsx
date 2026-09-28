import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "../lib/localization";
import { Button } from "../atoms/button";
import { ArtifactReview } from "./artifact-review";
import { reviewWorkbenchFromProjection } from "../models/review-workbench-adapter";
import { reconcileReviewWorkbenchProjection, reduceReviewWorkbench, type ReviewWorkbenchModel } from "../models/review-workbench";

export type FleetReviewSelection = { objective: string; lane: string; checkpoint: string; version: string; bundle: string };
type SavedReview = { bundle: string; subject_operation: string; recorded: boolean; content_complete: boolean; reviewed_head: string | null; presentation_digest: string | null; bundle_changes: unknown[]; bundle_changes_not_listed: number; subject_operations_not_listed: number; unavailable_code: string | null; projection_authorizes_approval: boolean };
export type FleetReviewPin = { key: string; selection: FleetReviewSelection; goal: string; startingInput: string; review: SavedReview | null; loading: boolean; error: string };
export type FleetReviewQueue = { objective: string; lane: string; loading: boolean; error: string; page: { after: string | null; rows: (FleetReviewSelection & { run: string })[]; total: number; nextAfter: string | null; revision: number } | null };
const send = (detail: Record<string, string>) => document.dispatchEvent(new CustomEvent("mesh:fleets-intent", { detail }));

export function fleetSavedReviewModel(review: SavedReview): ReviewWorkbenchModel {
  if (!review.recorded || review.subject_operations_not_listed !== 0 || review.unavailable_code !== null) throw new Error("Incomplete saved review");
  return reviewWorkbenchFromProjection("Fleet lane", "Pinned saved result", review, {
    canRenderArtifactPreview: false, canInspectExactCopies: false, canRecordReview: false, canApprove: false,
    canApproveAndExport: false, canExportGit: false, canExportPrivateCopy: false,
    approvalReason: "Read-only saved result. Approval and integration into the original project's main are not available here.",
  });
}
function ReviewContent({ initial }: { initial: ReviewWorkbenchModel }) {
  const t = useTranslation();
  const [model, setModel] = useState(initial);
  useEffect(() => setModel(previous => reconcileReviewWorkbenchProjection(previous, initial)), [initial]);
  return <ArtifactReview model={{ ...model, workspaceName: t(model.workspaceName), versionLabel: t(model.versionLabel), approvalReason: t(model.approvalReason) }} controls={{ countLabel: `${model.changes.length} ${t("changes in this saved review")}`, overflowLabel: null,
    canSetupApproval: false, setupApprovalLabel: t("Approval unavailable"), setupApprovalReason: t(model.approvalReason),
    canRecordReview: false, recordReviewLabel: t("Review already recorded"), recordReviewReason: t("This panel shows an existing immutable review."), earlierReviews: [] }}
    onIntent={intent => {
      // Local presentation only. This panel never routes mutation, approval or export intents.
      if (["select-change", "change-mode", "change-diff-layout"].includes(intent.type)) setModel(previous => reduceReviewWorkbench(previous, intent));
    }} />;
}
export function FleetReviewPanel({ pin }: { pin: FleetReviewPin }) {
  const t = useTranslation();
  const model = useMemo(() => {
    if (!pin.review?.content_complete || pin.review.bundle_changes.length === 0) return null;
    try { return fleetSavedReviewModel(pin.review); } catch { return null; }
  }, [pin.review]);
  const empty = pin.review?.content_complete && pin.review.bundle_changes.length === 0
    && pin.review.bundle_changes_not_listed === 0 && pin.review.subject_operations_not_listed === 0 && pin.review.unavailable_code === null;
  return <article className="grid min-w-0 gap-3 rounded-lg border border-border p-3" aria-label={`${t("Pinned fleet review")} ${pin.key}`} data-mesh-fleet-review={pin.key}>
    <div className="flex items-start justify-between gap-2"><h4 dir="auto" className="whitespace-pre-wrap break-words font-semibold">{pin.goal}</h4>
      <Button variant="secondary" onClick={() => send({ type: "close-review", pin: pin.key })}>{t("Close review")} <bdi dir="ltr">{pin.key}</bdi></Button></div>
    <p className="text-xs text-muted-foreground">{t("Comparison base: this review's recorded lane base. Comparison with the lane's starting input is not available yet.")}</p>
    <details className="break-all text-xs"><summary>{t("Exact saved selection and base")}</summary><p>{t("Fleet")}: <bdi dir="ltr">{pin.selection.objective}</bdi></p><p>{t("Lane")}: <bdi dir="ltr">{pin.selection.lane}</bdi></p><p>{t("Checkpoint")}: <bdi dir="ltr">{pin.selection.checkpoint}</bdi></p><p>{t("Saved operation")}: <bdi dir="ltr">{pin.selection.version}</bdi></p><p>{t("Review")}: <bdi dir="ltr">{pin.selection.bundle}</bdi></p>
      <p>{t("Recorded base head")}: {pin.review?.reviewed_head ? <bdi dir="ltr">{pin.review.reviewed_head}</bdi> : t("Unavailable")}</p><p>{t("Lane starting input")}: <bdi dir="ltr">{pin.startingInput}</bdi></p></details>
    {pin.loading && <p role="status" className="text-sm">{t("Reading the exact saved result\u2026")}</p>}
    {pin.error && <p role="alert" className="text-sm">{t(pin.error)} {t("Any content below is the previously verified cached result.")}</p>}
    {!pin.loading && (!pin.review || pin.error || !pin.review.content_complete || (!model && !empty)) && <Button variant="secondary" onClick={() => send({ type: "retry-review", pin: pin.key })}>{t("Retry exact saved result")}</Button>}
    {pin.review && !pin.review.content_complete && <p role="alert" className="text-sm">{t("This review is incomplete or unavailable.")} {pin.review.bundle_changes_not_listed} {t("changes and")} {pin.review.subject_operations_not_listed} {t("operations are not listed. No complete comparison is shown.")}</p>}
    {pin.review?.content_complete && !model && !empty && <p role="alert" className="text-sm">{t("The saved comparison could not be safely displayed. Its exact selection is retained.")}</p>}
    {empty && <p className="text-sm">{t("No file changes in this recorded comparison.")}</p>}
    {model && <ReviewContent initial={model} />}
  </article>;
}
export function FleetReviewPanels({ pins, notice }: { pins: FleetReviewPin[]; notice: string }) {
  const t = useTranslation();
  if (pins.length === 0 && !notice) return null;
  return <section className="grid gap-3" aria-label={t("Pinned fleet reviews")}><h3 className="text-lg font-semibold">{t("Pinned fleet reviews")} · {pins.length} / 8</h3>
    {notice && <p role="status" className="text-sm">{t(notice)}</p>}
    <p className="text-xs text-muted-foreground">{t("Panels keep independent file selections and comparison layouts while agents continue working. Pins currently stay in this app view; they are not restored after an app reload.")}</p>
    <div className="grid items-start gap-4 xl:grid-cols-2">{pins.map(pin => <FleetReviewPanel key={pin.key} pin={pin} />)}</div>
  </section>;
}
export function FleetSavedResults({ objective, lane, queue, available }: { objective: string; lane: string; queue?: FleetReviewQueue; available: boolean }) {
  const t = useTranslation();
  return <section className="grid gap-2" aria-label={t("Saved lane results")}>
    <Button variant="secondary" disabled={!available || queue?.loading} onClick={() => send({ type: "reviews", objective, lane })}>{t(queue ? "Refresh saved results" : "Show saved results")}</Button>
    {queue && <>
      <Button variant="quiet" onClick={() => send({ type: "close-reviews", objective, lane })}>{t("Close result list")}</Button>
      {queue.loading && <p role="status" className="text-xs">{t("Reading saved results\u2026")}</p>}
      {queue.error && <p role="alert" className="text-xs">{t(queue.error)}</p>}
      {queue.page && <><p className="text-xs">{queue.page.total} {t("recorded results. This page stays fixed while agents work; refresh to discover more.")}</p>
        {!queue.page.rows.length && <p className="text-xs">{t(queue.page.total === 0 ? "No submitted saved reviews yet." : "End of the saved-result list.")}</p>}
        <ul className="grid max-h-60 gap-2 overflow-auto">{queue.page.rows.map(row => <li key={row.checkpoint} className="grid gap-1 border-t border-border py-2 text-xs"><p className="break-all">{t("Saved operation")}: <bdi dir="ltr">{row.version}</bdi></p><details className="break-all"><summary>{t("Result identity")}</summary><p>{t("Checkpoint")}: <bdi dir="ltr">{row.checkpoint}</bdi></p><p>{t("Review")}: <bdi dir="ltr">{row.bundle}</bdi></p><p>{t("Run")}: <bdi dir="ltr">{row.run}</bdi></p></details>
          <Button variant="secondary" disabled={!available || queue.loading || Boolean(queue.error)} onClick={() => send({ type: "pin-review", objective, lane, checkpoint: row.checkpoint, version: row.version, bundle: row.bundle })}>{t("Pin saved review")}</Button></li>)}</ul>
        {queue.page.nextAfter && <Button variant="secondary" disabled={!available || queue.loading || Boolean(queue.error)} onClick={() => send({ type: "reviews-page", objective, lane, after: queue.page!.nextAfter! })}>{t("Next saved results")}</Button>}
      </>}
    </>}
  </section>;
}
