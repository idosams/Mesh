import { reviewWorkbenchEnvelope } from "./review-workbench-envelope";
import type { ReviewWorkbenchModel } from "./review-workbench";

export type ReviewPageStatusState = "empty" | "unavailable";

export type ReviewPageStatusModel = Readonly<{
  state: ReviewPageStatusState;
  title: string;
  description: string;
}>;

export type ReviewPageControls = Readonly<{
  countLabel: string;
  overflowLabel: string | null;
  canSetupApproval: boolean;
  setupApprovalLabel: string;
  setupApprovalReason: string;
  canRecordReview: boolean;
  recordReviewLabel: string;
  recordReviewReason: string;
  earlierReviews: readonly Readonly<{
    operation: string;
    label: string;
    canOpen: boolean;
  }>[];
}>;

export type ReviewPageEnvelope =
  | Readonly<{
      generation: number;
      state: "ready";
      bundle: string;
      model: ReviewWorkbenchModel;
      controls: ReviewPageControls;
    }>
  | Readonly<{
      generation: number;
      state: ReviewPageStatusState;
      bundle: null;
      model: ReviewPageStatusModel;
      controls: ReviewPageControls;
    }>;

type JsonRecord = Record<string, unknown>;

function record(value: unknown, label: string): JsonRecord {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} was not one object.`);
  }
  return value as JsonRecord;
}

function exactKeys(value: JsonRecord, expected: readonly string[], label: string): void {
  const actual = Object.keys(value).sort();
  const canonical = [...expected].sort();
  if (actual.length !== canonical.length || actual.some((key, index) => key !== canonical[index])) {
    throw new Error(`${label} had unrecognized or missing fields.`);
  }
}

const unsafeText = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u;

function safeText(value: unknown, label: string, maximum: number): string {
  if (typeof value !== "string" || value.length === 0 || value.length > maximum || unsafeText.test(value)) {
    throw new Error(`${label} was unsafe or unbounded.`);
  }
  return value;
}

function reviewPageControls(value: unknown): ReviewPageControls {
  const controls = record(value, "review page controls");
  exactKeys(controls, [
    "canRecordReview",
    "canSetupApproval",
    "countLabel",
    "earlierReviews",
    "overflowLabel",
    "recordReviewLabel",
    "recordReviewReason",
    "setupApprovalLabel",
    "setupApprovalReason",
  ], "review page controls");
  if (typeof controls.canSetupApproval !== "boolean"
    || typeof controls.canRecordReview !== "boolean"
    || !Array.isArray(controls.earlierReviews)
    || controls.earlierReviews.length > 63) {
    throw new Error("The coordinator supplied malformed review page authority.");
  }
  const earlierReviews = Object.freeze(controls.earlierReviews.map((value, index) => {
    const review = record(value, `earlier review ${index}`);
    exactKeys(review, ["canOpen", "label", "operation"], `earlier review ${index}`);
    const operation = safeText(review.operation, `earlier review ${index} operation`, 64);
    if (!/^[0-9a-f]{64}$/u.test(operation) || typeof review.canOpen !== "boolean") {
      throw new Error(`Earlier review ${index} was malformed.`);
    }
    return Object.freeze({
      operation,
      label: safeText(review.label, `earlier review ${index} label`, 240),
      canOpen: review.canOpen,
    });
  }));
  if (new Set(earlierReviews.map((review) => review.operation)).size !== earlierReviews.length) {
    throw new Error("The review page repeated an earlier saved point.");
  }
  return Object.freeze({
    countLabel: safeText(controls.countLabel, "review count", 240),
    overflowLabel: controls.overflowLabel === null
      ? null
      : safeText(controls.overflowLabel, "review overflow", 1_024),
    canSetupApproval: controls.canSetupApproval,
    setupApprovalLabel: safeText(controls.setupApprovalLabel, "approval setup label", 240),
    setupApprovalReason: safeText(controls.setupApprovalReason, "approval setup reason", 1_024),
    canRecordReview: controls.canRecordReview,
    recordReviewLabel: safeText(controls.recordReviewLabel, "record review label", 240),
    recordReviewReason: safeText(controls.recordReviewReason, "record review reason", 1_024),
    earlierReviews,
  });
}

function generation(value: unknown, previousGeneration: number): number {
  if (!Number.isSafeInteger(value) || (value as number) <= previousGeneration) {
    throw new Error("The review page envelope was stale or had no canonical generation.");
  }
  return value as number;
}

export function reviewPageEnvelope(value: unknown, previousGeneration: number): ReviewPageEnvelope {
  const envelope = record(value, "review page envelope");
  const state = envelope.state;
  if (state === "ready") {
    exactKeys(envelope, [
      "authority", "controls", "generation", "projection", "state", "versionLabel", "workspaceName",
    ], "ready review page envelope");
    const accepted = reviewWorkbenchEnvelope({
      authority: envelope.authority,
      generation: envelope.generation,
      projection: envelope.projection,
      versionLabel: envelope.versionLabel,
      workspaceName: envelope.workspaceName,
    }, previousGeneration);
    const controls = reviewPageControls(envelope.controls);
    if (controls.canRecordReview !== accepted.model.canRecordReview) {
      throw new Error("Review page and workbench record authority disagreed.");
    }
    return Object.freeze({ ...accepted, state: "ready", controls });
  }
  if (state !== "empty" && state !== "unavailable") {
    throw new Error("The review page state was not recognized.");
  }
  exactKeys(envelope, ["controls", "generation", "state", "status"], "review page status envelope");
  const exactGeneration = generation(envelope.generation, previousGeneration);
  const status = record(envelope.status, "review page status");
  exactKeys(status, ["description", "title"], "review page status");
  const model = Object.freeze({
    state,
    title: safeText(status.title, "review page status title", 240),
    description: safeText(status.description, "review page status description", 4_096),
  });
  return Object.freeze({
    generation: exactGeneration,
    state,
    bundle: null,
    model,
    controls: reviewPageControls(envelope.controls),
  });
}
