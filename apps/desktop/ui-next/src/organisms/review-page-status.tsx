import { useTranslation } from "../lib/localization";
import { Badge } from "../atoms/badge";
import { Card } from "../atoms/card";
import type { ReviewPageStatusModel } from "../models/review-page";
import type { ReviewPageControls as ReviewPageControlsModel } from "../models/review-page";
import type { ReviewWorkbenchIntent } from "../models/review-workbench";
import { ReviewPageControls } from "../molecules/review-page-controls";

export function ReviewPageStatus({ model, controls, onIntent }: Readonly<{
  model: ReviewPageStatusModel;
  controls?: ReviewPageControlsModel;
  onIntent?: (intent: ReviewWorkbenchIntent) => void;
}>) {
  const t = useTranslation();
  const safeControls = controls ?? Object.freeze({
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
  return (
    <Card
      aria-label={t("Review status")}
      data-mesh-proof={`review-${model.state}`}
      className="grid min-h-64 place-content-center gap-4 p-6 text-center"
    >
      <div><Badge tone="neutral">{t("Review")}</Badge></div>
      <h2 className="text-xl font-semibold tracking-tight">{t(model.title)}</h2>
      <p className="mx-auto max-w-2xl text-sm leading-6 text-muted-foreground" role="status">
        {t(model.description)}
      </p>
      <ReviewPageControls controls={safeControls} onIntent={onIntent ?? (() => undefined)} />
    </Card>
  );
}
