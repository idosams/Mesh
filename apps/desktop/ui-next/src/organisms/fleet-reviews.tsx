import { FleetProjectComparison, type ProjectComparison, type CandidateSelector, type ProjectImport } from "./fleet-project-comparison";
import { reviewArtifactPreviewEnvelope } from "../models/review-artifact-preview";
import { useMemo, useState } from "react";
import { useTranslation } from "../lib/localization";
import { Button } from "../atoms/button";
import { FleetInputComparison, type InputComparison } from "./fleet-input-comparison";
import { ArtifactReview } from "./artifact-review";
import { reviewWorkbenchFromProjection } from "../models/review-workbench-adapter";
import { reduceReviewWorkbench, type ReviewWorkbenchModel } from "../models/review-workbench";

export type FleetReviewSelection = { objective: string; lane: string; checkpoint: string; version: string; bundle: string };
export type SavedReview = { bundle: string; subject_operation: string; recorded: boolean; content_complete: boolean; reviewed_head: string | null; presentation_digest: string | null; bundle_changes: unknown[]; bundle_changes_not_listed: number; subject_operations_not_listed: number; unavailable_code: string | null; projection_authorizes_approval: boolean };
export type FleetReviewView = { candidate?: CandidateSelector | null; input_open: boolean; input_after: string | null; input_object: string | null; input_layout: "inline" | "split"; review_object: string | null; review_mode: "content" | "visual"; review_layout: "inline" | "split" };
export type FleetReviewPersistence = { phase: string; message: string; editable: boolean; busy?: boolean };
type ReviewDecision = { request: string; revision: number; status: "open" | "addressed"; checkpoint: string | null; version: string | null; bundle: string | null; approval_authority: false };
type ReviewChanges = { decisions?: ReviewDecision[]; deciding?: boolean; decisionPending?: { operation: string; request: string; expectedRevision: number; proposedCheckpoint: string | null; version: string | null; bundle: string | null } | null; decisionError?: string; decisionNotice?: string; responses?: { request: string; lane: string; checkpoint: string; version: string; bundle: string; status: "proposed"; approval_authority: false }[]; loaded?: boolean; rows: { id: string; message: string; status: "recorded" }[] | null; loading?: boolean; sending?: boolean; error: string; pending?: { request: string; message: string } | null };
export type FleetReviewPin = { candidate?: ProjectComparison; projectImport?: ProjectImport; projectSource?: string | null; candidateEnabled?: boolean; feedback?: ReviewChanges; artifact?: { generation: number; object: string; page: number; loading: boolean; error: string; envelope: unknown }; view?: FleetReviewView; input?: InputComparison; key: string; selection: FleetReviewSelection; goal: string | null; startingInput: string; review: SavedReview | null; loading: boolean; error: string };
export type FleetReviewQueue = { objective: string; lane: string; loading: boolean; error: string; page: { after: string | null; rows: (FleetReviewSelection & { run: string })[]; total: number; nextAfter: string | null; revision: number } | null };
const send = (detail: Record<string, string>) => document.dispatchEvent(new CustomEvent("mesh:fleets-intent", { detail }));

export function fleetSavedReviewModel(review: SavedReview): ReviewWorkbenchModel {
  if (!review.recorded || review.subject_operations_not_listed !== 0 || review.unavailable_code !== null) throw new Error("Incomplete saved review");
  return reviewWorkbenchFromProjection("Fleet lane", "Pinned saved result", review, {
    canRenderArtifactPreview: true, canInspectExactCopies: false, canRecordReview: false, canApprove: false,
    canApproveAndExport: false, canExportGit: false, canExportPrivateCopy: false,
    approvalReason: "Read-only saved result. Approval and integration into the original project's main are not available here.",
  });
}
function ReviewContent({ initial, pin, editable }: { initial: ReviewWorkbenchModel; pin: FleetReviewPin; editable: boolean }) {
  const t = useTranslation();
  const model = useMemo(() => ({ ...initial,
    selectedChangeId: initial.changes.some(change => change.id === pin.view?.review_object) ? pin.view!.review_object! : initial.selectedChangeId,
    mode: pin.view?.review_mode ?? initial.mode, diffLayout: pin.view?.review_layout ?? initial.diffLayout,
  }), [initial, pin.view]);
  const artifact = pin.artifact?.object === model.selectedChangeId ? pin.artifact : undefined;
  const preview = useMemo(() => {
    if (!artifact?.envelope) return null;
    try { return reviewArtifactPreviewEnvelope(artifact.envelope, artifact.generation, pin.selection.bundle, model); }
    catch { return null; }
  }, [artifact, pin.selection.bundle, model]);
  return <ArtifactReview model={{ ...model, workspaceName: t(model.workspaceName), versionLabel: t(model.versionLabel), approvalReason: t(model.approvalReason) }} artifactPreview={preview} artifactPreviewLoading={Boolean(artifact?.loading)}
    artifactPreviewError={artifact?.error || (artifact?.envelope && !preview ? "This exact saved preview could not be verified." : null)} controls={{ countLabel: `${model.changes.length} ${t("changes in this saved review")}`, overflowLabel: null,
    canSetupApproval: false, setupApprovalLabel: t("Approval unavailable"), setupApprovalReason: t(model.approvalReason),
    canRecordReview: false, recordReviewLabel: t("Review already recorded"), recordReviewReason: t("This panel shows an existing immutable review."), earlierReviews: [] }}
    onIntent={intent => {
      if (editable && intent.type === "load-artifact-preview") {
        send({ type: "artifact-preview", pin: pin.key, object: intent.changeId, page: String(intent.pageNumber) });
      }
      // Read-only presentation. This panel never routes mutation, approval or export intents.
      if (editable && ["select-change", "change-mode", "change-diff-layout"].includes(intent.type)) {
        const next = reduceReviewWorkbench(model, intent);
        send({ type: "review-view", pin: pin.key, object: next.selectedChangeId, mode: next.mode, layout: next.diffLayout });
      }
    }} />;
}
function ChangeRequests({ pin, editable }: { pin: FleetReviewPin; editable: boolean }) {
  const t = useTranslation();
  const [message, setMessage] = useState("");
  const feedback = pin.feedback;
  const validMessage = Boolean(message.trim()) && new TextEncoder().encode(message).length <= 8192
    && !/[\u0000-\u0008\u000b-\u001f\u007f-\u009f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(message);
  const busy = Boolean(feedback?.loading || feedback?.sending || feedback?.deciding);
  return <details><summary className="font-semibold">{t("Request changes to this saved result")}</summary>
    <div className="grid gap-2 py-2 text-sm">
      <p>{t("Requests stay tied to this checkpoint. The originating agent can read them when it next checks its context. Recorded does not mean delivered or addressed, and does not restart a worker.")}</p>
      <Button variant="secondary" disabled={busy} onClick={() => send({ type: "review-changes", pin: pin.key })}>{t("Read recorded change requests")}</Button>
      {busy && <p role="status">{t(feedback?.deciding ? "Waiting for the native decision confirmation…" : feedback?.sending ? "Recording the exact change request…" : "Reading recorded requests…")}</p>}
      {feedback?.error && <p role="alert">{t(feedback.error)}</p>}
      {feedback?.decisionNotice && <p role="status">{t(feedback.decisionNotice)}</p>}
      {feedback?.decisionError && <p role="alert">{t(feedback.decisionError)}</p>}
      {feedback?.decisionPending && <div className="grid gap-2"><p>{t("Keep the original decision retry, or read the latest state to choose again. Reloading does not cancel a native confirmation already in progress.")}</p>
        <Button variant="secondary" disabled={busy || !editable} onClick={() => send({ type: "retry-review-decision", pin: pin.key })}>{t("Retry this exact decision")}</Button>
        <Button variant="quiet" disabled={busy || !editable} onClick={() => send({ type: "reload-review-decision", pin: pin.key })}>{t("Read latest state and choose again")}</Button>
      </div>}
      {feedback?.rows && !feedback.loaded && <p>{t("Read recorded change requests to load any earlier feedback for this result.")}</p>}
      {feedback?.rows && (feedback.rows.length ? <ul className="grid gap-2">{feedback.rows.map(row => <li key={row.id} className="whitespace-pre-wrap break-words border-t border-border pt-2"><strong>{t("Recorded request")}: </strong><bdi dir="auto">{row.message}</bdi>
        {feedback.decisions?.find(decision => decision.request === row.id)?.status === "addressed" ? <div className="grid gap-1 text-xs"><p>{t("Request marked addressed. This is not approval or integration into main.")}</p>
          <Button variant="quiet" disabled={busy || !editable || Boolean(feedback.decisionPending) || Boolean(feedback.error) || (feedback.decisions.find(decision => decision.request === row.id)?.revision ?? 64) >= 64} onClick={() => send({ type: "decide-review-change", pin: pin.key, request: row.id, checkpoint: "" })}>{t("Reopen change request")}</Button>
        </div> : <p className="text-xs">{t(feedback.decisions?.some(decision => decision.request === row.id) ? "Request open." : "Read the current decision before choosing a result.")}</p>}
        {feedback.responses?.filter(response => response.request === row.id).map(response => <div key={response.checkpoint} className="mt-2 grid gap-1 rounded border border-border p-2 text-xs">
          <p className="break-all">{t("Proposed saved result")}: <bdi dir="ltr">{response.version}</bdi></p>
          <p>{t("This proposal does not resolve the request or approve the result.")}</p>
          <Button variant="secondary" disabled={busy || !editable || Boolean(feedback.error)} onClick={() => send({ type: "pin-review-response", pin: pin.key, request: row.id, checkpoint: response.checkpoint })}>{t("Pin proposed result beside this review")}</Button>
          <Button variant="secondary" disabled={busy || !editable || Boolean(feedback.decisionPending) || Boolean(feedback.error) || !feedback.decisions?.some(decision => decision.request === row.id && decision.checkpoint !== response.checkpoint && decision.revision < 64)} onClick={() => send({ type: "decide-review-change", pin: pin.key, request: row.id, checkpoint: response.checkpoint })}>{t("Mark request addressed by this result")}</Button>
        </div>)}
      </li>)}</ul> : <p>{t("No recorded requests for this result.")}</p>)}
      {feedback?.pending ? <><p className="whitespace-pre-wrap break-words">{t("Pending receipt")}: <bdi dir="auto">{feedback.pending.message}</bdi></p><Button variant="secondary" disabled={busy || !editable} onClick={() => send({ type: "retry-review-changes", pin: pin.key })}>{t("Retry this exact change request")}</Button></> :
        <form className="grid gap-2" onSubmit={event => { event.preventDefault(); if (validMessage) send({ type: "request-review-changes", pin: pin.key, message }); }}>
          <label className="grid gap-1">{t("Requested changes")}<textarea dir="auto" className="min-h-24 rounded border border-border bg-background p-2" value={message} maxLength={8192} disabled={busy || !editable} onChange={event => setMessage(event.target.value)} /></label>
          {message && !validMessage && <p role="alert">{t("Enter non-empty text within 8 KiB, without hidden control or direction characters.")}</p>}
          <p className="text-xs text-muted-foreground">{t("Up to 8 KiB of text. Saved requests remain available after restart. Closing this panel does not cancel a submitted request.")}</p>
          <Button variant="secondary" type="submit" disabled={busy || !editable || !pin.review?.content_complete || !validMessage}>{t("Record change request")}</Button>
        </form>}
    </div>
  </details>;
}
export function FleetReviewPanel({ pin, editable = true }: { pin: FleetReviewPin; editable?: boolean }) {
  const t = useTranslation();
  const model = useMemo(() => {
    if (!pin.review?.content_complete || pin.review.bundle_changes.length === 0) return null;
    try { return fleetSavedReviewModel(pin.review); } catch { return null; }
  }, [pin.review]);
  const empty = pin.review?.content_complete && pin.review.bundle_changes.length === 0
    && pin.review.bundle_changes_not_listed === 0 && pin.review.subject_operations_not_listed === 0 && pin.review.unavailable_code === null;
  return <article className="grid min-w-0 gap-3 rounded-lg border border-border p-3" aria-label={`${t("Pinned fleet review")} ${pin.key}`} data-mesh-fleet-review={pin.key}>
    <div className="flex items-start justify-between gap-2"><h4 dir="auto" className="whitespace-pre-wrap break-words font-semibold">{pin.goal ?? t("Saved lane review")}</h4>
      <Button variant="secondary" disabled={!editable} onClick={() => send({ type: "close-review", pin: pin.key })}>{t("Close review")} <bdi dir="ltr">{pin.key}</bdi></Button></div>
    <p className="text-xs text-muted-foreground">{t("Comparison base: the recorded review uses its original lane review base. Use the starting-version comparison below to inspect what this lane changed.")}</p>
    <details className="break-all text-xs"><summary>{t("Exact saved selection and base")}</summary><p>{t("Fleet")}: <bdi dir="ltr">{pin.selection.objective}</bdi></p><p>{t("Lane")}: <bdi dir="ltr">{pin.selection.lane}</bdi></p><p>{t("Checkpoint")}: <bdi dir="ltr">{pin.selection.checkpoint}</bdi></p><p>{t("Saved operation")}: <bdi dir="ltr">{pin.selection.version}</bdi></p><p>{t("Review")}: <bdi dir="ltr">{pin.selection.bundle}</bdi></p>
      <p>{t("Recorded base head")}: {pin.review?.reviewed_head ? <bdi dir="ltr">{pin.review.reviewed_head}</bdi> : t("Unavailable")}</p><p>{t("Lane starting input")}: <bdi dir="ltr">{pin.startingInput}</bdi></p></details>
    {pin.view?.input_object && !pin.input?.file && <p className="break-all text-xs">{t("Saved changed-object selection")}: <bdi dir="ltr">{pin.view.input_object}</bdi>. {t("Content must be verified before display.")}</p>}
    <FleetInputComparison pin={pin.key} input={pin.input} layout={pin.view?.input_layout} editable={editable} />
    <FleetProjectComparison pin={pin.key} pending={pin.view?.candidate} comparison={pin.candidate} imported={pin.projectImport} enabled={editable && Boolean(pin.candidateEnabled)} project={pin.projectSource} />
    <ChangeRequests pin={pin} editable={editable} />
    <details><summary className="font-semibold">{t("Recorded review against its original review base")}</summary>
    {pin.loading && <p role="status" className="text-sm">{t("Reading the exact saved result\u2026")}</p>}
    {pin.error && <p role="alert" className="text-sm">{t(pin.error)} {t("Any content below is the previously verified cached result.")}</p>}
    {!pin.loading && (!pin.review || pin.error || !pin.review.content_complete || (!model && !empty)) && <Button variant="secondary" onClick={() => send({ type: "retry-review", pin: pin.key })}>{t("Retry exact saved result")}</Button>}
    {pin.review && !pin.review.content_complete && <p role="alert" className="text-sm">{t("This review is incomplete or unavailable.")} {pin.review.bundle_changes_not_listed} {t("changes and")} {pin.review.subject_operations_not_listed} {t("operations are not listed. No complete comparison is shown.")}</p>}
    {pin.review?.content_complete && !model && !empty && <p role="alert" className="text-sm">{t("The saved comparison could not be safely displayed. Its exact selection is retained.")}</p>}
    {empty && <p className="text-sm">{t("No file changes in this recorded comparison.")}</p>}
    {model && pin.view?.review_object && !model.changes.some(change => change.id === pin.view?.review_object) && <p role="status">{t("The saved file selection is unavailable in this review. Showing the first available change; the saved selection is retained.")}</p>}
    {model && <ReviewContent initial={model} pin={pin} editable={editable} />}
    </details>
  </article>;
}
export function FleetReviewPanels({ pins, notice, persistence }: { pins: FleetReviewPin[]; notice: string; persistence?: FleetReviewPersistence }) {
  const t = useTranslation();
  if (pins.length === 0 && !notice && (!persistence || persistence.phase === "session")) return null;
  return <section className="grid gap-3" aria-label={t("Pinned fleet reviews")}><h3 className="text-lg font-semibold">{t("Pinned fleet reviews")} · {pins.length} / 8</h3>
    {notice && <p role="status" className="text-sm">{t(notice)}</p>}
    <p className="text-xs text-muted-foreground">{t("Saved selections contain no file content. Reopening rechecks exact history; unavailable lanes remain listed and never restart agents.")}</p>
    {persistence && <div className="grid gap-2 text-sm" role="status">
      <p>{t(persistence.phase === "loading" ? "Loading saved review selections…" : persistence.phase === "saving" ? "Saving review selections…" : persistence.phase === "saved" ? "Review selections saved." : persistence.phase === "session" ? "Review selections have not loaded yet." : persistence.message)}</p>
      {persistence.phase === "error" && <Button variant="secondary" disabled={persistence.busy} onClick={() => send({ type: "retry-saved-reviews" })}>{t("Retry saving or loading review selections")}</Button>}
      {["saved", "error"].includes(persistence.phase) && <><Button variant="quiet" disabled={persistence.busy} onClick={() => send({ type: "reload-saved-reviews" })}>{t("Reload saved review set")}</Button><p className="text-xs text-muted-foreground">{t("Reload replaces local selections with the saved set, including any unsaved local choices.")}</p></>}
    </div>}
    <div className="grid items-start gap-4 xl:grid-cols-2">{pins.map(pin => <FleetReviewPanel key={pin.key} pin={pin} editable={persistence?.editable ?? true} />)}</div>
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
