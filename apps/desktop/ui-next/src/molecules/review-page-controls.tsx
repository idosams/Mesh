import { Button } from "../atoms/button";
import type { ReviewPageControls as ReviewPageControlsModel } from "../models/review-page";
import type { ReviewWorkbenchIntent } from "../models/review-workbench";

export function ReviewPageControls({ controls, onIntent }: Readonly<{
  controls: ReviewPageControlsModel;
  onIntent: (intent: ReviewWorkbenchIntent) => void;
}>) {
  return (
    <section className="flex flex-col gap-3 rounded-lg border border-border bg-muted/20 p-4" aria-label="Review workspace summary">
      <div>
        <p className="text-sm font-semibold">{controls.countLabel}</p>
        {controls.overflowLabel ? (
          <p className="mt-1 text-xs leading-5 text-muted-foreground">{controls.overflowLabel}</p>
        ) : null}
      </div>
      <div className="flex flex-wrap gap-2">
        <Button
          variant="secondary"
          disabled={!controls.canSetupApproval}
          title={controls.canSetupApproval ? undefined : controls.setupApprovalReason}
          onClick={() => onIntent({ type: "setup-approval" })}
        >
          {controls.setupApprovalLabel}
        </Button>
        <Button
          variant="secondary"
          disabled={!controls.canRecordReview}
          title={controls.canRecordReview ? undefined : controls.recordReviewReason}
          onClick={() => onIntent({ type: "record-review" })}
        >
          {controls.recordReviewLabel}
        </Button>
      </div>
      {controls.earlierReviews.length > 0 ? (
        <div className="grid gap-2 border-t border-border pt-3" aria-label="Earlier recorded reviews">
          <p className="text-xs font-semibold uppercase tracking-[0.14em] text-muted-foreground">Earlier saved reviews</p>
          {controls.earlierReviews.map((review) => (
            <Button
              key={review.operation}
              variant="quiet"
              disabled={!review.canOpen}
              onClick={() => onIntent({ type: "open-earlier-review", operation: review.operation })}
            >
              {review.label}
            </Button>
          ))}
        </div>
      ) : null}
    </section>
  );
}
