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

export type LiveReviewChange = Readonly<{
  path: string;
  kind: "modified-file" | "new-file" | "new-folder" | "missing-file" | "unsupported";
}>;

export type LiveReviewWorkspace = Readonly<{
  path: string;
  label: string;
  state: "current" | "agent-assigned" | "available";
  canOpen: boolean;
}>;

export type LiveReviewModel = Readonly<{
  available: boolean;
  state: "idle" | "scanning" | "ready" | "error";
  summary: string;
  workspaceRoot: string;
  changes: readonly LiveReviewChange[];
  workspaces: readonly LiveReviewWorkspace[];
}>;

export type LiveReviewFilePreview = Readonly<{
  path: string;
  kind: "modified-file" | "new-file";
  byteCount: number;
  contentDigest: string;
  executable: boolean;
  text: string | null;
  previewKind: "text" | "image" | "artifact" | "metadata";
  imageDataUrl: string | null;
  previewError: string | null;
}>;

export type LiveReviewFileResult = Readonly<{
  path: string;
  preview: LiveReviewFilePreview | null;
  error: string | null;
}>;

export const LIVE_REVIEW_ROW_LIMIT = 500;

export type LiveReviewChangeProjection = Readonly<{
  changes: readonly LiveReviewChange[];
  matched: number;
  truncated: boolean;
}>;

export function liveReviewChangeProjection(
  changes: readonly LiveReviewChange[],
  filterText: string,
  maximum = LIVE_REVIEW_ROW_LIMIT,
): LiveReviewChangeProjection {
  if (!Number.isSafeInteger(maximum) || maximum < 1) throw new Error("The visible live-change bound was invalid.");
  const query = filterText.trim().toLocaleLowerCase();
  const matches = changes.filter((change) => !query || change.path.toLocaleLowerCase().includes(query));
  const visible = matches.slice(0, maximum);
  return Object.freeze({
    changes: Object.freeze(visible),
    matched: matches.length,
    truncated: matches.length > visible.length,
  });
}

export type ReviewPageEnvelope =
  | Readonly<{
      generation: number;
      state: "ready";
      bundle: string;
      model: ReviewWorkbenchModel;
      controls: ReviewPageControls;
      live: LiveReviewModel;
    }>
  | Readonly<{
      generation: number;
      state: ReviewPageStatusState;
      bundle: null;
      model: ReviewPageStatusModel;
      controls: ReviewPageControls;
      live: LiveReviewModel;
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

function liveReview(value: unknown): LiveReviewModel {
  const live = record(value, "live review");
  exactKeys(live, ["available", "changes", "state", "summary", "workspaceRoot", "workspaces"], "live review");
  if (typeof live.available !== "boolean"
    || !["idle", "scanning", "ready", "error"].includes(live.state as string)
    || !Array.isArray(live.changes)
    || live.changes.length > 10_000
    || !Array.isArray(live.workspaces)
    || live.workspaces.length > 8) {
    throw new Error("The coordinator supplied malformed live review state.");
  }
  const changes = Object.freeze(live.changes.map((candidate, index) => {
    const change = record(candidate, `live review change ${index}`);
    exactKeys(change, ["kind", "path"], `live review change ${index}`);
    if (!["modified-file", "new-file", "new-folder", "missing-file", "unsupported"].includes(change.kind as string)) {
      throw new Error("The live review contained an unknown change kind.");
    }
    return Object.freeze({
      path: safeText(change.path, `live review change ${index} path`, 4_096),
      kind: change.kind as LiveReviewChange["kind"],
    });
  }));
  if (new Set(changes.map((change) => `${change.kind}\0${change.path}`)).size !== changes.length) {
    throw new Error("The live review repeated a change.");
  }
  const workspaces = Object.freeze(live.workspaces.map((candidate, index) => {
    const workspace = record(candidate, `live review workspace ${index}`);
    exactKeys(workspace, ["canOpen", "label", "path", "state"], `live review workspace ${index}`);
    if (typeof workspace.canOpen !== "boolean"
      || !["current", "agent-assigned", "available"].includes(workspace.state as string)) {
      throw new Error("The live review contained a malformed workspace choice.");
    }
    return Object.freeze({
      path: safeText(workspace.path, `live review workspace ${index} path`, 4_096),
      label: safeText(workspace.label, `live review workspace ${index} label`, 512),
      state: workspace.state as LiveReviewWorkspace["state"],
      canOpen: workspace.canOpen,
    });
  }));
  if (new Set(workspaces.map((workspace) => workspace.path)).size !== workspaces.length) {
    throw new Error("The live review repeated a workspace.");
  }
  return Object.freeze({
    available: live.available,
    state: live.state as LiveReviewModel["state"],
    summary: safeText(live.summary, "live review summary", 2_048),
    workspaceRoot: live.workspaceRoot === "" ? "" : safeText(live.workspaceRoot, "live review workspace root", 4_096),
    changes,
    workspaces,
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
      "authority", "controls", "generation", "live", "projection", "state", "versionLabel", "workspaceName",
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
    return Object.freeze({ ...accepted, state: "ready", controls, live: liveReview(envelope.live) });
  }
  if (state !== "empty" && state !== "unavailable") {
    throw new Error("The review page state was not recognized.");
  }
  exactKeys(envelope, ["controls", "generation", "live", "state", "status"], "review page status envelope");
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
    live: liveReview(envelope.live),
  });
}

export function liveReviewFileEnvelope(
  value: unknown,
  expectedGeneration: number,
  live: LiveReviewModel,
): LiveReviewFileResult {
  const envelope = record(value, "live file preview envelope");
  exactKeys(envelope, ["error", "generation", "path", "snapshot"], "live file preview envelope");
  if (envelope.generation !== expectedGeneration) throw new Error("The live file preview was stale.");
  const path = safeText(envelope.path, "live file preview path", 4_096);
  if (!live.changes.some((change) => change.path === path && ["modified-file", "new-file"].includes(change.kind))) {
    throw new Error("The live file preview did not match a current file change.");
  }
  if (envelope.snapshot === null) {
    return Object.freeze({
      path,
      preview: null,
      error: safeText(envelope.error, "live file preview error", 1_024),
    });
  }
  if (envelope.error !== null) throw new Error("The live file preview carried conflicting state.");
  const snapshot = record(envelope.snapshot, "live file snapshot");
  exactKeys(snapshot, [
    "byteCount", "contentDigest", "executable", "imageDataUrl", "kind", "path",
    "previewError", "previewKind", "text",
  ], "live file snapshot");
  if (snapshot.path !== path
    || !["modified-file", "new-file"].includes(snapshot.kind as string)
    || !Number.isSafeInteger(snapshot.byteCount)
    || (snapshot.byteCount as number) < 0
    || typeof snapshot.executable !== "boolean"
    || typeof snapshot.contentDigest !== "string"
    || !/^[0-9a-f]{64}$/u.test(snapshot.contentDigest)
    || !["text", "image", "artifact", "metadata"].includes(snapshot.previewKind as string)
    || (snapshot.imageDataUrl !== null && (typeof snapshot.imageDataUrl !== "string"
      || !snapshot.imageDataUrl.startsWith("data:image/")
      || snapshot.imageDataUrl.length > 12 * 1024 * 1024))
    || (snapshot.previewError !== null && (typeof snapshot.previewError !== "string" || snapshot.previewError.length > 1_024))
    || (snapshot.text !== null && (typeof snapshot.text !== "string" || snapshot.text.length > 1_048_576))) {
    throw new Error("The live file snapshot was malformed.");
  }
  return Object.freeze({
    path,
    preview: Object.freeze({
      path,
      kind: snapshot.kind as LiveReviewFilePreview["kind"],
      byteCount: snapshot.byteCount as number,
      contentDigest: snapshot.contentDigest,
      executable: snapshot.executable,
      text: snapshot.text as string | null,
      previewKind: snapshot.previewKind as LiveReviewFilePreview["previewKind"],
      imageDataUrl: snapshot.imageDataUrl as string | null,
      previewError: snapshot.previewError as string | null,
    }),
    error: null,
  });
}
