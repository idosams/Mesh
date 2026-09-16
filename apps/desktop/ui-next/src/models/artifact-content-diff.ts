import type {
  ReviewArtifactPreview,
  ReviewArtifactPreviewSide,
  ReviewChange,
  ReviewDiffHunk,
  ReviewDiffLine,
} from "./review-workbench";

export type ArtifactContentComparison = Readonly<{
  title: string;
  note: string;
  hunks: readonly ReviewDiffHunk[];
  sectionLabels: readonly string[];
}>;

export type ArtifactContentReviewState =
  | Readonly<{ kind: "ready"; comparison: ArtifactContentComparison }>
  | Readonly<{ kind: "not-loaded" }>
  | Readonly<{ kind: "failed"; messages: readonly string[] }>
  | Readonly<{ kind: "incompatible" }>;

type ArtifactRow = Readonly<{
  key: string;
  text: string;
  number: number | null;
  section: boolean;
}>;

function documentSectionLabel(label: string): string {
  if (label === "Document" || label === "Document opening") return label;
  const heading = /^Section ([1-9][0-9]*) · /u.exec(label);
  return heading ? `Section ${heading[1]}` : label;
}

function rows(preview: ReviewArtifactPreviewSide | null): readonly ArtifactRow[] {
  if (!preview?.textLines || !preview.textSections) return Object.freeze([]);
  const result: ArtifactRow[] = [];
  for (const [index, section] of preview.textSections.entries()) {
    const sectionLabel = preview.textSource === "mesh-pptx-slide-text-v1"
      ? `Slide ${index + 1}`
      : preview.textSource === "mesh-docx-block-text-v1"
        ? documentSectionLabel(section.label)
        : section.label;
    result.push(Object.freeze({
      key: `section\u0000${sectionLabel}`,
      text: sectionLabel,
      number: null,
      section: true,
    }));
    if (section.label !== sectionLabel) {
      result.push(Object.freeze({
        key: `text\u0000${sectionLabel}\u0000title\u0000${section.label}`,
        text: section.label,
        number: null,
        section: false,
      }));
    }
    for (let offset = 0; offset < section.lineCount; offset += 1) {
      const number = section.lineStart + offset + 1;
      const text = preview.textLines[number - 1];
      result.push(Object.freeze({
        key: `text\u0000${sectionLabel}\u0000${text}`,
        text,
        number,
        section: false,
      }));
    }
  }
  return Object.freeze(result);
}

function diffLines(beforeRows: readonly ArtifactRow[], afterRows: readonly ArtifactRow[]): readonly ReviewDiffLine[] {
  const width = afterRows.length + 1;
  const matrix = new Uint16Array((beforeRows.length + 1) * width);
  const at = (before: number, after: number) => before * width + after;
  for (let before = beforeRows.length - 1; before >= 0; before -= 1) {
    for (let after = afterRows.length - 1; after >= 0; after -= 1) {
      matrix[at(before, after)] = beforeRows[before].key === afterRows[after].key
        ? matrix[at(before + 1, after + 1)] + 1
        : Math.max(matrix[at(before + 1, after)], matrix[at(before, after + 1)]);
    }
  }
  const lines: ReviewDiffLine[] = [];
  let before = 0;
  let after = 0;
  while (before < beforeRows.length || after < afterRows.length) {
    if (before < beforeRows.length && after < afterRows.length
      && beforeRows[before].key === afterRows[after].key) {
      lines.push(Object.freeze({
        kind: "context",
        before: beforeRows[before].number,
        after: afterRows[after].number,
        text: beforeRows[before].text,
        section: beforeRows[before].section,
      }));
      before += 1;
      after += 1;
    } else if (before < beforeRows.length
      && (after === afterRows.length
        || matrix[at(before + 1, after)] >= matrix[at(before, after + 1)])) {
      lines.push(Object.freeze({
        kind: "removed",
        before: beforeRows[before].number,
        after: null,
        text: beforeRows[before].text,
        section: beforeRows[before].section,
      }));
      before += 1;
    } else {
      lines.push(Object.freeze({
        kind: "added",
        before: null,
        after: afterRows[after].number,
        text: afterRows[after].text,
        section: afterRows[after].section,
      }));
      after += 1;
    }
  }
  return Object.freeze(lines);
}

function oneHunk(lines: readonly ReviewDiffLine[]): readonly ReviewDiffHunk[] {
  if (lines.length === 0) return Object.freeze([]);
  return Object.freeze([Object.freeze({
    beforeStart: lines.find((line) => line.before !== null)?.before ?? 1,
    afterStart: lines.find((line) => line.after !== null)?.after ?? 1,
    lines,
  })]);
}

type ContentComparisonState = "differences" | "complete-match" | "bounded-prefix";

function exactBytesCopy(kind: ReviewChange["kind"]): Readonly<{ title: string; note: string }> {
  const artifact = Object.freeze({
    text: "file",
    pdf: "PDF",
    presentation: "presentation",
    document: "document",
    spreadsheet: "workbook",
    file: "file",
  })[kind];
  return Object.freeze({
    title: `Exact ${artifact} bytes match`,
    note: "The saved file bytes are identical. This review change affects the path or file metadata, not document content.",
  });
}

function sourceCopy(
  source: string,
  state: ContentComparisonState,
  truncated: boolean,
): Readonly<{ title: string; note: string }> {
  const changed = state === "differences";
  const bounded = truncated
    ? " The extraction reached its bounded limit, so content beyond the extracted prefix was not compared."
    : "";
  if (source === "mesh-xlsx-cell-formula-v1") return Object.freeze({
    title: changed
      ? "Workbook content and structure changes"
      : state === "bounded-prefix"
        ? "No differences in the extracted workbook prefix"
        : "Workbook changed; extracted content and structure match",
    note: `Compared inert worksheet names, worksheet visibility, explicit and default row visibility, merged ranges, cell coordinates, stored values, formulas, attributes, and cached results. Mesh never executes the workbook. Column visibility, formatting, and embedded content require exact-copy inspection.${bounded}`,
  });
  if (source === "mesh-pptx-slide-text-v1") return Object.freeze({
    title: changed
      ? "Slide content and structure changes"
      : state === "bounded-prefix"
        ? "No differences in the extracted presentation prefix"
        : "Presentation changed; extracted slide content and structure match",
    note: `Compared inert slide visibility, object visibility, and text in exact slide order. Mesh never executes presentation content. Layout, animation, speaker notes, charts, media, and embedded content require exact-copy inspection.${bounded}`,
  });
  if (source === "mesh-docx-block-text-v1") return Object.freeze({
    title: changed
      ? "Document content and visibility changes"
      : state === "bounded-prefix"
        ? "No differences in the extracted document prefix"
        : "Document changed; extracted content and visibility match",
    note: `Compared inert paragraph and table text, preserved whitespace, and directly formatted hidden-run visibility. Mesh never executes fields, macros, links, or embedded content. Style-inherited visibility, layout, comments, tracked-change metadata, images, and formatting require exact-copy inspection.${bounded}`,
  });
  if (source === "macos-pdfkit-page-text-v1") return Object.freeze({
    title: changed
      ? "Page text changes"
      : state === "bounded-prefix"
        ? "No differences in the extracted page prefix"
        : "PDF changed; selected page text matches",
    note: `Compared inert text from the exact selected PDF page. Mesh never executes document actions or embedded content. Layout, annotations, forms, signatures, images, and embedded files remain visible only in the visual or exact-copy views.${bounded}`,
  });
  return Object.freeze({
    title: changed
      ? "Visible content changes"
      : state === "bounded-prefix"
        ? "No differences in the extracted preview prefix"
        : "Artifact changed; visible text matches",
    note: `Compared inert visible text from the representative native preview. Mesh never executes document actions or embedded content. Formatting, formulas, metadata, and embedded content require exact-copy inspection.${bounded}`,
  });
}

export function artifactContentComparison(
  change: ReviewChange,
  preview: ReviewArtifactPreview | null,
): ArtifactContentComparison | null {
  if (!preview || preview.changeId !== change.id) return null;
  if ((change.beforeVersionId !== null && !preview.before && !preview.beforeAbsentPage)
    || (change.afterVersionId !== null && !preview.after && !preview.afterAbsentPage)) return null;
  const sides = [preview.before, preview.after].filter((side): side is ReviewArtifactPreviewSide => side !== null);
  const sources = new Set(sides.map((side) => side.textSource).filter((source): source is string => source !== null));
  if (sources.size !== 1 || sides.some((side) => side.textSource === null)) return null;
  if (change.beforeContentDigest !== null
    && change.beforeContentDigest === change.afterContentDigest) {
    const copy = exactBytesCopy(change.kind);
    return Object.freeze({
      title: copy.title,
      note: copy.note,
      hunks: Object.freeze([]),
      sectionLabels: Object.freeze([]),
    });
  }
  const lines = diffLines(rows(preview.before), rows(preview.after));
  const changed = lines.some((line) => line.kind !== "context");
  const truncated = sides.some((side) => side.textTruncated);
  const state: ContentComparisonState = changed
    ? "differences"
    : truncated ? "bounded-prefix" : "complete-match";
  const copy = sourceCopy([...sources][0], state, truncated);
  const sectionLabels = Object.freeze([...new Set(
    lines.filter((line) => line.section).map((line) => line.text),
  )]);
  return Object.freeze({ title: copy.title, note: copy.note, hunks: oneHunk(lines), sectionLabels });
}

export function artifactSectionHunks(
  comparison: ArtifactContentComparison,
  sectionLabel: string | null,
): readonly ReviewDiffHunk[] {
  if (sectionLabel === null) return comparison.hunks;
  if (!comparison.sectionLabels.includes(sectionLabel)) {
    throw new Error("The selected artifact section is not in the exact content comparison.");
  }
  const lines = comparison.hunks.flatMap((hunk) => hunk.lines);
  const selected: ReviewDiffHunk[] = [];
  let current: ReviewDiffLine[] | null = null;
  // A reordered named worksheet can appear once as removed and again as added, with another
  // section between those positions. Preserve every occurrence so the filtered view cannot hide
  // either exact side of the move.
  const finishCurrent = () => {
    if (current) selected.push(oneHunk(current)[0]);
    current = null;
  };
  for (const line of lines) {
    if (line.section === true) {
      finishCurrent();
      current = line.text === sectionLabel ? [line] : null;
    } else if (current) {
      current.push(line);
    }
  }
  finishCurrent();
  if (selected.length === 0) {
    throw new Error("The selected artifact section has no exact content boundary.");
  }
  return Object.freeze(selected);
}

export function artifactContentReviewState(
  change: ReviewChange,
  preview: ReviewArtifactPreview | null,
  requestError: string | null,
): ArtifactContentReviewState {
  const failures = [
    requestError,
    preview?.beforeError ? `Earlier version: ${preview.beforeError}` : null,
    preview?.afterError ? `Current version: ${preview.afterError}` : null,
  ].filter((value): value is string => value !== null);
  if (failures.length > 0) {
    return Object.freeze({ kind: "failed", messages: Object.freeze(failures) });
  }
  const comparison = artifactContentComparison(change, preview);
  if (comparison) return Object.freeze({ kind: "ready", comparison });
  if (preview) return Object.freeze({ kind: "incompatible" });
  return Object.freeze({ kind: "not-loaded" });
}
