import { useState } from "react";
import { Badge } from "../atoms/badge";
import { Button } from "../atoms/button";
import { Card } from "../atoms/card";
import { SegmentedControl } from "../atoms/segmented-control";
import { ChangeNavigator } from "../molecules/change-navigator";
import { ReviewPageControls } from "../molecules/review-page-controls";
import type { ReviewPageControls as ReviewPageControlsModel } from "../models/review-page";
import {
  artifactContentReviewState,
  artifactSectionHunks,
  type ArtifactContentComparison,
} from "../models/artifact-content-diff";
import {
  selectedReviewChange,
  type DiffLayout,
  type ReviewArtifactAbsentPage,
  type ReviewArtifactPreview,
  type ReviewArtifactPreviewSide,
  type ReviewChange,
  type ReviewDiffLine,
  type ReviewMode,
  type ReviewWorkbenchIntent,
  type ReviewWorkbenchModel,
} from "../models/review-workbench";

const views = ["Visual", "Content changes"] as const;
const diffLayouts = ["Split", "Inline"] as const;
const PDF_PREVIEW_PAGE_LIMIT = 64;

type ArtifactReviewProps = {
  model: ReviewWorkbenchModel;
  controls?: ReviewPageControlsModel;
  onIntent: (intent: ReviewWorkbenchIntent) => void;
  artifactPreview?: ReviewArtifactPreview | null;
  artifactPreviewLoading?: boolean;
  artifactPreviewError?: string | null;
};

const disabledReviewControls: ReviewPageControlsModel = Object.freeze({
  countLabel: "Review summary unavailable",
  overflowLabel: null,
  canSetupApproval: false,
  setupApprovalLabel: "Approval unavailable",
  setupApprovalReason: "Review authority is unavailable.",
  canRecordReview: false,
  recordReviewLabel: "Record reviewed version",
  recordReviewReason: "Review authority is unavailable.",
  earlierReviews: Object.freeze([]),
});

export function ArtifactReview({
  model,
  controls = disabledReviewControls,
  onIntent,
  artifactPreview = null,
  artifactPreviewLoading = false,
  artifactPreviewError = null,
}: ArtifactReviewProps) {
  const change = selectedReviewChange(model);
  const selectedView = model.mode === "visual" ? "Visual" : "Content changes";
  const selectView = (value: string) => {
    const mode: ReviewMode = value === "Visual" ? "visual" : "content";
    onIntent({ type: "change-mode", mode });
  };

  return (
    <Card aria-label="Review workbench" data-mesh-proof="review-mounted" className="overflow-hidden">
      <header className="flex flex-col gap-4 border-b border-border p-5 lg:flex-row lg:items-center lg:justify-between">
        <div>
          <div className="flex flex-wrap items-center gap-2">
            <p className="text-xs font-semibold uppercase tracking-[0.16em] text-primary">Review workbench</p>
            <Badge tone="neutral">{model.bundleLabel}</Badge>
          </div>
          <h2 className="mt-2 text-xl font-semibold tracking-tight">Understand exactly what changed</h2>
          <p className="mt-1 text-sm text-muted-foreground">
            {model.workspaceName} · {model.versionLabel}
          </p>
        </div>
        <SegmentedControl proof="comparison-view" label="Comparison view" options={views} value={selectedView} onChange={selectView} />
      </header>

      <div className="border-b border-border p-5">
        <ReviewPageControls controls={controls} onIntent={onIntent} />
      </div>

      <div className="grid min-h-[32rem] xl:grid-cols-[17rem_minmax(0,1fr)_18rem]">
        <ChangeNavigator
          changes={model.changes}
          selectedChangeId={model.selectedChangeId}
          onSelect={(changeId) => onIntent({ type: "select-change", changeId })}
        />

        <section className="min-w-0 p-5" aria-label="Selected change comparison" aria-busy={artifactPreviewLoading}>
          <p className="sr-only" role="status" aria-live="polite">
            Selected {change.path}. {change.summary}
          </p>
          <div className="mb-5">
            <div className="flex flex-wrap items-center gap-2">
              <h3 className="text-lg font-semibold">{change.path}</h3>
              <Badge tone="changed">{change.kindLabel}</Badge>
            </div>
            <p className="mt-2 text-sm text-muted-foreground">{change.summary}</p>
          </div>

          {change.comparisonLimitation ? (
            <section
              className="mb-4 rounded-lg border border-amber-300/40 bg-amber-300/10 p-4"
              aria-label="Text comparison unavailable"
            >
              <h4 className="font-semibold" role="status">Text comparison unavailable</h4>
              <p className="mt-2 text-sm leading-6 text-muted-foreground">
                {change.comparisonLimitation === "above-line-ceiling"
                  ? "This change exceeds Mesh's 4,096-line comparison limit. Mesh is showing saved-version metadata only, not file contents. Do not treat the values below as a content diff."
                  : "One saved side is text and the other is binary, so a line comparison would be misleading. Mesh is showing saved-version metadata only, not file contents. Do not treat the values below as a content diff."}
              </p>
            </section>
          ) : null}

          {change.kind === "file" ? (
            <div className="grid gap-4">
              <section className="rounded-lg border border-border bg-muted/20 p-4" aria-label="Saved file metadata comparison">
                <h4 className="font-semibold">Exact file metadata</h4>
                <p className="mt-2 text-sm leading-6 text-muted-foreground">
                  Mesh saved and compared the exact bytes, but this file type has no specialized visual or text preview. The metadata below remains part of this review; inspect the file in its owning application when content-level review is required.
                </p>
              </section>
              <div className="grid gap-4 md:grid-cols-2">
                <ReviewPanel label={change.beforeLabel} tone="before" values={change.beforeValues} />
                <ReviewPanel label={change.afterLabel} tone="after" values={change.afterValues} />
              </div>
            </div>
          ) : model.mode === "visual" && change.kind !== "text" ? (
            <ArtifactVisualComparison
              change={change}
              preview={artifactPreview?.changeId === change.id ? artifactPreview : null}
              loading={artifactPreviewLoading}
              error={artifactPreviewError}
              canLoad={model.canRenderArtifactPreview}
              onLoad={(pageNumber) => onIntent({
                type: "load-artifact-preview",
                changeId: change.id,
                pageNumber,
              })}
            />
          ) : model.mode === "visual" ? (
            <div className="grid gap-4 md:grid-cols-2">
              <ReviewPanel label={change.beforeLabel} tone="before" values={change.beforeValues} />
              <ReviewPanel label={change.afterLabel} tone="after" values={change.afterValues} />
            </div>
          ) : (
            <ContentReview
              change={change}
              layout={model.diffLayout}
              preview={artifactPreview?.changeId === change.id ? artifactPreview : null}
              loading={artifactPreviewLoading}
              error={artifactPreviewError}
              canLoad={model.canRenderArtifactPreview}
              onLayout={(layout) => onIntent({ type: "change-diff-layout", layout })}
              onLoad={(pageNumber) => onIntent({ type: "load-artifact-preview", changeId: change.id, pageNumber })}
            />
          )}
        </section>

        <aside className="border-t border-border bg-muted/20 p-5 xl:border-l xl:border-t-0" aria-label="Review decision">
          <p className="text-xs font-semibold uppercase tracking-[0.16em] text-muted-foreground">Decision</p>
          <h3 className="mt-2 text-base font-semibold">Review this exact saved version</h3>
          <p className="mt-2 text-sm leading-6 text-muted-foreground">{model.approvalReason}</p>
          <dl className="mt-5 grid gap-4 text-sm">
            <Proof label="Saved version" value={model.versionLabel} />
            <Proof label="Review bundle" value={model.bundleLabel} />
            <Proof label="Working folder" value="Unchanged during review" />
          </dl>
          <div className="mt-6 grid gap-2">
            <Button
              variant="secondary"
              disabled={change.kind === "text" || change.kind === "file" || !model.canInspectExactCopies}
              onClick={() => onIntent({ type: "inspect-exact-copies", changeId: change.id })}
            >
              Inspect exact copies
            </Button>
            <Button
              variant="secondary"
              disabled={!model.canRecordReview}
              onClick={() => onIntent({ type: "record-review" })}
            >
              Confirm review complete
            </Button>
            {model.canExportPrivateCopy ? (
              <Button variant="secondary" onClick={() => onIntent({ type: "choose-private-export" })}>
                Choose export folder
              </Button>
            ) : null}
            <Button
              variant="primary"
              disabled={!model.canApprove}
              onClick={() => onIntent({ type: "approve-version" })}
            >
              Approve exact version
            </Button>
            {model.canApproveAndExport ? (
              <Button variant="primary" onClick={() => onIntent({ type: "approve-and-export" })}>
                Approve and create Git branch
              </Button>
            ) : null}
            {model.canExportGit ? (
              <Button variant="secondary" onClick={() => onIntent({ type: "export-git" })}>
                Create Git branch
              </Button>
            ) : null}
          </div>
          <p className="mt-4 text-xs leading-5 text-muted-foreground">
            Exporting a separate copy, confirming review, and approving are independent actions. It does not approve or change the original folder.
            Agents and web content cannot skip the native user-presence approval ceremony.
          </p>
        </aside>
      </div>
    </Card>
  );
}

function ContentReview({ change, layout, preview, loading, error, canLoad, onLayout, onLoad }: {
  change: ReviewChange;
  layout: DiffLayout;
  preview: ReviewArtifactPreview | null;
  loading: boolean;
  error: string | null;
  canLoad: boolean;
  onLayout: (layout: DiffLayout) => void;
  onLoad: (pageNumber: number) => void;
}) {
  const currentPage = preview?.requestedPage ?? 1;
  const totalPages = artifactPageCount(preview);
  const contentState = change.kind === "text"
    ? Object.freeze({ kind: "not-loaded" as const })
    : artifactContentReviewState(change, preview, error);
  const artifactContent = contentState.kind === "ready" ? contentState.comparison : null;
  const presentedChange = artifactContent
    ? Object.freeze({ ...change, diffHunks: artifactContent.hunks })
    : change;
  if (change.kind !== "text" && !artifactContent) {
    const failed = contentState.kind === "failed";
    const incompatible = contentState.kind === "incompatible";
    return (
      <div className="grid gap-3">
        <section
          className="grid gap-3 rounded-lg border border-border bg-muted/20 p-5"
          aria-label="Artifact content comparison unavailable"
        >
          <h4 className="font-semibold" role={failed ? "alert" : "status"}>
            {failed
              ? "Content comparison could not be loaded"
              : incompatible
                ? "These versions do not expose one comparable text view"
                : "Load the exact versions to compare their content"}
          </h4>
          <p className="text-sm leading-6 text-muted-foreground">
            {incompatible
              ? "Use Visual view or inspect the exact copies. Mesh will not guess across incompatible extraction sources."
              : "Mesh derives this view from bounded inert text in the exact saved artifact. It never executes document actions, formulas, links, or embedded content."}
          </p>
          {failed ? (
            <ul className="grid gap-1 text-sm text-red-100">
              {contentState.messages.map((failure) => <li key={failure}>{failure}</li>)}
            </ul>
          ) : null}
          {!incompatible ? (
            <Button className="w-fit" variant="secondary" disabled={!canLoad || loading} onClick={() => onLoad(currentPage)}>
              {loading ? "Reading exact versions…" : failed ? "Try content comparison again" : "Load content comparison"}
            </Button>
          ) : null}
        </section>
        {change.kind === "pdf" && preview && totalPages > 1 ? (
          <PdfPageNavigation
            label="PDF page content comparison"
            currentPage={currentPage}
            totalPages={totalPages}
            loading={loading}
            failed={failed}
            canLoad={canLoad}
            onLoad={onLoad}
          />
        ) : null}
      </div>
    );
  }
  return (
    <div className="grid gap-3">
      {artifactContent ? (
        <div className="rounded-lg border border-border bg-muted/20 p-4">
          <h4 className="font-semibold">{artifactContent.title}</h4>
          <p className="mt-2 text-sm leading-6 text-muted-foreground">{artifactContent.note}</p>
        </div>
      ) : null}
      {change.kind === "pdf" && preview && totalPages > 1 ? (
        <PdfPageNavigation
          label="PDF page content comparison"
          currentPage={currentPage}
          totalPages={totalPages}
          loading={loading}
          failed={false}
          canLoad={canLoad}
          onLoad={onLoad}
        />
      ) : null}
      {presentedChange.diffHunks.length > 0 ? (
        <div className="flex justify-end">
          <SegmentedControl
            proof="content-diff-layout"
            label="Content diff layout"
            options={diffLayouts}
            value={layout === "split" ? "Split" : "Inline"}
            onChange={(value) => onLayout(value === "Split" ? "split" : "inline")}
          />
        </div>
      ) : null}
      {artifactContent ? (
        <ArtifactContentChanges
          key={`${change.id}:${preview?.before?.versionId ?? "absent"}:${preview?.after?.versionId ?? "absent"}`}
          change={presentedChange}
          layout={layout}
          comparison={artifactContent}
        />
      ) : (
        <ContentChanges change={presentedChange} layout={layout} />
      )}
    </div>
  );
}

function ArtifactContentChanges({ change, layout, comparison }: {
  change: ReviewChange;
  layout: DiffLayout;
  comparison: ArtifactContentComparison;
}) {
  const [sectionIndex, setSectionIndex] = useState<number | null>(null);
  const sectionLabels = comparison.sectionLabels;
  const selectedLabel = sectionIndex === null ? null : sectionLabels[sectionIndex] ?? null;
  const selectedChange = Object.freeze({
    ...change,
    diffHunks: artifactSectionHunks(comparison, selectedLabel),
  });
  return (
    <div className="grid gap-3">
      {sectionLabels.length > 0 ? (
        <nav className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-border bg-muted/30 p-3" aria-label="Document section comparison">
          <div className="flex items-center gap-2">
            <Button
              variant="secondary"
              disabled={sectionLabels.length === 0 || sectionIndex === 0}
              onClick={() => setSectionIndex(sectionIndex === null ? sectionLabels.length - 1 : Math.max(0, sectionIndex - 1))}
            >
              Previous section
            </Button>
            <Button
              variant="secondary"
              disabled={sectionLabels.length === 0 || sectionIndex === sectionLabels.length - 1}
              onClick={() => setSectionIndex(sectionIndex === null ? 0 : Math.min(sectionLabels.length - 1, sectionIndex + 1))}
            >
              Next section
            </Button>
          </div>
          <label className="flex items-center gap-2 text-sm font-medium">
            Section
            <select
              className="max-w-64 rounded-md border border-border bg-background px-3 py-2 text-foreground"
              aria-label="Artifact section"
              value={sectionIndex === null ? "all" : String(sectionIndex)}
              onChange={(event) => setSectionIndex(event.target.value === "all" ? null : Number(event.target.value))}
            >
              <option value="all">All sections</option>
              {sectionLabels.map((label, index) => <option key={`${index}:${label}`} value={index}>{label}</option>)}
            </select>
          </label>
          <strong className="text-sm" role="status" aria-live="polite">
            {sectionIndex === null
              ? `All ${sectionLabels.length} sections`
              : `Section ${sectionIndex + 1} of ${sectionLabels.length}`}
          </strong>
        </nav>
      ) : null}
      <ContentChanges change={selectedChange} layout={layout} />
    </div>
  );
}

function ArtifactVisualComparison({
  change,
  preview,
  loading,
  error,
  canLoad,
  onLoad,
}: {
  change: ReviewChange;
  preview: ReviewArtifactPreview | null;
  loading: boolean;
  error: string | null;
  canLoad: boolean;
  onLoad: (pageNumber: number) => void;
}) {
  const currentPage = preview?.requestedPage ?? 1;
  const totalPages = artifactPageCount(preview);
  const failureMessage = [
    error,
    preview?.beforeError ? `Earlier version: ${preview.beforeError}` : null,
    preview?.afterError ? `Current version: ${preview.afterError}` : null,
  ].filter((message): message is string => message !== null).join(" ");
  return (
    <section className="grid gap-4" aria-label="Exact visual artifact comparison">
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-border bg-muted/30 p-3">
        <p className="text-sm text-muted-foreground">
          {change.kind === "pdf"
            ? "Rendered from the exact saved PDF pages."
            : "Representative macOS previews from the exact saved files; inspect exact copies for complete formatting and interactions."}
        </p>
        <Button
          variant="secondary"
          disabled={!canLoad || loading}
          onClick={() => onLoad(currentPage)}
        >
          {loading
            ? "Rendering exact versions…"
            : failureMessage
              ? "Try visual comparison again"
              : preview ? "Reload visual comparison" : "Load visual comparison"}
        </Button>
      </div>
      {failureMessage ? (
        <p
          className="rounded-lg border border-red-400/40 bg-red-400/10 p-3 text-sm text-red-100"
          role="alert"
          aria-atomic="true"
        >
          {failureMessage}
        </p>
      ) : null}
      {preview ? (
        <div className="grid gap-4 md:grid-cols-2">
          <ArtifactPreviewPanel
            label={change.beforeLabel}
            preview={preview.before}
            absentPage={preview.beforeAbsentPage}
            requestedPage={currentPage}
            error={preview.beforeError}
          />
          <ArtifactPreviewPanel
            label={change.afterLabel}
            preview={preview.after}
            absentPage={preview.afterAbsentPage}
            requestedPage={currentPage}
            error={preview.afterError}
          />
        </div>
      ) : (
        <div className="grid gap-4 md:grid-cols-2">
          <ReviewPanel label={`${change.beforeLabel} · visual not loaded`} tone="before" values={change.beforeValues} />
          <ReviewPanel label={`${change.afterLabel} · visual not loaded`} tone="after" values={change.afterValues} />
        </div>
      )}
      {change.kind === "pdf" && preview && totalPages > 1 ? (
        <PdfPageNavigation
          label="PDF page comparison"
          currentPage={currentPage}
          totalPages={totalPages}
          loading={loading}
          failed={failureMessage.length > 0}
          canLoad={canLoad}
          onLoad={onLoad}
        />
      ) : null}
    </section>
  );
}

function artifactPageCount(preview: ReviewArtifactPreview | null): number {
  return Math.max(
    preview?.before?.pageCount ?? 1,
    preview?.after?.pageCount ?? 1,
    preview?.beforeAbsentPage?.pageCount ?? 1,
    preview?.afterAbsentPage?.pageCount ?? 1,
  );
}

export function PdfPageNavigation({ label, currentPage, totalPages, loading, failed, canLoad, onLoad }: {
  label: string;
  currentPage: number;
  totalPages: number;
  loading: boolean;
  failed: boolean;
  canLoad: boolean;
  onLoad: (pageNumber: number) => void;
}) {
  const lastPreviewablePage = Math.min(totalPages, PDF_PREVIEW_PAGE_LIMIT);
  const previousUnavailable = !canLoad || loading || currentPage <= 1;
  const nextUnavailable = !canLoad || loading || currentPage >= lastPreviewablePage;
  const status = loading
    ? `Loading PDF page comparison. Page ${currentPage} of ${totalPages} remains shown.`
    : failed
      ? `PDF page comparison could not be loaded. Page ${currentPage} of ${totalPages} remains selected.`
      : `Page ${currentPage} of ${totalPages} is shown.`;
  return (
    <div className="grid gap-2 text-center">
      <nav className="flex items-center justify-center gap-3" aria-label={label}>
        <Button
          className="aria-disabled:pointer-events-none aria-disabled:opacity-45"
          variant="secondary"
          disabled={!canLoad}
          aria-disabled={previousUnavailable}
          onClick={() => {
            if (!previousUnavailable) onLoad(currentPage - 1);
          }}
        >
          Previous page
        </Button>
        <strong
          className="text-sm"
          data-mesh-proof="pdf-page-status"
          role={failed ? undefined : "status"}
          aria-live={failed ? undefined : "polite"}
          aria-atomic={failed ? undefined : "true"}
        >
          {status}
        </strong>
        <Button
          className="aria-disabled:pointer-events-none aria-disabled:opacity-45"
          variant="secondary"
          disabled={!canLoad}
          aria-disabled={nextUnavailable}
          onClick={() => {
            if (!nextUnavailable) onLoad(currentPage + 1);
          }}
        >
          Next page
        </Button>
      </nav>
      {totalPages > PDF_PREVIEW_PAGE_LIMIT ? (
        <p className="text-xs leading-5 text-muted-foreground">
          Visual preview is limited to the first {PDF_PREVIEW_PAGE_LIMIT} pages. Use Inspect exact copies to review the remaining pages in your PDF reader.
        </p>
      ) : null}
    </div>
  );
}

function ArtifactPreviewPanel({ label, preview, absentPage, requestedPage, error }: {
  label: string;
  preview: ReviewArtifactPreviewSide | null;
  absentPage: ReviewArtifactAbsentPage | null;
  requestedPage: number;
  error: string | null;
}) {
  return (
    <figure className="overflow-hidden rounded-lg border border-border bg-background">
      {preview ? (
        <img className="block max-h-[42rem] w-full bg-white object-contain" src={preview.imageDataUrl} alt={`${label} exact visual preview`} />
      ) : (
        <div className="grid min-h-48 place-items-center bg-muted/30 p-5 text-center text-sm text-muted-foreground">
          {absentPage
            ? `This saved PDF has ${absentPage.pageCount} page${absentPage.pageCount === 1 ? "" : "s"}; page ${requestedPage} is not present.`
            : error ?? `${label} is not present in this saved version.`}
        </div>
      )}
      <figcaption className="border-t border-border px-4 py-3 text-xs text-muted-foreground">
        {label}{preview?.pageNumber
          ? ` · page ${preview.pageNumber} of ${preview.pageCount}`
          : absentPage ? " · page not present in this version" : ""}
      </figcaption>
    </figure>
  );
}

function ContentChanges({ change, layout }: { change: ReviewChange; layout: DiffLayout }) {
  if (change.diffHunks.length > 0) {
    return layout === "split" ? <SplitTextDiff change={change} /> : <InlineTextDiff change={change} />;
  }
  return (
    <div className="overflow-hidden rounded-lg border border-border bg-background">
      <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] border-b border-border text-xs font-semibold text-muted-foreground">
        <span className="p-3">{change.beforeLabel}</span>
        <span className="border-l border-border p-3">{change.afterLabel}</span>
      </div>
      <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] text-sm">
        <div className="grid gap-2 bg-red-400/5 p-4 text-red-100">
          {change.beforeValues.map((value) => <span key={value}>{value}</span>)}
        </div>
        <div className="grid gap-2 border-l border-border bg-emerald-400/5 p-4 text-emerald-100">
          {change.afterValues.map((value) => <span key={value}>{value}</span>)}
        </div>
      </div>
    </div>
  );
}

function lineTone(line: ReviewDiffLine): string {
  if (line.kind === "removed") return "bg-red-400/10 text-red-100";
  if (line.kind === "added") return "bg-emerald-400/10 text-emerald-100";
  return "text-foreground";
}

function lineMark(line: ReviewDiffLine): string {
  if (line.kind === "removed") return "−";
  if (line.kind === "added") return "+";
  return " ";
}

function lineStatus(line: ReviewDiffLine): string {
  if (line.kind === "removed") return "Removed";
  if (line.kind === "added") return "Added";
  return "Unchanged";
}

function InlineTextDiff({ change }: { change: ReviewChange }) {
  return (
    <div className="overflow-hidden rounded-lg border border-border bg-background font-mono text-xs" aria-label="Inline text diff">
      {change.diffHunks.map((hunk, hunkIndex) => (
        <div key={`${hunk.beforeStart}:${hunk.afterStart}:${hunkIndex}`}>
          <div className="border-y border-border bg-muted/60 px-3 py-2 text-muted-foreground">
            <span className="sr-only">Changed lines beginning at </span>
            @@ before {hunk.beforeStart} · after {hunk.afterStart} @@
          </div>
          {hunk.lines.map((line, index) => (
            line.section ? (
              <SectionDiffHeading line={line} key={`${line.kind}:section:${line.text}:${index}`} />
            ) : <div className={`grid grid-cols-[3rem_3rem_2rem_minmax(0,1fr)] ${lineTone(line)}`} key={`${line.kind}:${line.before}:${line.after}:${index}`}>
              <span className="sr-only">{lineStatus(line)}. {line.before === null ? "No earlier line" : `Earlier line ${line.before}`}. {line.after === null ? "No current line" : `Current line ${line.after}`}. </span>
              <span aria-hidden="true" className="border-r border-border px-2 py-1 text-right text-muted-foreground">{line.before ?? ""}</span>
              <span aria-hidden="true" className="border-r border-border px-2 py-1 text-right text-muted-foreground">{line.after ?? ""}</span>
              <span className="px-2 py-1 text-center" aria-hidden="true">{lineMark(line)}</span>
              <span className="whitespace-pre-wrap break-words px-2 py-1">{line.text || " "}</span>
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}

function SplitTextDiff({ change }: { change: ReviewChange }) {
  return (
    <div className="overflow-x-auto rounded-lg border border-border bg-background font-mono text-xs" aria-label="Split text diff">
      <div className="grid min-w-[46rem] grid-cols-2 border-b border-border font-sans text-xs font-semibold text-muted-foreground">
        <span className="px-3 py-2">{change.beforeLabel}</span>
        <span className="border-l border-border px-3 py-2">{change.afterLabel}</span>
      </div>
      {change.diffHunks.flatMap((hunk, hunkIndex) => hunk.lines.map((line, index) => {
        if (line.section) {
          return <SectionDiffHeading line={line} key={`${hunkIndex}:section:${line.text}:${index}`} />;
        }
        const before = line.kind === "added" ? null : line;
        const after = line.kind === "removed" ? null : line;
        return (
          <div className="grid min-w-[46rem] grid-cols-2" key={`${hunkIndex}:${index}:${line.kind}`}>
            <DiffSide side="Earlier" line={before} number={line.before} emptyTone={line.kind === "added" ? "added" : "context"} />
            <DiffSide side="Current" line={after} number={line.after} emptyTone={line.kind === "removed" ? "removed" : "context"} right />
          </div>
        );
      }))}
    </div>
  );
}

function SectionDiffHeading({ line }: { line: ReviewDiffLine }) {
  return (
    <div className={`flex items-center justify-between gap-3 border-y border-border px-4 py-3 font-sans ${lineTone(line)}`}>
      <h5 className="font-semibold">{line.text}</h5>
      <span className="text-xs font-medium">{lineStatus(line)} section</span>
    </div>
  );
}

function DiffSide({ side, line, number, emptyTone, right = false }: {
  side: "Earlier" | "Current";
  line: ReviewDiffLine | null;
  number: number | null;
  emptyTone: ReviewDiffLine["kind"];
  right?: boolean;
}) {
  const tone = line ? lineTone(line) : emptyTone === "added" ? "bg-emerald-400/5" : emptyTone === "removed" ? "bg-red-400/5" : "";
  return (
    <div className={`grid grid-cols-[3rem_2rem_minmax(0,1fr)] ${right ? "border-l border-border" : ""} ${tone}`}>
      <span className="sr-only">{line ? `${side} line ${number ?? "without a number"}. ${lineStatus(line)}. ` : `No corresponding ${side.toLowerCase()} line. `}</span>
      <span aria-hidden="true" className="border-r border-border px-2 py-1 text-right text-muted-foreground">{number ?? ""}</span>
      <span className="px-2 py-1 text-center" aria-hidden="true">{line ? lineMark(line) : ""}</span>
      <span className="whitespace-pre-wrap break-words px-2 py-1">{line?.text ?? " "}</span>
    </div>
  );
}

function ReviewPanel({ label, tone, values }: { label: string; tone: "before" | "after"; values: readonly string[] }) {
  return (
    <figure className="overflow-hidden rounded-lg border border-border bg-background">
      <figcaption className="border-b border-border px-4 py-3 text-xs font-semibold text-muted-foreground">{label}</figcaption>
      <div className="grid grid-cols-[1.5fr_1fr_1fr] text-sm">
        {values.map((value, index) => (
          <div key={value} className={index === 2 ? (tone === "after" ? "bg-emerald-400/10 p-4 text-emerald-100" : "bg-red-400/10 p-4 text-red-100") : "p-4"}>
            {value}
          </div>
        ))}
      </div>
    </figure>
  );
}

function Proof({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="mt-1 break-all font-mono text-xs text-foreground">{value}</dd>
    </div>
  );
}
