// Exact saved artifact validation shared by workspace and independent fleet reviews.
const PDF_DOCUMENT_PAGE_LIMIT = 1_000_000;

export function reviewDiffTextIsSafe(text) {
  return !/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/u.test(text);
}

const REVIEW_ARTIFACT_KINDS = Object.freeze({
  pdf: 'pdf',
  pptx: 'presentation',
  docx: 'document',
  xlsx: 'spreadsheet',
  png: 'image',
  jpg: 'image',
  jpeg: 'image',
  gif: 'image',
  webp: 'image',
});

export function reviewArtifactKind(change) {
  const paths = [change.path_after, change.path_before].filter((path) => typeof path === 'string');
  const kinds = paths.map((path) => {
    const name = path.split('/').at(-1) || '';
    const extension = name.includes('.') ? name.split('.').at(-1).toLowerCase() : '';
    return REVIEW_ARTIFACT_KINDS[extension] || null;
  });
  return kinds.length > 0 && kinds.every((kind) => kind !== null && kind === kinds[0])
    ? kinds[0]
    : null;
}

function documentSectionsValid(sections) {
  if (sections.length === 1 && sections[0].label === 'Document') return true;
  const opening = sections[0]?.label === 'Document opening';
  if (opening && sections.length === 1) return false;
  return sections.slice(opening ? 1 : 0).every(
    (section, index) => section.label.startsWith(`Section ${index + 1} · `),
  );
}

export function validatedReviewArtifactPreview(answer, change, side, kind) {
  const summary = side === 'before' ? change.before : side === 'after' ? change.after : null;
  const path = side === 'before' ? change.path_before : change.path_after;
  const pdfPage = kind === 'pdf'
    && answer?.renderer === 'macos-pdfkit-page-v1'
    && answer?.scope === 'exact-page-preview'
    && Number.isSafeInteger(answer?.page_number)
    && answer.page_number >= 1
    && Number.isSafeInteger(answer?.page_count)
    && answer.page_count >= answer.page_number
    && answer.page_count <= PDF_DOCUMENT_PAGE_LIMIT;
  const representative = kind !== 'pdf'
    && answer?.renderer === (kind === 'image'
      ? 'macos-imageio-thumbnail-v1'
      : 'macos-quick-look-thumbnail')
    && answer?.scope === 'representative-preview'
    && answer?.page_number === null
    && answer?.page_count === null;
  if (
    !summary
    || summary.kind !== 'binary'
    || !/^[0-9a-f]{64}$/u.test(summary.version_id || '')
    || !/^[0-9a-f]{64}$/u.test(summary.content_digest || '')
    || typeof path !== 'string'
    || reviewArtifactKind({ [`path_${side}`]: path }) !== kind
    || !answer
    || (!pdfPage && !representative)
    || answer.kind !== kind
    || answer.side !== side
    || answer.version_id !== summary.version_id
    || answer.content_digest !== summary.content_digest
    || answer.rendering_authorizes_approval !== false
    || typeof answer.image_data_url !== 'string'
    || answer.image_data_url.length > 16 * 1024 * 1024
    || !/^data:image\/png;base64,[A-Za-z0-9+/]+={0,2}$/u.test(answer.image_data_url)
  ) {
    throw new Error('The visual preview did not match the exact reviewed artifact.');
  }
  const noText = answer.text_source === null
    && answer.text_lines === null
    && answer.text_sections === null
    && answer.text_truncated === false;
  const textLines = Array.isArray(answer.text_lines) ? answer.text_lines : [];
  const textSections = Array.isArray(answer.text_sections) ? answer.text_sections : [];
  let expectedLineStart = 0;
  const sectionsValid = textSections.length > 0
    && textSections.length <= 64
    && textSections.every((section) => {
      const valid = section !== null
        && typeof section === 'object'
        && !Array.isArray(section)
        && Object.keys(section).sort().join(',') === 'label,line_count,line_start'
        && typeof section.label === 'string'
        && section.label.length > 0
        // Word headings are truncated by Unicode scalar value in Rust. Astral characters occupy
        // two UTF-16 units here, so the longest valid native `Section N · …` label needs 128.
        && section.label.length <= 128
        && reviewDiffTextIsSafe(section.label)
        && Number.isSafeInteger(section.line_start)
        && section.line_start === expectedLineStart
        && Number.isSafeInteger(section.line_count)
        && section.line_count > 0
        && section.line_count <= 512;
      if (valid) expectedLineStart += section.line_count;
      return valid;
    })
    && expectedLineStart === textLines.length;
  const textSourceValid = answer.text_source === 'macos-quick-look-visible-text'
    || (kind === 'pdf' && answer.text_source === 'macos-pdfkit-page-text-v1')
    || (kind === 'presentation' && answer.text_source === 'mesh-pptx-slide-text-v1')
    || (kind === 'document' && answer.text_source === 'mesh-docx-block-text-v1')
    || (kind === 'spreadsheet' && answer.text_source === 'mesh-xlsx-cell-formula-v1');
  const textStructureValid = (answer.text_source !== 'macos-pdfkit-page-text-v1'
      || (textSections.length === 1 && textSections[0].label === `Page ${answer.page_number}`))
    && (answer.text_source !== 'mesh-pptx-slide-text-v1'
      || textSections.every((section, index) => section.label === `Slide ${index + 1}`
        || section.label.startsWith(`Slide ${index + 1} · `)))
    && (answer.text_source !== 'mesh-docx-block-text-v1'
      || documentSectionsValid(textSections))
    && (answer.text_source !== 'mesh-xlsx-cell-formula-v1'
      || new Set(textSections.map((section) => section.label)).size === textSections.length);
  const hasText = textSourceValid
    && textStructureValid
    && textLines.length > 0
    && textLines.length <= 512
    && sectionsValid
    && typeof answer.text_truncated === 'boolean'
    && textLines.every((line) => typeof line === 'string'
      && line.length > 0
      && reviewDiffTextIsSafe(line))
    && textLines.reduce((total, line) => total + line.length, 0) <= 128 * 1024;
  if (!noText && !hasText) {
    throw new Error('The extracted artifact text was malformed or unbounded.');
  }
  return answer;
}

