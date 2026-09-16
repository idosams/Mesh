export type ArtifactKind = "text" | "pdf" | "presentation" | "document" | "spreadsheet";

export type ReviewMode = "visual" | "content";

export type DiffLayout = "split" | "inline";

export type ReviewComparisonLimitation = "above-line-ceiling" | "content-class-changed";

export type ReviewDiffLine = Readonly<{
  kind: "context" | "removed" | "added";
  before: number | null;
  after: number | null;
  text: string;
  section?: boolean;
}>;

export type ReviewDiffHunk = Readonly<{
  beforeStart: number;
  afterStart: number;
  lines: readonly ReviewDiffLine[];
}>;

export type ReviewChange = {
  id: string;
  path: string;
  kind: ArtifactKind;
  kindLabel: string;
  summary: string;
  impact: string;
  beforeLabel: string;
  afterLabel: string;
  beforeValues: readonly string[];
  afterValues: readonly string[];
  diffHunks: readonly ReviewDiffHunk[];
  comparisonLimitation: ReviewComparisonLimitation | null;
  beforeVersionId: string | null;
  beforeContentDigest: string | null;
  afterVersionId: string | null;
  afterContentDigest: string | null;
};

export type ReviewArtifactPreviewSide = Readonly<{
  side: "before" | "after";
  versionId: string;
  contentDigest: string;
  imageDataUrl: string;
  pageNumber: number | null;
  pageCount: number | null;
  textSource: string | null;
  textLines: readonly string[] | null;
  textSections: readonly ReviewArtifactTextSection[] | null;
  textTruncated: boolean;
}>;

export type ReviewArtifactAbsentPage = Readonly<{
  side: "before" | "after";
  versionId: string;
  contentDigest: string;
  pageCount: number;
}>;

export type ReviewArtifactTextSection = Readonly<{
  label: string;
  lineStart: number;
  lineCount: number;
}>;

export type ReviewArtifactPreview = Readonly<{
  changeId: string;
  requestedPage: number;
  before: ReviewArtifactPreviewSide | null;
  after: ReviewArtifactPreviewSide | null;
  beforeAbsentPage: ReviewArtifactAbsentPage | null;
  afterAbsentPage: ReviewArtifactAbsentPage | null;
  beforeError: string | null;
  afterError: string | null;
}>;

export type ReviewWorkbenchModel = {
  workspaceName: string;
  versionLabel: string;
  bundleLabel: string;
  changes: readonly ReviewChange[];
  selectedChangeId: string;
  mode: ReviewMode;
  diffLayout: DiffLayout;
  canRenderArtifactPreview: boolean;
  canInspectExactCopies: boolean;
  canRecordReview: boolean;
  canApprove: boolean;
  canApproveAndExport: boolean;
  canExportGit: boolean;
  canExportPrivateCopy: boolean;
  approvalReason: string;
};

export type ReviewWorkbenchIntent =
  | { type: "select-change"; changeId: string }
  | { type: "change-mode"; mode: ReviewMode }
  | { type: "change-diff-layout"; layout: DiffLayout }
  | { type: "load-artifact-preview"; changeId: string; pageNumber: number }
  | { type: "inspect-exact-copies"; changeId: string }
  | { type: "record-review" }
  | { type: "setup-approval" }
  | { type: "approve-version" }
  | { type: "approve-and-export" }
  | { type: "export-git" }
  | { type: "open-earlier-review"; operation: string }
  | { type: "choose-private-export" };

export function selectedReviewChange(model: ReviewWorkbenchModel): ReviewChange {
  const selected = model.changes.find((change) => change.id === model.selectedChangeId);
  if (!selected) {
    throw new Error("The selected review change is not in the frozen review model.");
  }
  return selected;
}

export function reduceReviewWorkbench(
  model: ReviewWorkbenchModel,
  intent: ReviewWorkbenchIntent,
): ReviewWorkbenchModel {
  if (intent.type === "select-change") {
    const selected = model.changes.find((change) => change.id === intent.changeId);
    if (!selected) {
      throw new Error("The selected review change is not in the frozen review model.");
    }
    return Object.freeze({
      ...model,
      selectedChangeId: intent.changeId,
      // A text/code change with exact hunks must never land on the summary-only visual fallback
      // merely because the previously selected Office/PDF artifact preferred Visual view.
      mode: selected.kind === "text" ? "content" : model.mode,
    });
  }
  if (intent.type === "change-mode") {
    return Object.freeze({ ...model, mode: intent.mode });
  }
  if (intent.type === "change-diff-layout") {
    return Object.freeze({ ...model, diffLayout: intent.layout });
  }
  // Native intents are deliberately not reduced into optimistic presentation state. The legacy
  // coordinator must execute the exact operation and install a newly verified projection.
  return model;
}

export function reconcileReviewWorkbenchProjection(
  current: ReviewWorkbenchModel,
  source: ReviewWorkbenchModel,
): ReviewWorkbenchModel {
  const selected = source.changes.find((change) => change.id === current.selectedChangeId);
  if (!selected) return source;
  return Object.freeze({
    ...source,
    // Selection and presentation are local review choices, not native authority. Keep them while
    // the exact bundle is unchanged, but take every action bit and every content row from source.
    selectedChangeId: selected.id,
    mode: selected.kind === "text" ? "content" : current.mode,
    diffLayout: current.diffLayout,
  });
}
