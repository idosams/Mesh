import { useTranslation } from "../lib/localization";
import { useEffect, useState } from "react";
import { Button } from "../atoms/button";

type Project = { nativeEvents?: boolean; detached?: boolean; recovery: "restored-stopped" | "unavailable" | null; id: string; generation: string; root: string; phase: string; outcome: string; savedVersion: string | null; captureAgeMs: number | null };
type SavedEntry = { path: string; kind: "file" | "folder"; bytes: number | null; digest: string | null; executable: boolean | null };
type SavedFile = { path: string; state: "text" | "binary" | "too-large"; text: string | null; bytes: number };
type Inspection = { operation: string; entries: SavedEntry[]; nextAfter: string | null; file: SavedFile | null };
type VersionPage = { versions: string[]; nextBefore: string | null };
type Change = { path: string; change: string; before: Omit<SavedEntry, "path"> | null; after: Omit<SavedEntry, "path"> | null };
type Comparison = { base: string; target: string; changes: Change[]; total: number; nextAfter: string | null;
  file: { path: string; before: SavedFile | null; after: SavedFile | null; beforeKind: string; afterKind: string } | null };
type PinnedComparison = { key: string; project: string; root: string; comparison: Comparison | null; selector: { base: string; target: string } };
type SavedReview = { bundle: string; target: string; reviewed_head: string | null; presentation: string | null; complete: boolean; unavailable: string | null;
  changes: { before: string | null; after: string | null; effect: string }[]; changes_not_listed: number; operations_not_listed: number };
type ReviewQueue = { reviews: SavedReview[]; notListed: number };
type ApprovalState = { available: boolean; enrolled: boolean; reason: string | null; mainAvailable: boolean;
  main: { head: string; bundle: string; target: string } | null };
type IntegrationPreview = { head: string; bundle: string; target: string; base_head: string; observed_digest: string;
  matches_base: number; already_present: number; preserve_current: number; conflicts: number; blocked: number; not_listed: number;
  entries: { path: string; status: string; reason: string | null }[] };
type Projection = { integrationPreviews?: Record<string, IntegrationPreview>; integrationErrors?: Record<string, string>; approvalStates?: Record<string, ApprovalState>; approvalFeedback?: Record<string, string>; reviewQueues?: Record<string, ReviewQueue>; selectedReviews?: Record<string, SavedReview>; pinStatus: string; pinError: string; pins: PinnedComparison[]; bases: Record<string, string>; comparisons: Record<string, Comparison>; inspections: Record<string, Inspection>; histories: Record<string, VersionPage>; projects: Project[]; busy: boolean; error: string; available: boolean };
const empty: Projection = { pinStatus: "loading", pinError: "", pins: [], bases: {}, comparisons: {}, inspections: {}, histories: {}, projects: [], busy: false, error: "", available: false };
const send = (detail: Record<string, string | null>) => document.dispatchEvent(new CustomEvent("mesh:attachments-intent", { detail }));
const phases: Record<string, string> = {
  starting: "Starting capture", scanning: "Checking changes", saving: "Saving a version",
  waiting: "Watching for changes", stopping: "Stopping capture", stopped: "Capture stopped", failed: "Capture needs attention",
};
const outcomes: Record<string, string> = {
  pending: "First capture pending", saved: "Version saved", unchanged: "No new changes",
  incomplete: "Capture incomplete", "source-unavailable": "Project folder unavailable",
  "store-unavailable": "History storage unavailable", "save-unavailable": "Version could not be saved", cancelled: "Capture cancelled",
};

export function AttachedProjects() {
  const t = useTranslation();
  const [projection, setProjection] = useState<Projection>(empty);
  const [source, setSource] = useState("");
  useEffect(() => {
    const update = (event: Event) => setProjection((event as CustomEvent<Projection>).detail);
    document.addEventListener("mesh:attachments-projection", update);
    document.dispatchEvent(new CustomEvent("mesh:attachments-visible", { detail: true }));
    return () => {
      document.removeEventListener("mesh:attachments-projection", update);
      document.dispatchEvent(new CustomEvent("mesh:attachments-visible", { detail: false }));
    };
  }, []);
  const disabled = projection.busy || !projection.available || Boolean(projection.error);
  return <section className="grid gap-4 rounded-xl border border-border bg-background p-5" aria-label={t("Attached projects")} data-mesh-proof="attached-projects">
    <div>
      <h3 className="text-xl font-semibold">{t("Use your existing project")}</h3>
      <p className="mt-2 text-sm leading-6 text-muted-foreground">{t("Keep your folder, editor, Git workflow and agent sessions where they are. Mesh saves history in separate storage while you work.")}</p>
    </div>
    <Button disabled={projection.busy || !projection.available} onClick={() => send({ type: "choose" })}>{t("Choose project to attach")}</Button>
    <label className="grid gap-2 text-sm font-medium">{t("Existing project path")}
      <input dir="ltr" className="min-h-11 rounded-md border border-border bg-background px-3" value={source}
        onChange={(event) => setSource(event.target.value)} placeholder="/Users/you/Project" autoComplete="off" />
    </label>
    <Button disabled={projection.busy || !projection.available || !source.startsWith("/")}
      onClick={() => send({ type: "attach", source })}>{t("Attach existing project")}</Button>
    {!projection.available && <p className="text-sm text-muted-foreground">{t("Attachment requires the native Mesh desktop.")}</p>}
    {projection.error && <p role="alert" className="text-sm">{t(projection.error)}</p>}
    <div className="flex items-center gap-3">
      <Button variant="secondary" disabled={projection.busy || !projection.available} onClick={() => send({ type: "refresh" })}>{t("Refresh status")}</Button>
      <span role="status" className="text-sm text-muted-foreground">{t(projection.busy ? "Updating…" : projection.error ? "Status may be out of date" : "")}</span>
    </div>
    <p role="status" className="text-xs text-muted-foreground">{t(projection.pinStatus === "loading" ? "Restoring saved comparisons…" : projection.pinStatus === "saving" ? "Saving comparison selections…" : projection.pinStatus === "saved" ? "Comparison selections saved" : "")}</p>
    {projection.pinError && <div role="alert" className="grid gap-2 text-sm"><p>{t(projection.pinError)}</p>
      <Button disabled={projection.busy} onClick={() => send({ type: "retry-pin-save" })}>{t("Retry saving or loading pins")}</Button>
      <Button variant="secondary" disabled={projection.busy} onClick={() => send({ type: "reload-pins" })}>{t("Replace open pins with saved set")}</Button>
    </div>}
    {projection.pins.length > 0 && <section aria-label={t("Pinned comparisons")} className="grid gap-3">
      <h3 className="text-lg font-semibold">{t("Pinned comparisons")} · {projection.pins.length} / 8</h3>
      <div className="grid items-start gap-4 xl:grid-cols-2">{projection.pins.map((pin) => <article key={pin.key} className="min-w-0 rounded-lg border border-border p-3">
        <div className="mb-3 flex items-start justify-between gap-2"><p className="break-all text-sm font-medium"><bdi dir="ltr">{pin.root}</bdi></p>
          <Button variant="secondary" disabled={projection.pinStatus === "loading"} onClick={() => send({ type: "close-pin", pin: pin.key })}>{t("Close comparison")} <bdi dir="ltr">{pin.key}</bdi></Button></div>
        {pin.comparison ? <SavedComparison project={pin.project} comparison={pin.comparison} disabled={disabled} pinKey={pin.key} />
          : <div className="grid gap-2 text-sm"><p>{t("Saved comparison unavailable. Its selection is retained.")}</p>
            <p className="break-all text-xs">{t("Base:")} <bdi dir="ltr">{pin.selector.base}</bdi></p><p className="break-all text-xs">{t("Compared version:")} <bdi dir="ltr">{pin.selector.target}</bdi></p>
            <Button disabled={projection.busy} onClick={() => send({ type: "retry-pin", pin: pin.key })}>{t("Retry saved comparison")}</Button></div>}
      </article>)}</div>
      <p className="text-xs text-muted-foreground">{t("Each pin keeps its own version pair, page and file selection. Saved selections return when Mesh opens; content is verified again from history.")}</p>
    </section>}
    {projection.projects.map((project) => {
      const terminal = project.phase === "stopped" || project.phase === "failed";
      return <article key={project.id} className="grid gap-2 rounded-lg border border-border p-4">
        <p className="break-all text-sm font-medium"><bdi dir="ltr">{project.root}</bdi></p>
        <p className="text-sm">{t(projection.error ? "Status may be out of date" : project.detached ? "Detached · saved history retained" : phases[project.phase])} · {t(project.detached ? "Reattach to enable capture controls" : project.recovery === "restored-stopped" ? "Saved history restored; resume when ready" : project.recovery === "unavailable" ? "Project or history needs reconciliation" : outcomes[project.outcome])}</p>
        {!terminal && !project.detached && <p className="text-xs text-muted-foreground">{t(project.nativeEvents ? "File-change signals active, with periodic checks for missed changes." : "Using periodic checks for file changes.")}</p>}
        <p className="break-all text-xs text-muted-foreground">{project.savedVersion ? <>{t("Latest saved version:")} <bdi dir="ltr">{project.savedVersion}</bdi></> : t(project.recovery === "unavailable" ? "Saved version unavailable until history is verified" : "No saved version yet")}</p>
        <p className="text-xs text-muted-foreground">{project.captureAgeMs === null ? t("No complete capture in this session") : <>{t("Last complete capture started")} {Math.floor(project.captureAgeMs / 1000)} {t("seconds ago")}</>}. {t("Change author unknown.")}</p>
        <div className="flex flex-wrap gap-2">
          <Button variant="secondary" disabled={disabled || !project.savedVersion}
            onClick={() => send({ type: "versions", id: project.id, before: null })}>{t("Show latest versions")}</Button>
          <Button variant="secondary" disabled={disabled || !project.savedVersion}
            onClick={() => send({ type: "reviews", id: project.id })}>{t("Show review requests")}</Button>
          <Button variant="secondary" disabled={disabled || project.detached || terminal || project.phase === "stopping"}
            onClick={() => send({ type: "control", id: project.id, generation: project.generation, action: "capture" })}>{t("Capture now")}</Button>
          <Button variant="secondary" disabled={disabled || project.detached || project.phase === "stopping"}
            onClick={() => send({ type: "control", id: project.id, generation: project.generation, action: terminal ? "resume" : "stop" })}>{t(terminal ? project.recovery === "unavailable" ? "Retry capture" : "Resume capture" : "Stop capture")}</Button>
          <Button variant="secondary" disabled={disabled}
            onClick={() => send({ type: "control", id: project.id, generation: project.generation, action: project.detached ? "reattach" : "detach" })}>{t(project.detached ? "Reattach project" : "Detach Mesh")}</Button>
        </div>
        {project.detached && <p className="text-xs text-muted-foreground">{t("Capture is disabled. Your files, Git workflow and saved history are retained. Reattach, then resume capture when ready.")}</p>}
        <section aria-label={t("Mesh main")} className="grid gap-2 rounded-lg border border-border p-3">
          <h4 className="text-sm font-semibold">{t("Mesh main · last checked")}</h4>
          <p className="text-xs text-muted-foreground">{t("The accepted saved version in Mesh. Your current files and Git branch can keep changing independently.")}</p>
          {!projection.approvalStates?.[project.id] ? <p className="text-sm">{t("Main and approval availability have not been verified.")}</p>
            : !projection.approvalStates[project.id].mainAvailable ? <p className="text-sm">{t("Mesh main cannot currently be verified. Its history is retained.")}</p>
            : projection.approvalStates[project.id].main ? <>
              <p className="break-all text-xs">{t("Accepted saved version:")} <bdi dir="ltr">{projection.approvalStates[project.id].main!.target}</bdi></p>
              <Button variant="secondary" disabled={disabled} onClick={() => send({ type: "open-main", id: project.id })}>{t("Inspect Mesh main")}</Button>
              <Button variant="secondary" disabled={disabled} onClick={() => send({ type: "compare-main", id: project.id })}>{t("Compare main with working files")}</Button>
            </> : <p className="text-sm">{t("No version has been accepted as Mesh main yet.")}</p>}
          <Button variant="secondary" disabled={disabled} onClick={() => send({ type: "check-approval", id: project.id })}>{t("Refresh main and approval availability")}</Button>
          {projection.approvalStates?.[project.id] && !projection.approvalStates[project.id].available && <p className="text-sm">{t("Approval unavailable:")} {t(projection.approvalStates[project.id].reason)}</p>}
          {projection.approvalStates?.[project.id]?.available && !projection.approvalStates[project.id].enrolled && <Button disabled={disabled}
            onClick={() => send({ type: "enroll-approval", id: project.id })}>{t("Set up approvals on this Mac")}</Button>}
          {projection.approvalFeedback?.[project.id] && <p role="status" className="text-sm">{t(projection.approvalFeedback[project.id])}</p>}
        </section>
        {projection.integrationErrors?.[project.id] && <p role="alert" className="text-sm">{t(projection.integrationErrors[project.id])}</p>}
        {projection.integrationPreviews?.[project.id] && <IntegrationPreviewCard preview={projection.integrationPreviews[project.id]} />}
        {projection.histories[project.id] && <section aria-label={`${t("Saved versions for")} \u2068${project.root}\u2069`} className="grid gap-2">
          <p className="text-sm font-medium">{t("Saved versions · newest first")}</p>
          <p className="text-xs text-muted-foreground">{t("Review requests use Mesh’s accepted main version as their base, or the empty starting state before its first approval.")}</p>
          <p className="break-all text-xs text-muted-foreground">{projection.bases[project.id] && <>{t("Selected comparison base:")} <bdi dir="ltr">{projection.bases[project.id]}</bdi></>}</p>
          <ol className="max-h-64 overflow-auto text-xs">
            {projection.histories[project.id].versions.map((version) => <li key={version} className="break-all border-b border-border py-2"><button className="text-left underline" disabled={disabled}
              onClick={() => send({ type: "inspect", id: project.id, operation: version })}><bdi dir="ltr">{version}</bdi></button>
              <div className="mt-1 flex gap-3"><button className="underline" disabled={disabled}
                onClick={() => send({ type: "set-base", id: project.id, operation: version })}>{t("Use as base")}</button>
              <button className="underline" disabled={disabled}
                onClick={() => send({ type: "request-review", id: project.id, target: version })}>{t("Request review")}</button>
              <button className="underline" disabled={disabled || !projection.bases[project.id]}
                onClick={() => send({ type: "compare", id: project.id, target: version })}>{t("Compare with base")}</button></div></li>)}
          </ol>
          {projection.histories[project.id].nextBefore && <Button variant="secondary" disabled={disabled}
            onClick={() => send({ type: "versions", id: project.id, before: projection.histories[project.id].nextBefore })}>{t("Older versions")}</Button>}
          <p className="text-xs text-muted-foreground">{t("This history page stays fixed while capture continues.")}</p>
        </section>}
        {projection.reviewQueues?.[project.id] && <section aria-label={t("Saved review requests")} className="grid gap-2">
          <h4 className="text-sm font-semibold">{t("Saved review requests")}</h4>
          {projection.reviewQueues[project.id].reviews.length === 0 && <p className="text-sm">{t("No review requests recorded.")}</p>}
          {projection.reviewQueues[project.id].reviews.map((review) => <button key={review.bundle} className="break-all text-left text-xs underline" disabled={disabled}
            onClick={() => send({ type: "open-review", id: project.id, bundle: review.bundle, target: review.target })}>{t("Open review")} <bdi dir="ltr">{review.bundle}</bdi></button>)}
          {projection.reviewQueues[project.id].notListed > 0 && <p className="text-xs">{projection.reviewQueues[project.id].notListed} {t("more requests are outside this page. Request review from a saved version to reopen its exact request.")}</p>}
        </section>}
        {projection.selectedReviews?.[project.id] && <AttachmentReview project={project.id} review={projection.selectedReviews[project.id]} disabled={disabled} approval={projection.approvalStates?.[project.id]} />}
        {projection.comparisons[project.id] && <SavedComparison project={project.id} comparison={projection.comparisons[project.id]} disabled={disabled} canPin={projection.pins.length < 8 && projection.pinStatus !== "loading" && !projection.pinError} />}
        {projection.inspections[project.id] && <SavedInspection project={project.id} inspection={projection.inspections[project.id]} disabled={disabled} />}
      </article>;
    })}
    <p className="text-xs text-muted-foreground">{t("Registered projects return when Mesh opens. Recovered projects remain stopped until you resume capture.")} {t("Writing saved versions back to your working folder is not available here yet.")}</p>
  </section>;
}

function SavedInspection({ project, inspection, disabled }: { project: string; inspection: Inspection; disabled: boolean }) {
  const t = useTranslation();
  return <section aria-label={t("Saved version files")} className="grid gap-3 rounded-lg border border-border p-3">
    <h4 className="text-sm font-semibold">{t("Files in saved version")}</h4>
    <p className="break-all text-xs text-muted-foreground"><bdi dir="ltr">{inspection.operation}</bdi></p>
    <ul className="max-h-64 overflow-auto text-sm">
      {inspection.entries.map((entry) => <li key={entry.path} className="break-all py-1">
        {entry.kind === "folder" ? <bdi dir="ltr">{entry.path}/</bdi> : <button className="text-left underline" disabled={disabled}
          onClick={() => send({ type: "file", id: project, operation: inspection.operation, path: entry.path })}><bdi dir="ltr">{entry.path}</bdi> · {entry.bytes} {t("Bytes")}{entry.executable ? <> · {t("Executable")}</> : ""}</button>}
      </li>)}
    </ul>
    {inspection.entries.length === 0 && <p className="text-sm">{t("This saved version has no files or folders.")}</p>}
    {inspection.nextAfter && <Button variant="secondary" disabled={disabled}
      onClick={() => send({ type: "entries", id: project, operation: inspection.operation, after: inspection.nextAfter })}>{t("More files")}</Button>}
    {inspection.file && <div className="grid gap-2">
      <p className="break-all text-sm font-medium"><bdi dir="ltr">{inspection.file.path}</bdi></p>
      {inspection.file.state === "text" ? <pre dir="ltr" className="max-h-96 overflow-auto whitespace-pre-wrap rounded-md bg-muted p-3 text-xs">{inspection.file.text}</pre>
        : <p className="text-sm text-muted-foreground">{t(inspection.file.state === "binary" ? "Binary file. Text preview is unavailable." : "This file exceeds the 256 KiB text preview limit.")}</p>}
    </div>}
    <p className="text-xs text-muted-foreground">{t("Read-only saved content. Edits in your working folder do not change this view.")}</p>
  </section>;
}

function SavedComparison({ project, comparison, disabled, pinKey, canPin = false }: { project: string; comparison: Comparison; disabled: boolean; pinKey?: string; canPin?: boolean }) {
  const t = useTranslation();
  const labels: Record<string, string> = { added: "Added", removed: "Removed", modified: "Content changed", "mode-changed": "Executable mode changed", "type-changed": "File/folder type changed" };
  const describe = (side: Change["before"]) => side?.kind === "file"
    ? <>{side.bytes} {t("Bytes")}, {t(side.executable ? "Executable" : "Not executable")}</>
    : t(side ? "Folder" : "Absent in this version");
  return <section aria-label={t("Saved version comparison")} className="grid gap-3 rounded-lg border border-border p-3">
    <h4 className="text-sm font-semibold">{t("Compare saved versions")} · {comparison.total} {t("changed paths")}</h4>
    {!pinKey && <Button variant="secondary" disabled={disabled || !canPin}
      onClick={() => send({ type: "pin-comparison", id: project, base: comparison.base, target: comparison.target })}>{t("Pin comparison alongside others")}</Button>}
    <p className="break-all text-xs text-muted-foreground">{t("Base:")} <bdi dir="ltr">{comparison.base}</bdi></p>
    <p className="break-all text-xs text-muted-foreground">{t("Compared version:")} <bdi dir="ltr">{comparison.target}</bdi></p>
    <ul className="max-h-64 overflow-auto text-sm">{comparison.changes.map((change) => <li key={change.path} className="py-1">
      <button className="break-all text-left underline" disabled={disabled}
        onClick={() => send({ type: "compare-file", id: project, base: comparison.base, target: comparison.target, path: change.path, ...(pinKey ? { pin: pinKey } : {}) })}>{t(labels[change.change])} · <bdi dir="ltr">{change.path}</bdi></button>
      <span className="block text-xs text-muted-foreground">{t("Before")}: {describe(change.before)} · {t("After")}: {describe(change.after)}</span>
    </li>)}</ul>
    {comparison.total === 0 && <p className="text-sm">{t("These versions have the same paths, content and executable modes.")}</p>}
    {comparison.nextAfter && <Button variant="secondary" disabled={disabled}
      onClick={() => send({ type: "compare-page", id: project, base: comparison.base, target: comparison.target, after: comparison.nextAfter, ...(pinKey ? { pin: pinKey } : {}) })}>{t("More changes")}</Button>}
    {comparison.file && <div className="grid gap-3">
      <p className="break-all text-sm font-medium"><bdi dir="ltr">{comparison.file.path}</bdi></p>
      <div className="grid gap-3 lg:grid-cols-2">
        <ComparisonSide label="Before" file={comparison.file.before} kind={comparison.file.beforeKind} />
        <ComparisonSide label="After" file={comparison.file.after} kind={comparison.file.afterKind} />
      </div>
    </div>}
    <p className="text-xs text-muted-foreground">{t("This comparison stays on these saved versions while work continues. It does not approve or apply changes.")}</p>
  </section>;
}
function ComparisonSide({ label, file, kind }: { label: string; file: SavedFile | null; kind: string }) {
  const t = useTranslation();
  return <section className="min-w-0" aria-label={t(label)}><h5 className="text-sm font-medium">{t(label)}</h5>
    {file?.state === "text" ? <pre dir="ltr" className="max-h-96 overflow-auto whitespace-pre-wrap rounded-md bg-muted p-3 text-xs">{file.text}</pre>
      : <p className="text-sm text-muted-foreground">{t(!file ? kind === "absent" ? "Absent in this version" : "Folder" : file.state === "binary" ? "Binary file; no text preview" : "File exceeds the 256 KiB preview limit")}</p>}
  </section>;
}

function AttachmentReview({ project, review, disabled, approval }: { project: string; review: SavedReview; disabled: boolean; approval?: ApprovalState }) {
  const t = useTranslation();
  return <section aria-label={t("Selected saved review")} className="grid gap-2 rounded-lg border border-border p-3">
    <h4 className="text-sm font-semibold">{t("Review request recorded")}</h4>
    <p className="break-all text-xs">{t("Bundle:")} <bdi dir="ltr">{review.bundle}</bdi></p>
    <p className="break-all text-xs">{t("Saved version:")} <bdi dir="ltr">{review.target}</bdi></p>
    <p className="break-all text-xs">{t("Presentation:")} {review.presentation ? <bdi dir="ltr">{review.presentation}</bdi> : t("Unavailable")}</p>
    <p className="text-sm">{t(review.unavailable ? "Review content is unavailable; the request is retained."
      : review.complete ? "Saved content verified for this request." : "This overview is incomplete; it cannot stand in for complete review.")}</p>
    <ul className="max-h-64 overflow-auto text-xs">{review.changes.map((change, index) => <li key={index} className="break-all py-1">
      <bdi dir="ltr">{change.effect}: {change.before && change.after && change.before !== change.after ? change.before + " → " + change.after : change.after ?? change.before}</bdi>
    </li>)}</ul>
    {(review.changes_not_listed > 0 || review.operations_not_listed > 0) && <p className="text-xs">{review.changes_not_listed} {t("changes and")} {review.operations_not_listed} {t("operations omitted from this overview.")}</p>}
    <Button variant="secondary" disabled={disabled || Boolean(review.unavailable)}
      onClick={() => send({ type: "review-files", id: project, bundle: review.bundle, target: review.target })}>{t("Inspect saved result")}</Button>
    <Button disabled={disabled || !review.complete || Boolean(review.unavailable) || !approval?.available || !approval.enrolled || !approval.mainAvailable || approval.main?.head === review.reviewed_head}
      onClick={() => send({ type: "approve-review", id: project, bundle: review.bundle, target: review.target })}>{t(approval?.main?.head === review.reviewed_head ? "This version is Mesh main" : "Approve as Mesh main…")}</Button>
    <p className="text-xs text-muted-foreground">{t("This request stays on its saved version while work continues. Change author unknown. Approval requires native confirmation and Touch ID or your Mac password. Working files and Git remain unchanged.")}</p>
  </section>;
}

const integrationLabels: Record<string, string> = {
  "matches-base": "Matches approved base", "already-present": "Already matches main", "preserve-current": "Keep current work",
  conflict: "Conflict", blocked: "Blocked by a parent conflict",
};
const integrationReasons: Record<string, string> = {
  "current-content-diverged": "Working content differs from both the base and main.",
  "contains-current-work": "This directory contains current work that must be retained.",
  "contains-unobserved-entry": "This directory contains entries outside the captured view, including possible ignored content.",
  "parent-change-conflicts": "A parent directory must be resolved first.",
};
function IntegrationPreviewCard({ preview }: { preview: IntegrationPreview }) {
  const t = useTranslation();
  return <section aria-label={t("Working-folder comparison")} className="grid gap-2 rounded-lg border border-border p-3">
    <h4 className="text-sm font-semibold">{t("Working folder compared with Mesh main")}</h4>
    <p className="break-all text-xs">{t("Accepted saved version:")} <bdi dir="ltr">{preview.target}</bdi></p>
    <p className="text-sm">{preview.matches_base} {t("match the approved base")} · {preview.already_present} {t("already match main")} · {preview.preserve_current} {t("current changes to keep")} · {preview.conflicts} {t("conflicts")} · {preview.blocked} {t("blocked changes")}</p>
    <ul className="max-h-64 overflow-auto text-xs">{preview.entries.map(entry => <li key={entry.path} className="break-all py-1">
      <strong>{t(integrationLabels[entry.status])}</strong>: <bdi dir="ltr">{entry.path}</bdi>{entry.reason && <p>{t(integrationReasons[entry.reason])}</p>}
    </li>)}</ul>
    {preview.not_listed > 0 && <p className="text-xs">{preview.not_listed} {t("more entries are omitted from this overview. The counts include all entries.")}</p>}
    <p className="text-xs text-muted-foreground">{t("Read-only observation. Working files may have changed since this comparison; it is not an atomic snapshot or permission to overwrite them. Write-back is not enabled here.")}</p>
  </section>;
}
