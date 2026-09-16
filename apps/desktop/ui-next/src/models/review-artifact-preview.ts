import type {
  ReviewArtifactAbsentPage,
  ReviewArtifactPreview,
  ReviewArtifactPreviewSide,
  ReviewArtifactTextSection,
  ReviewChange,
  ReviewWorkbenchModel,
} from "./review-workbench";

type JsonRecord = Record<string, unknown>;

const PDF_PREVIEW_PAGE_LIMIT = 64;
const PDF_DOCUMENT_PAGE_LIMIT = 1_000_000;

function exactKeys(value: JsonRecord, expected: readonly string[], label: string): void {
  const actual = Object.keys(value).sort();
  const canonical = [...expected].sort();
  if (actual.length !== canonical.length
    || actual.some((key, index) => key !== canonical[index])) {
    throw new Error(`${label} had unrecognized or missing fields.`);
  }
}

function record(value: unknown, label: string): JsonRecord {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} was not one object.`);
  }
  return value as JsonRecord;
}

function exactHex(value: unknown, label: string): string {
  if (typeof value !== "string" || !/^[0-9a-f]{64}$/u.test(value)) {
    throw new Error(`${label} was not one canonical identity.`);
  }
  return value;
}

function boundedError(value: unknown, label: string): string | null {
  if (value === null) return null;
  return safeText(value, label, 1_024);
}

function safeText(value: unknown, label: string, maximum: number): string {
  if (typeof value !== "string" || value.length === 0 || value.length > maximum
    || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(value)) {
    throw new Error(`${label} was unsafe or unbounded.`);
  }
  return value;
}

function artifactText(
  candidate: JsonRecord,
  change: ReviewChange,
  side: "before" | "after",
): Readonly<{
  source: string | null;
  lines: readonly string[] | null;
  sections: readonly ReviewArtifactTextSection[] | null;
  truncated: boolean;
}> {
  const source = candidate.textSource;
  const linesValue = candidate.textLines;
  const sectionsValue = candidate.textSections;
  const truncated = candidate.textTruncated;
  if (source === null && linesValue === null && sectionsValue === null && truncated === false) {
    return Object.freeze({ source: null, lines: null, sections: null, truncated: false });
  }
  const allowedSources: Readonly<Record<ReviewChange["kind"], readonly string[]>> = Object.freeze({
    text: Object.freeze([]),
    pdf: Object.freeze(["macos-pdfkit-page-text-v1"]),
    presentation: Object.freeze(["mesh-pptx-slide-text-v1", "macos-quick-look-visible-text"]),
    document: Object.freeze(["mesh-docx-block-text-v1", "macos-quick-look-visible-text"]),
    spreadsheet: Object.freeze(["mesh-xlsx-cell-formula-v1", "macos-quick-look-visible-text"]),
    file: Object.freeze([]),
  });
  if (typeof source !== "string" || !allowedSources[change.kind].includes(source)
    || !Array.isArray(linesValue) || linesValue.length === 0 || linesValue.length > 512
    || !Array.isArray(sectionsValue) || sectionsValue.length === 0 || sectionsValue.length > 64
    || typeof truncated !== "boolean") {
    throw new Error(`${side} extracted artifact content was malformed or unbounded.`);
  }
  let characters = 0;
  const lines = Object.freeze(linesValue.map((line, index) => {
    // Native extraction and the established coordinator bound the complete inert text payload,
    // not each visual line. PDFKit can truthfully return a single 32 KiB line, and Quick Look can
    // return a longer one, so keep this side of the adapter aligned with that 128 KiB contract.
    const text = safeText(line, `${side} extracted line ${index + 1}`, 128 * 1_024);
    characters += text.length;
    if (characters > 128 * 1_024) {
      throw new Error(`${side} extracted artifact content was malformed or unbounded.`);
    }
    return text;
  }));
  let nextLine = 0;
  const sections = Object.freeze(sectionsValue.map((value, index) => {
    const section = record(value, `${side} extracted section ${index + 1}`);
    exactKeys(section, ["label", "lineCount", "lineStart"], `${side} extracted section ${index + 1}`);
    // Native Word headings are bounded by Unicode scalar value. Astral characters consume two
    // JavaScript UTF-16 units, so accept the full valid native `Section N · …` envelope.
    const label = safeText(section.label, `${side} extracted section label`, 128);
    if (!Number.isSafeInteger(section.lineStart) || section.lineStart !== nextLine
      || !Number.isSafeInteger(section.lineCount) || (section.lineCount as number) < 1
      || (section.lineCount as number) > 512) {
      throw new Error(`${side} extracted artifact sections were not contiguous.`);
    }
    nextLine += section.lineCount as number;
    return Object.freeze({
      label,
      lineStart: section.lineStart as number,
      lineCount: section.lineCount as number,
    });
  }));
  if (nextLine !== lines.length) {
    throw new Error(`${side} extracted artifact sections did not cover their lines.`);
  }
  const pageStructureValid = source !== "macos-pdfkit-page-text-v1"
    || (sections.length === 1 && sections[0].label === `Page ${candidate.pageNumber}`);
  const presentationStructureValid = source !== "mesh-pptx-slide-text-v1"
    || sections.every((section, index) => section.label === `Slide ${index + 1}`
      || section.label.startsWith(`Slide ${index + 1} · `));
  const documentOpening = sections[0]?.label === "Document opening";
  const documentStructureValid = source !== "mesh-docx-block-text-v1"
    || (sections.length === 1 && sections[0].label === "Document")
    || (documentOpening && sections.length > 1
      && sections.slice(1).every((section, index) => section.label.startsWith(`Section ${index + 1} · `)))
    || (!documentOpening
      && sections.every((section, index) => section.label.startsWith(`Section ${index + 1} · `)));
  const workbookStructureValid = source !== "mesh-xlsx-cell-formula-v1"
    || new Set(sections.map((section) => section.label)).size === sections.length;
  if (!pageStructureValid || !presentationStructureValid
    || !documentStructureValid || !workbookStructureValid) {
    throw new Error(`${side} extracted artifact sections did not match their format.`);
  }
  return Object.freeze({ source, lines, sections, truncated });
}

function sidePreview(
  value: unknown,
  change: ReviewChange,
  side: "before" | "after",
  requestedPage: number,
): ReviewArtifactPreviewSide | null {
  if (value === null) return null;
  const candidate = record(value, `${side} visual preview`);
  exactKeys(candidate, [
    "contentDigest",
    "imageDataUrl",
    "pageCount",
    "pageNumber",
    "side",
    "textLines",
    "textSections",
    "textSource",
    "textTruncated",
    "versionId",
  ], `${side} visual preview`);
  const versionId = exactHex(candidate.versionId, `${side} preview version`);
  const contentDigest = exactHex(candidate.contentDigest, `${side} preview digest`);
  const expectedVersion = side === "before" ? change.beforeVersionId : change.afterVersionId;
  const expectedDigest = side === "before" ? change.beforeContentDigest : change.afterContentDigest;
  if (versionId !== expectedVersion || contentDigest !== expectedDigest || candidate.side !== side) {
    throw new Error(`${side} visual preview did not match the reviewed artifact.`);
  }
  if (typeof candidate.imageDataUrl !== "string"
    || candidate.imageDataUrl.length > 16 * 1024 * 1024
    || !/^data:image\/png;base64,[A-Za-z0-9+/]+={0,2}$/u.test(candidate.imageDataUrl)) {
    throw new Error(`${side} visual preview image was malformed or unbounded.`);
  }
  const pdf = change.kind === "pdf";
  const pageNumber = candidate.pageNumber;
  const pageCount = candidate.pageCount;
  if (pdf) {
    if (!Number.isSafeInteger(pageNumber)
      || pageNumber !== requestedPage
      || !Number.isSafeInteger(pageCount)
      || (pageCount as number) < (pageNumber as number)
      || (pageNumber as number) > PDF_PREVIEW_PAGE_LIMIT
      || (pageCount as number) > PDF_DOCUMENT_PAGE_LIMIT) {
      throw new Error(`${side} PDF preview page was invalid.`);
    }
  } else if (pageNumber !== null || pageCount !== null) {
    throw new Error(`${side} representative preview invented PDF pages.`);
  }
  const text = artifactText(candidate, change, side);
  return Object.freeze({
    side,
    versionId,
    contentDigest,
    imageDataUrl: candidate.imageDataUrl,
    pageNumber: pageNumber as number | null,
    pageCount: pageCount as number | null,
    textSource: text.source,
    textLines: text.lines,
    textSections: text.sections,
    textTruncated: text.truncated,
  });
}

function absentPage(
  value: unknown,
  change: ReviewChange,
  side: "before" | "after",
  requestedPage: number,
): ReviewArtifactAbsentPage | null {
  if (value === null) return null;
  if (change.kind !== "pdf") {
    throw new Error(`${side} representative preview invented an absent PDF page.`);
  }
  const candidate = record(value, `${side} absent PDF page`);
  exactKeys(candidate, ["contentDigest", "pageCount", "side", "versionId"], `${side} absent PDF page`);
  const versionId = exactHex(candidate.versionId, `${side} absent-page version`);
  const contentDigest = exactHex(candidate.contentDigest, `${side} absent-page digest`);
  const expectedVersion = side === "before" ? change.beforeVersionId : change.afterVersionId;
  const expectedDigest = side === "before" ? change.beforeContentDigest : change.afterContentDigest;
  if (candidate.side !== side || versionId !== expectedVersion || contentDigest !== expectedDigest) {
    throw new Error(`${side} absent PDF page did not match the reviewed artifact.`);
  }
  if (!Number.isSafeInteger(candidate.pageCount)
    || (candidate.pageCount as number) < 1
    || (candidate.pageCount as number) > PDF_DOCUMENT_PAGE_LIMIT
    || requestedPage <= (candidate.pageCount as number)) {
    throw new Error(`${side} absent PDF page did not precede the requested page.`);
  }
  return Object.freeze({
    side,
    versionId,
    contentDigest,
    pageCount: candidate.pageCount as number,
  });
}

export function reviewArtifactPreviewEnvelope(
  value: unknown,
  generation: number,
  bundle: string,
  model: ReviewWorkbenchModel,
): ReviewArtifactPreview {
  const envelope = record(value, "artifact preview envelope");
  exactKeys(envelope, [
    "after",
    "afterAbsentPage",
    "afterError",
    "before",
    "beforeAbsentPage",
    "beforeError",
    "bundle",
    "changeId",
    "generation",
    "kind",
    "requestedPage",
  ], "artifact preview envelope");
  if (envelope.generation !== generation || envelope.bundle !== bundle) {
    throw new Error("The artifact preview did not match the mounted review.");
  }
  if (typeof envelope.changeId !== "string") throw new Error("The artifact preview did not name a change.");
  const change = model.changes.find((candidate) => candidate.id === envelope.changeId);
  if (!change || change.kind === "text" || envelope.kind !== change.kind) {
    throw new Error("The artifact preview did not match a supported review change.");
  }
  const requestedPage = envelope.requestedPage;
  if (!Number.isSafeInteger(requestedPage)
    || (requestedPage as number) < 1
    || (requestedPage as number) > PDF_PREVIEW_PAGE_LIMIT) {
    throw new Error("The artifact preview page was invalid.");
  }
  const before = sidePreview(envelope.before, change, "before", requestedPage as number);
  const after = sidePreview(envelope.after, change, "after", requestedPage as number);
  const beforeAbsentPage = absentPage(
    envelope.beforeAbsentPage,
    change,
    "before",
    requestedPage as number,
  );
  const afterAbsentPage = absentPage(
    envelope.afterAbsentPage,
    change,
    "after",
    requestedPage as number,
  );
  const beforeError = boundedError(envelope.beforeError, "before preview error");
  const afterError = boundedError(envelope.afterError, "after preview error");
  const beforeResults = [before, beforeAbsentPage, beforeError].filter((result) => result !== null).length;
  const afterResults = [after, afterAbsentPage, afterError].filter((result) => result !== null).length;
  if ((change.beforeVersionId === null ? beforeResults !== 0 : beforeResults !== 1)
    || (change.afterVersionId === null ? afterResults !== 0 : afterResults !== 1)
    || (change.kind === "pdf" && !before && !after)) {
    throw new Error("The artifact preview did not account for every reviewed side exactly once.");
  }
  return Object.freeze({
    changeId: change.id,
    requestedPage: requestedPage as number,
    before,
    after,
    beforeAbsentPage,
    afterAbsentPage,
    beforeError,
    afterError,
  });
}
