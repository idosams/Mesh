import {
  reviewWorkbenchFromProjection,
  type ReviewWorkbenchAuthority,
} from "./review-workbench-adapter";
import type { ReviewWorkbenchModel } from "./review-workbench";

type JsonRecord = Record<string, unknown>;

export type ReviewWorkbenchEnvelope = Readonly<{
  generation: number;
  bundle: string;
  model: ReviewWorkbenchModel;
}>;

function record(value: unknown, label: string): JsonRecord {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} was not one object.`);
  }
  return value as JsonRecord;
}

function exactKeys(value: JsonRecord, expected: readonly string[], label: string): void {
  const actual = Object.keys(value).sort();
  const canonical = [...expected].sort();
  if (actual.length !== canonical.length
    || actual.some((key, index) => key !== canonical[index])) {
    throw new Error(`${label} had unrecognized or missing fields.`);
  }
}

function boundedText(value: unknown, label: string): string {
  if (typeof value !== "string" || value.length === 0 || value.length > 1_024
    || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(value)) {
    throw new Error(`${label} was not safe bounded text.`);
  }
  return value;
}

export function reviewWorkbenchEnvelope(
  value: unknown,
  previousGeneration: number,
): ReviewWorkbenchEnvelope {
  const envelope = record(value, "review workbench envelope");
  exactKeys(envelope, [
    "authority",
    "generation",
    "projection",
    "versionLabel",
    "workspaceName",
  ], "review workbench envelope");
  const generation = envelope.generation;
  if (!Number.isSafeInteger(generation) || (generation as number) <= previousGeneration) {
    throw new Error("The review workbench envelope was stale or had no canonical generation.");
  }
  const projection = record(envelope.projection, "review projection");
  const bundle = boundedText(projection.bundle, "review bundle");
  if (!/^[0-9a-f]{64}$/u.test(bundle)) throw new Error("The review bundle was not canonical.");
  const authorityRecord = record(envelope.authority, "review authority");
  exactKeys(authorityRecord, [
    "approvalReason",
    "canApprove",
    "canApproveAndExport",
    "canExportGit",
    "canExportPrivateCopy",
    "canInspectExactCopies",
    "canRecordReview",
    "canRenderArtifactPreview",
  ], "review authority");
  if (typeof authorityRecord.canRenderArtifactPreview !== "boolean"
    || typeof authorityRecord.canInspectExactCopies !== "boolean"
    || typeof authorityRecord.canRecordReview !== "boolean"
    || typeof authorityRecord.canApprove !== "boolean"
    || typeof authorityRecord.canApproveAndExport !== "boolean"
    || typeof authorityRecord.canExportGit !== "boolean"
    || typeof authorityRecord.canExportPrivateCopy !== "boolean") {
    throw new Error("The coordinator supplied malformed review authority.");
  }
  const authority: ReviewWorkbenchAuthority = {
    canRenderArtifactPreview: authorityRecord.canRenderArtifactPreview as boolean,
    canInspectExactCopies: authorityRecord.canInspectExactCopies as boolean,
    canRecordReview: authorityRecord.canRecordReview as boolean,
    canApprove: authorityRecord.canApprove as boolean,
    canApproveAndExport: authorityRecord.canApproveAndExport as boolean,
    canExportGit: authorityRecord.canExportGit as boolean,
    canExportPrivateCopy: authorityRecord.canExportPrivateCopy as boolean,
    approvalReason: boundedText(authorityRecord.approvalReason, "review authority reason"),
  };
  const model = reviewWorkbenchFromProjection(
    boundedText(envelope.workspaceName, "workspace name"),
    boundedText(envelope.versionLabel, "version label"),
    projection,
    authority,
  );
  return Object.freeze({ generation: generation as number, bundle, model });
}
