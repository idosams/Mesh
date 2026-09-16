import type {
  ArtifactKind,
  ReviewComparisonLimitation,
  ReviewChange,
  ReviewDiffHunk,
  ReviewWorkbenchModel,
} from "./review-workbench";

export type ReviewWorkbenchAuthority = Readonly<{
  canRenderArtifactPreview: boolean;
  canInspectExactCopies: boolean;
  canRecordReview: boolean;
  canApprove: boolean;
  canApproveAndExport: boolean;
  canExportGit: boolean;
  canExportPrivateCopy: boolean;
  approvalReason: string;
}>;

type JsonRecord = Record<string, unknown>;

const artifactKinds = Object.freeze<Record<string, Exclude<ArtifactKind, "text">>>({
  pdf: "pdf",
  pptx: "presentation",
  docx: "document",
  xlsx: "spreadsheet",
});

const artifactLabels: Readonly<Record<ArtifactKind, string>> = Object.freeze({
  text: "Text",
  pdf: "PDF",
  presentation: "PowerPoint",
  document: "Word",
  spreadsheet: "Excel",
});

function record(value: unknown, label: string): JsonRecord {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} was not one object.`);
  }
  return value as JsonRecord;
}

function text(value: unknown, label: string): string {
  if (typeof value !== "string" || value.length === 0 || value.length > 1_024) {
    throw new Error(`${label} was not bounded text.`);
  }
  return value;
}

function exactHex(value: unknown, bytes: number, label: string): string {
  const candidate = text(value, label);
  if (!new RegExp(`^[0-9a-f]{${bytes * 2}}$`, "u").test(candidate)) {
    throw new Error(`${label} was not one canonical identity.`);
  }
  return candidate;
}

function decimal(value: unknown, label: string): string {
  const candidate = text(value, label);
  if (!/^(0|[1-9][0-9]*)$/u.test(candidate)) {
    throw new Error(`${label} was not one canonical decimal integer.`);
  }
  return candidate;
}

function boundedInteger(value: unknown, label: string, { nullable = false } = {}): number | null {
  if (nullable && value === null) return null;
  if (!Number.isSafeInteger(value) || (value as number) < 0 || (value as number) > 4_096) {
    throw new Error(`${label} was not one bounded integer.`);
  }
  return value as number;
}

function safeLine(value: unknown, label: string): string {
  if (typeof value !== "string" || value.length > 16_384
    || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(value)) {
    throw new Error(`${label} was unsafe or unbounded.`);
  }
  return value;
}

function comparisonLimitation(
  body: string,
  value: unknown,
  verifiedText: unknown,
): ReviewComparisonLimitation | null {
  if (body !== "opaque") {
    if (value !== null && value !== undefined) {
      throw new Error("A non-opaque review change carried an opaque reason.");
    }
    return null;
  }
  if (verifiedText !== null) {
    throw new Error("An opaque change invented a text comparison.");
  }
  if (value === "above-line-ceiling" || value === "content-class-changed") return value;
  throw new Error("The opaque reason was unsupported.");
}

function pathArtifactKind(
  path: string,
  body: string,
  verifiedText: unknown,
  limitation: ReviewComparisonLimitation | null,
): ArtifactKind {
  // An opaque body is the daemon's closed metadata-only representation. It must not be routed to
  // an extension-selected renderer, even when a content-class transition used an Office suffix.
  if (limitation !== null) return "text";
  const extension = path.split("/").at(-1)?.split(".").at(-1)?.toLowerCase() ?? "";
  const kind = artifactKinds[extension];
  if (kind) return kind;
  // The daemon's conservative body classifier may call an imported UTF-8 file binary while also
  // supplying a fully verified before/after text projection. The verified projection is the more
  // specific evidence and is already identity-checked below; use it for the text workbench rather
  // than rejecting ordinary extensionless, script, and text files.
  if (body === "text" || verifiedText !== null) return "text";
  throw new Error("The review change has no supported comparison surface.");
}

function artifactKind(
  beforePath: string | null,
  afterPath: string | null,
  body: string,
  verifiedText: unknown,
  limitation: ReviewComparisonLimitation | null,
): ArtifactKind {
  const paths = [beforePath, afterPath].filter((path): path is string => path !== null);
  if (paths.length === 0) throw new Error("The review change did not name a path.");
  const kinds = paths.map((path) => pathArtifactKind(path, body, verifiedText, limitation));
  if (!kinds.every((kind) => kind === kinds[0])) {
    throw new Error("The reviewed paths do not share one supported comparison surface.");
  }
  return kinds[0];
}

function summaryValues(value: unknown, side: "before" | "after"): readonly string[] {
  if (value === null) return Object.freeze([side === "before" ? "Not present before" : "Not present after"]);
  const summary = record(value, `${side} content summary`);
  const version = exactHex(summary.version_id, 32, `${side} version`);
  const kind = text(summary.kind, `${side} content kind`);
  if (kind === "binary") {
    const digest = exactHex(summary.content_digest, 32, `${side} content digest`);
    const bytes = decimal(summary.byte_length, `${side} byte length`);
    if (summary.line_count !== null) throw new Error(`${side} binary content carried a line count.`);
    return Object.freeze([`${bytes} bytes`, `Digest ${digest.slice(0, 12)}…`, `Version ${version.slice(0, 12)}…`]);
  }
  if (kind === "text") {
    const lines = decimal(summary.line_count, `${side} line count`);
    if (summary.content_digest !== null || summary.byte_length !== null) {
      throw new Error(`${side} text content carried binary fields.`);
    }
    return Object.freeze([`${lines} lines`, `Version ${version.slice(0, 12)}…`]);
  }
  throw new Error(`${side} content kind was unsupported.`);
}

function changedTextValues(
  verified: unknown,
  beforeFallback: readonly string[],
  afterFallback: readonly string[],
  beforeSummary: unknown,
  afterSummary: unknown,
): Readonly<{ before: readonly string[]; after: readonly string[]; hunks: readonly ReviewDiffHunk[] }> {
  if (verified === null) return Object.freeze({ before: beforeFallback, after: afterFallback, hunks: Object.freeze([]) });
  const projection = record(verified, "verified text projection");
  if (projection.source !== "before-after" || !Array.isArray(projection.hunks) || projection.hunks.length > 128) {
    throw new Error("The verified text projection was unsupported or unbounded.");
  }
  const identityMatches = (identityValue: unknown, summaryValue: unknown, side: string) => {
    if (identityValue === null || summaryValue === null) {
      if (identityValue !== summaryValue) throw new Error(`${side} verified text identity did not match its summary.`);
      return;
    }
    const identity = record(identityValue, `${side} verified text identity`);
    const summary = record(summaryValue, `${side} content summary`);
    const verifiedVersion = exactHex(identity.version_id, 32, `${side} verified version`);
    const verifiedDigest = exactHex(identity.content_digest, 32, `${side} verified digest`);
    const summaryDigest = summary.content_digest;
    if (verifiedVersion !== summary.version_id
      || (summaryDigest !== null && verifiedDigest !== summaryDigest)) {
      throw new Error(`${side} verified text identity did not match its summary.`);
    }
  };
  identityMatches(projection.before, beforeSummary, "before");
  identityMatches(projection.after, afterSummary, "after");
  const before: string[] = [];
  const after: string[] = [];
  const hunks: ReviewDiffHunk[] = [];
  let totalLines = 0;
  let totalCharacters = 0;
  for (const hunk of projection.hunks) {
    const hunkRecord = record(hunk, "verified text hunk");
    const beforeStart = boundedInteger(hunkRecord.before_start, "verified text before start") as number;
    const afterStart = boundedInteger(hunkRecord.after_start, "verified text after start") as number;
    const beforeLength = boundedInteger(hunkRecord.before_len, "verified text before length") as number;
    const afterLength = boundedInteger(hunkRecord.after_len, "verified text after length") as number;
    if (beforeStart < 1 || afterStart < 1) throw new Error("A verified text hunk started before line one.");
    const lines = hunkRecord.lines;
    if (!Array.isArray(lines) || lines.length > 4_096) throw new Error("A verified text hunk was unbounded.");
    totalLines += lines.length;
    if (totalLines > 4_096) throw new Error("The verified text projection contained too many lines.");
    let nextBefore = beforeStart;
    let nextAfter = afterStart;
    const verifiedLines = [];
    for (const candidate of lines) {
      const line = record(candidate, "verified text line");
      const kind = text(line.kind, "verified text line kind");
      const value = safeLine(line.text, "verified text line");
      if (!['context', 'removed', 'added'].includes(kind)) throw new Error("A verified text line kind was unknown.");
      totalCharacters += value.length;
      if (totalCharacters > 512 * 1_024) throw new Error("The verified text projection contained too many characters.");
      const beforeNumber = boundedInteger(line.before, "verified text before line", { nullable: true });
      const afterNumber = boundedInteger(line.after, "verified text after line", { nullable: true });
      if ((kind === "added" ? beforeNumber !== null : beforeNumber !== nextBefore)
        || (kind === "removed" ? afterNumber !== null : afterNumber !== nextAfter)) {
        throw new Error("A verified text line was not contiguous with its hunk.");
      }
      if (kind !== "added") nextBefore += 1;
      if (kind !== "removed") nextAfter += 1;
      if (kind !== "added" && before.length < 12) before.push(value);
      if (kind !== "removed" && after.length < 12) after.push(value);
      verifiedLines.push(Object.freeze({ kind, before: beforeNumber, after: afterNumber, text: value }));
    }
    if (nextBefore !== beforeStart + beforeLength || nextAfter !== afterStart + afterLength) {
      throw new Error("A verified text hunk length did not match its lines.");
    }
    hunks.push(Object.freeze({ beforeStart, afterStart, lines: Object.freeze(verifiedLines) }) as ReviewDiffHunk);
  }
  return Object.freeze({
    before: Object.freeze(before.length > 0 ? before : [...beforeFallback]),
    after: Object.freeze(after.length > 0 ? after : [...afterFallback]),
    hunks: Object.freeze(hunks),
  });
}

function readableEffect(value: string): string {
  return value.replaceAll("-", " ").replace(/^./u, (letter) => letter.toUpperCase());
}

function reviewChange(value: unknown): ReviewChange {
  const change = record(value, "review change");
  const object = exactHex(change.object_id, 16, "review object");
  const beforePath = change.path_before === null ? null : text(change.path_before, "before path");
  const afterPath = change.path_after === null ? null : text(change.path_after, "after path");
  const path = afterPath ?? beforePath;
  if (!path) throw new Error("The review change did not name a path.");
  const body = text(change.body, "review body");
  const limitation = comparisonLimitation(body, change.opaque_reason, change.verified_text);
  const kind = artifactKind(beforePath, afterPath, body, change.verified_text, limitation);
  const effect = text(change.effect, "review effect");
  const beforeFallback = summaryValues(change.before, "before");
  const afterFallback = summaryValues(change.after, "after");
  const beforeSummary = change.before === null ? null : record(change.before, "before content summary");
  const afterSummary = change.after === null ? null : record(change.after, "after content summary");
  const values = kind === "text"
    ? changedTextValues(change.verified_text, beforeFallback, afterFallback, change.before, change.after)
    : Object.freeze({ before: beforeFallback, after: afterFallback, hunks: Object.freeze([]) });
  return Object.freeze({
    id: object,
    path: beforePath && afterPath && beforePath !== afterPath ? `${beforePath} → ${afterPath}` : path,
    kind,
    kindLabel: limitation === null ? artifactLabels[kind] : "File",
    summary: readableEffect(effect),
    impact: limitation !== null ? "Metadata only" : kind === "text" ? "Exact text" : "Exact saved artifact",
    beforeLabel: beforePath ? "Before" : "Before · not present",
    afterLabel: afterPath ? "After" : "After · not present",
    beforeValues: values.before,
    afterValues: values.after,
    diffHunks: values.hunks,
    comparisonLimitation: limitation,
    beforeVersionId: beforeSummary === null ? null : exactHex(beforeSummary.version_id, 32, "before version"),
    beforeContentDigest: beforeSummary?.kind === "binary"
      ? exactHex(beforeSummary.content_digest, 32, "before content digest")
      : null,
    afterVersionId: afterSummary === null ? null : exactHex(afterSummary.version_id, 32, "after version"),
    afterContentDigest: afterSummary?.kind === "binary"
      ? exactHex(afterSummary.content_digest, 32, "after content digest")
      : null,
  });
}

export function reviewWorkbenchFromProjection(
  workspaceName: string,
  versionLabel: string,
  value: unknown,
  authority: ReviewWorkbenchAuthority,
): ReviewWorkbenchModel {
  if (typeof authority.canRenderArtifactPreview !== "boolean"
    || typeof authority.canInspectExactCopies !== "boolean"
    || typeof authority.canRecordReview !== "boolean"
    || typeof authority.canApprove !== "boolean"
    || typeof authority.canApproveAndExport !== "boolean"
    || typeof authority.canExportGit !== "boolean"
    || typeof authority.canExportPrivateCopy !== "boolean") {
    throw new Error("The coordinator supplied malformed review authority.");
  }
  const item = record(value, "review projection");
  if (item.content_complete !== true
    || item.projection_authorizes_approval !== false
    || item.bundle_changes_not_listed !== 0
    || !Array.isArray(item.bundle_changes)
    || item.bundle_changes.length === 0
    || item.bundle_changes.length > 64) {
    throw new Error("The review projection was incomplete, authoritative, or unbounded.");
  }
  const bundle = exactHex(item.bundle, 32, "review bundle");
  exactHex(item.subject_operation, 32, "review subject");
  const changes = Object.freeze(item.bundle_changes.map(reviewChange));
  const unique = new Set(changes.map((change) => change.id));
  if (unique.size !== changes.length) throw new Error("The review projection repeated one object.");
  if (typeof authority.approvalReason !== "string" || authority.approvalReason.length === 0) {
    throw new Error("The coordinator did not explain the review authority state.");
  }
  return Object.freeze({
    workspaceName: text(workspaceName, "workspace name"),
    versionLabel: text(versionLabel, "version label"),
    bundleLabel: `Review ${bundle.slice(0, 12)}…`,
    changes,
    selectedChangeId: changes[0].id,
    mode: changes[0].kind === "text" ? "content" : "visual",
    diffLayout: "split",
    canRenderArtifactPreview: authority.canRenderArtifactPreview,
    canInspectExactCopies: authority.canInspectExactCopies,
    canRecordReview: authority.canRecordReview,
    canApprove: authority.canApprove,
    canApproveAndExport: authority.canApproveAndExport,
    canExportGit: authority.canExportGit,
    canExportPrivateCopy: authority.canExportPrivateCopy,
    approvalReason: authority.approvalReason,
  });
}
