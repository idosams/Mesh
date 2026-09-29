import { reviewArtifactKind, validatedReviewArtifactPreview } from './review-artifact-validation.js';

const exactKeys = (value, keys) => value && typeof value === 'object' && !Array.isArray(value)
  && Object.keys(value).sort().join(',') === keys;

export function fleetArtifactSide(raw, selection, change, side, kind, page) {
  if (typeof raw === 'string' && raw.length > 18 * 1024 * 1024) throw new Error('Oversized preview');
  const value = typeof raw === 'string' ? JSON.parse(raw) : raw;
  if (!exactKeys(value, 'object,objective,preview,schema,selection')
    || value.schema !== 'mesh.fleet-artifact-preview/v1' || value.objective !== selection.objective
    || value.object !== change.object_id || !exactKeys(value.selection, 'bundle,checkpoint,lane,version')
    || ['lane', 'checkpoint', 'version', 'bundle'].some(field => value.selection[field] !== selection[field])) {
    throw new Error('Preview selection mismatch');
  }
  const answer = validatedReviewArtifactPreview(value.preview, change, side, kind);
  if (kind === 'pdf' && answer.page_number !== page) throw new Error('Preview page mismatch');
  return { side, versionId: answer.version_id, contentDigest: answer.content_digest,
    imageDataUrl: answer.image_data_url, pageNumber: answer.page_number, pageCount: answer.page_count,
    textSource: answer.text_source, textLines: answer.text_lines,
    textSections: answer.text_sections?.map(section => ({ label: section.label, lineStart: section.line_start, lineCount: section.line_count })) ?? null,
    textTruncated: answer.text_truncated };
}

// One page/object per pin. Content is transient and never enters saved selector persistence.
export const loadFleetArtifact = (invoke, pin, change, page, generation) => loadSavedArtifact(invoke, pin, change, page, generation, 'render_fleet_review_artifact', fleetArtifactSide);
export async function loadSavedArtifact(invoke, pin, change, page, generation, command, parseSide) {
  const kind = reviewArtifactKind(change);
  if (!kind || !Number.isSafeInteger(page) || page < 1 || page > 64 || (kind !== 'pdf' && page !== 1)) throw new Error('Unsupported preview');
  const envelope = { generation, bundle: pin.selection.bundle, changeId: change.object_id, kind, requestedPage: page,
    before: null, after: null, beforeAbsentPage: null, afterAbsentPage: null, beforeError: null, afterError: null };
  await Promise.all(['before', 'after'].map(async side => {
    const summary = change[side];
    if (summary === null) return;
    const previous = pin.artifact?.envelope;
    const known = previous?.changeId === change.object_id ? previous[side] ?? previous[`${side}AbsentPage`] : null;
    if (kind === 'pdf' && known?.versionId === summary.version_id && known?.contentDigest === summary.content_digest
      && Number.isSafeInteger(known.pageCount) && page > known.pageCount) {
      envelope[`${side}AbsentPage`] = { side, versionId: known.versionId, contentDigest: known.contentDigest, pageCount: known.pageCount }; return;
    }
    try {
      const answer = await invoke(command, { ...pin.selection, objectId: change.object_id, side, pageNumber: kind === 'pdf' ? page : null });
      envelope[side] = parseSide(answer, pin.selection, change, side, kind, page);
    } catch {
      envelope[`${side}Error`] = 'This exact saved artifact preview is unavailable. Retry to render it again.';
    }
  }));
  if (!envelope.before && !envelope.after) throw new Error('No artifact preview available');
  return envelope;
}
