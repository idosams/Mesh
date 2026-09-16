import { useState } from "react";
import { Badge } from "../atoms/badge";
import { Button } from "../atoms/button";
import { Card, CardContent } from "../atoms/card";
import { WorkspaceShell } from "../layouts/workspace-shell";
import { WorkspaceCommandBar } from "../molecules/workspace-command-bar";
import { ArtifactReview } from "../organisms/artifact-review";
import { reviewWorkbenchFromProjection } from "../models/review-workbench-adapter";
import type {
  ReviewWorkbenchIntent,
  ReviewWorkbenchModel,
} from "../models/review-workbench";
import { reduceReviewWorkbench } from "../models/review-workbench";

const initialReview: ReviewWorkbenchModel = reviewWorkbenchFromProjection(
  "Compensation planning",
  "4b7c5f1…e91",
  {
    bundle: "a1".repeat(32),
    subject_operation: "b2".repeat(32),
    content_complete: true,
    projection_authorizes_approval: false,
    bundle_changes_not_listed: 0,
    bundle_changes: [
      {
        object_id: "c3".repeat(16),
        path_before: "FY27 workforce plan.xlsx",
        path_after: "FY27 workforce plan.xlsx",
        effect: "content-written",
        body: "binary",
        before: { kind: "binary", version_id: "d4".repeat(32), content_digest: "e5".repeat(32), byte_length: "1420000", line_count: null },
        after: { kind: "binary", version_id: "f6".repeat(32), content_digest: "07".repeat(32), byte_length: "1600000", line_count: null },
        verified_text: null,
      },
      {
        object_id: "18".repeat(16),
        path_before: "Benefits renewal.pdf",
        path_after: "Benefits renewal.pdf",
        effect: "content-written",
        body: "binary",
        before: { kind: "binary", version_id: "29".repeat(32), content_digest: "3a".repeat(32), byte_length: "420000", line_count: null },
        after: { kind: "binary", version_id: "4b".repeat(32), content_digest: "5c".repeat(32), byte_length: "465000", line_count: null },
        verified_text: null,
      },
      {
        object_id: "6d".repeat(16),
        path_before: "People review.pptx",
        path_after: "People review.pptx",
        effect: "content-written",
        body: "binary",
        before: { kind: "binary", version_id: "7e".repeat(32), content_digest: "8f".repeat(32), byte_length: "910000", line_count: null },
        after: { kind: "binary", version_id: "90".repeat(32), content_digest: "a1".repeat(32), byte_length: "930000", line_count: null },
        verified_text: null,
      },
      {
        object_id: "b2".repeat(16),
        path_before: "src/benefits.ts",
        path_after: "src/benefits.ts",
        effect: "content-written",
        body: "text",
        before: { kind: "text", version_id: "c3".repeat(32), content_digest: null, byte_length: null, line_count: "3" },
        after: { kind: "text", version_id: "d4".repeat(32), content_digest: null, byte_length: null, line_count: "4" },
        verified_text: {
          source: "before-after",
          before: { version_id: "c3".repeat(32), content_digest: "e5".repeat(32) },
          after: { version_id: "d4".repeat(32), content_digest: "f6".repeat(32) },
          hunks: [{
            before_start: 1,
            before_len: 3,
            after_start: 1,
            after_len: 4,
            lines: [
              { kind: "context", before: 1, after: 1, text: "export function annualBenefit() {" },
              { kind: "removed", before: 2, after: null, text: "  return 1200;" },
              { kind: "added", before: null, after: 2, text: "  const monthly = 125;" },
              { kind: "added", before: null, after: 3, text: "  return monthly * 12;" },
              { kind: "context", before: 3, after: 4, text: "}" },
            ],
          }],
        },
      },
    ],
  },
  {
  canRenderArtifactPreview: false,
  canInspectExactCopies: false,
  canRecordReview: false,
  canApprove: false,
  canApproveAndExport: false,
  canExportGit: false,
  canExportPrivateCopy: false,
  approvalReason: "Gallery only. Native review and approval authority are intentionally disconnected.",
  },
);

export function GalleryPage() {
  const [review, setReview] = useState(initialReview);
  const handleIntent = (intent: ReviewWorkbenchIntent) => {
    setReview((current) => reduceReviewWorkbench(current, intent));
  };

  return (
    <WorkspaceShell
      contextLabel="Interface preview"
      status={{ state: "preview", label: "Preview only" }}
    >
      <div className="grid gap-6">
        <section className="max-w-3xl">
          <p className="text-xs font-semibold uppercase tracking-[0.18em] text-primary">UI foundation · isolated prototype</p>
          <h1 className="mt-3 text-4xl font-semibold tracking-[-0.04em] sm:text-5xl">Review work, not implementation details.</h1>
          <p className="mt-4 text-base leading-7 text-muted-foreground">A component-first Mesh surface for finance, people, operations, and development teams. This gallery is intentionally disconnected from native commands; the production review island delegates every native action to the established coordinator.</p>
        </section>

        <WorkspaceCommandBar />
        <ArtifactReview
          model={review}
          controls={{
            countLabel: "1 example review",
            overflowLabel: null,
            canSetupApproval: false,
            setupApprovalLabel: "Approval unavailable",
            setupApprovalReason: "Gallery only.",
            canRecordReview: false,
            recordReviewLabel: "Record reviewed version",
            recordReviewReason: "Gallery only.",
            earlierReviews: [],
          }}
          onIntent={handleIntent}
        />

        <Card>
          <CardContent>
            <div className="flex flex-wrap items-center gap-3">
              <span className="text-xs font-semibold uppercase tracking-[0.16em] text-muted-foreground">Atoms</span>
              <Button variant="primary">Primary action</Button>
              <Button variant="secondary">Secondary action</Button>
              <Button variant="quiet">Quiet action</Button>
              <Button variant="danger">Destructive action</Button>
              <Badge tone="positive">Saved</Badge>
              <Badge tone="warning">Needs attention</Badge>
              <Badge tone="changed">Changed</Badge>
            </div>
          </CardContent>
        </Card>
      </div>
    </WorkspaceShell>
  );
}
