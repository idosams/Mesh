import assert from 'node:assert/strict';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import test from 'node:test';
import { build } from 'esbuild';

const root = new URL('.', import.meta.url).pathname;

function sourceFiles(directory) {
  return readdirSync(directory).flatMap((name) => {
    const path = join(directory, name);
    return statSync(path).isDirectory() ? sourceFiles(path) : [path];
  });
}

test('the prototype preserves the native-command boundary', () => {
  const source = sourceFiles(join(root, 'src')).map((path) => readFileSync(path, 'utf8')).join('\n');
  assert.doesNotMatch(source, /__TAURI__|\.invoke\s*\(|fetch\s*\(|WebSocket\s*\(/);
});

test('the prototype keeps the atomic component taxonomy explicit', () => {
  for (const layer of ['atoms', 'molecules', 'models', 'organisms', 'layouts', 'pages', 'views']) {
    const entries = readdirSync(join(root, 'src', layer));
    assert.ok(entries.length > 0, `${layer} must contain at least one component`);
  }
});

test('the alpha gives each primary journey one explicit visual action', () => {
  const onboarding = readFileSync(join(root, 'src/organisms/import-workbench.tsx'), 'utf8');
  const overview = readFileSync(join(root, 'src/organisms/workspace-overview.tsx'), 'utf8');
  assert.match(onboarding, /variant="primary"[\s\S]{0,240}\{model\.confirmLabel\}<\/Button>/);
  assert.match(onboarding, /variant="primary"[\s\S]{0,240}>Choose a folder<\/Button>/);
  assert.match(overview, /variant="primary"[\s\S]{0,240}onClick=\{\(\) => onIntent\(\{ type: "recommended" \}\)\}/);
  assert.equal((onboarding.match(/variant="primary"/g) || []).length, 2);
  assert.equal((overview.match(/variant="primary"/g) || []).length, 1);
});

test('the document workbench keeps before and after visible at alpha window widths', () => {
  const organism = readFileSync(join(root, 'src/organisms/artifact-review.tsx'), 'utf8');
  const navigator = readFileSync(join(root, 'src/molecules/change-navigator.tsx'), 'utf8');
  const tauri = JSON.parse(readFileSync(join(root, '..', 'src-tauri', 'tauri.conf.json'), 'utf8'));
  const desktop = tauri.app.windows[0];
  assert.ok(desktop.width < 1_280 && desktop.minWidth < 1_280);
  assert.ok(desktop.width >= 768 && desktop.minWidth >= 768);
  assert.match(organism, /xl:grid-cols-\[20rem_minmax\(0,1fr\)\]/);
  assert.equal((organism.match(/md:grid-cols-2/g) || []).length, 4);
  assert.doesNotMatch(organism, /grid-cols-\[17rem_minmax\(0,1fr\)_18rem\]/);
  assert.match(navigator, /max-h-72[^"\n]*overflow-y-auto[^"\n]*xl:max-h-\[44rem\]/);
  assert.match(navigator, /xl:border-b-0 xl:border-r/);
  assert.match(organism, /aria-label="Document section comparison"/);
  assert.match(organism, />\s*Previous section\s*</);
  assert.match(organism, />\s*Next section\s*</);
  assert.match(organism, /aria-label="Artifact section"/);
  assert.match(organism, /<h5 className="font-semibold">\{line\.text\}<\/h5>/);
});

test('the alpha documentation names the React-only visible shell without claiming native authority moved', () => {
  const readme = readFileSync(join(root, '..', 'README.md'), 'utf8');
  const alphaGuide = readFileSync(join(root, '..', 'ALPHA-START-HERE.txt'), 'utf8');
  assert.doesNotMatch(readme, /\*\*A React dependency graph\.\*\*/);
  assert.match(readme, /npm --prefix apps\/desktop run ui:next:check/);
  assert.match(readme, /\*\*A native-authority React replacement\.\*\*/);
  assert.match(readme, /React owns the entire visible shell, pages, and views/);
  assert.match(alphaGuide, /React shell is the only visible alpha interface/);
  assert.doesNotMatch(alphaGuide, /Use current import|Use current review/);
});

test('the review workbench is driven by a frozen model and typed intents', () => {
  const model = readFileSync(join(root, 'src/models/review-workbench.ts'), 'utf8');
  const organism = readFileSync(join(root, 'src/organisms/artifact-review.tsx'), 'utf8');
  const gallery = readFileSync(join(root, 'src/pages/gallery-page.tsx'), 'utf8');
  assert.match(model, /export type ReviewWorkbenchModel/);
  assert.match(model, /export type ReviewWorkbenchIntent/);
  assert.match(organism, /model: ReviewWorkbenchModel/);
  assert.match(organism, /onIntent: \(intent: ReviewWorkbenchIntent\)/);
  assert.match(organism, /Understand exactly what changed/);
  assert.doesNotMatch(organism, /before approving/);
  assert.match(organism, /Confirm review complete/);
  assert.match(organism, /Approve exact version/);
  assert.match(organism, /model\.canExportPrivateCopy/);
  assert.match(organism, /Choose export folder/);
  assert.match(organism, /It does not approve or change the original folder/);
  assert.match(gallery, /canInspectExactCopies: false/);
  assert.match(gallery, /canRenderArtifactPreview: false/);
  assert.match(gallery, /canRecordReview: false/);
  assert.match(gallery, /canApprove: false/);
  assert.match(gallery, /canExportPrivateCopy: false/);
});

test('the production review projection crosses one fail-closed typed adapter', async () => {
  const adapterPath = join(root, 'src/models/review-workbench-adapter.ts');
  const output = await build({
    entryPoints: [adapterPath],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const source = output.outputFiles[0].text;
  const adapter = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);
  const projection = {
    bundle: '11'.repeat(32),
    subject_operation: '22'.repeat(32),
    content_complete: true,
    projection_authorizes_approval: false,
    bundle_changes_not_listed: 0,
    bundle_changes: [{
      object_id: '33'.repeat(16),
      path_before: 'finance/plan.xlsx',
      path_after: 'finance/plan.xlsx',
      effect: 'content-written',
      body: 'binary',
      before: { kind: 'binary', version_id: '44'.repeat(32), content_digest: '55'.repeat(32), byte_length: '10', line_count: null },
      after: { kind: 'binary', version_id: '66'.repeat(32), content_digest: '77'.repeat(32), byte_length: '12', line_count: null },
      verified_text: null,
    }],
  };
  const authority = {
    canRenderArtifactPreview: true,
    canInspectExactCopies: false,
    canRecordReview: false,
    canApprove: false,
    canApproveAndExport: false,
    canExportGit: false,
    canExportPrivateCopy: false,
    approvalReason: 'The gallery has no native authority.',
  };
  const model = adapter.reviewWorkbenchFromProjection('Finance', 'Saved version', projection, authority);
  assert.equal(model.changes[0].kind, 'spreadsheet');
  assert.equal(model.changes[0].kindLabel, 'Excel');
  assert.equal(model.mode, 'visual');
  assert.equal(model.diffLayout, 'split');
  assert.equal(model.canApprove, false);
  assert.equal(model.canExportPrivateCopy, false);
  assert.equal(model.canRenderArtifactPreview, true);
  assert.equal(model.changes[0].beforeVersionId, '44'.repeat(32));
  assert.equal(model.changes[0].beforeContentDigest, '55'.repeat(32));
  assert.equal(Object.isFrozen(model), true);
  assert.equal(Object.isFrozen(model.changes), true);
  assert.equal(Object.isFrozen(model.changes[0]), true);
  const importedTextProjection = structuredClone(projection);
  importedTextProjection.bundle_changes[0] = {
    ...projection.bundle_changes[0],
    path_before: null,
    path_after: 'run.sh',
    before: null,
    body: 'binary',
    verified_text: {
      source: 'before-after',
      before: null,
      after: {
        version_id: projection.bundle_changes[0].after.version_id,
        content_digest: projection.bundle_changes[0].after.content_digest,
      },
      hunks: [{
        before_start: 1,
        before_len: 0,
        after_start: 1,
        after_len: 1,
        lines: [{ kind: 'added', before: null, after: 1, text: '#!/bin/sh' }],
      }],
    },
  };
  const importedText = adapter.reviewWorkbenchFromProjection(
    'Imported project',
    'Saved version',
    importedTextProjection,
    authority,
  );
  assert.equal(importedText.changes[0].kind, 'text');
  assert.equal(importedText.changes[0].diffHunks[0].lines[0].text, '#!/bin/sh');
  const genericBinaryProjection = structuredClone(projection);
  genericBinaryProjection.bundle_changes[0] = {
    ...projection.bundle_changes[0],
    path_before: null,
    path_after: '/.DS_Store',
    before: null,
    body: 'binary',
    verified_text: null,
  };
  const genericBinary = adapter.reviewWorkbenchFromProjection(
    'Alpha workspace',
    'Saved version',
    genericBinaryProjection,
    authority,
  );
  assert.equal(genericBinary.changes[0].kind, 'file');
  assert.equal(genericBinary.changes[0].kindLabel, 'File');
  assert.equal(genericBinary.changes[0].impact, 'Exact saved artifact');
  assert.deepEqual(genericBinary.changes[0].afterValues, [
    '12 bytes',
    `Digest ${'77'.repeat(6)}…`,
    `Version ${'66'.repeat(6)}…`,
  ]);
  const imageProjection = structuredClone(projection);
  imageProjection.bundle_changes[0] = {
    ...projection.bundle_changes[0],
    path_before: null,
    path_after: 'assets/hero.PNG',
    before: null,
    body: 'binary',
    verified_text: null,
  };
  const image = adapter.reviewWorkbenchFromProjection(
    'Alpha workspace',
    'Saved version',
    imageProjection,
    authority,
  );
  assert.equal(image.changes[0].kind, 'image');
  assert.equal(image.changes[0].kindLabel, 'Image');
  const crossFormatRenames = [
    ['finance/plan.docx', 'finance/plan.pptx'],
    ['finance/plan.pdf', 'finance/plan.txt'],
    ['finance/plan.txt', 'finance/plan.docx'],
  ];
  for (const [pathBefore, pathAfter] of crossFormatRenames) {
    const renamed = structuredClone(projection);
    renamed.bundle_changes[0] = {
      ...projection.bundle_changes[0],
      path_before: pathBefore,
      path_after: pathAfter,
    };
    assert.throws(
      () => adapter.reviewWorkbenchFromProjection('Finance', 'Saved version', renamed, {
        ...authority,
        canInspectExactCopies: true,
        canRecordReview: true,
        canApprove: true,
      }),
      /supported comparison surface/,
      `${pathBefore} to ${pathAfter} exposed an authoritative mismatched review`,
    );
  }
  const mixedBundle = structuredClone(projection);
  mixedBundle.bundle_changes.push({
    ...projection.bundle_changes[0],
    object_id: '88'.repeat(16),
    path_before: 'people/policy.docx',
    path_after: 'people/policy.pptx',
  });
  assert.throws(
    () => adapter.reviewWorkbenchFromProjection('Finance', 'Saved version', mixedBundle, {
      ...authority,
      canRenderArtifactPreview: true,
      canInspectExactCopies: true,
      canRecordReview: true,
      canApprove: true,
    }),
    /supported comparison surface/,
    'a supported change leaked aggregate actions onto a cross-format change',
  );
  const previewBuild = await build({
    entryPoints: [join(root, 'src/models/review-artifact-preview.ts')],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const previewModule = await import(`data:text/javascript;base64,${Buffer.from(previewBuild.outputFiles[0].text).toString('base64')}`);
  const previewEnvelope = {
    generation: 4,
    bundle: projection.bundle,
    changeId: projection.bundle_changes[0].object_id,
    kind: 'spreadsheet',
    requestedPage: 1,
    before: {
      side: 'before',
      versionId: projection.bundle_changes[0].before.version_id,
      contentDigest: projection.bundle_changes[0].before.content_digest,
      imageDataUrl: 'data:image/png;base64,YmVmb3Jl',
      pageNumber: null,
      pageCount: null,
      textSource: 'mesh-xlsx-cell-formula-v1',
      textLines: ['A1 · value: 100', 'B1 · formula: =A1*2 · cached: 200', 'C1 · value: 80'],
      textSections: [
        { label: 'Budget', lineStart: 0, lineCount: 2 },
        { label: 'Forecast', lineStart: 2, lineCount: 1 },
      ],
      textTruncated: false,
    },
    after: {
      side: 'after',
      versionId: projection.bundle_changes[0].after.version_id,
      contentDigest: projection.bundle_changes[0].after.content_digest,
      imageDataUrl: 'data:image/png;base64,YWZ0ZXI=',
      pageNumber: null,
      pageCount: null,
      textSource: 'mesh-xlsx-cell-formula-v1',
      textLines: ['A1 · value: 120', 'B1 · formula: =A1*2 · cached: 240', 'C1 · value: 95'],
      textSections: [
        { label: 'Budget', lineStart: 0, lineCount: 2 },
        { label: 'Forecast', lineStart: 2, lineCount: 1 },
      ],
      textTruncated: false,
    },
    beforeAbsentPage: null,
    afterAbsentPage: null,
    beforeError: null,
    afterError: null,
  };
  const preview = previewModule.reviewArtifactPreviewEnvelope(previewEnvelope, 4, projection.bundle, model);
  assert.match(preview.before.imageDataUrl, /^data:image\/png;base64,/);
  assert.equal(Object.isFrozen(preview), true);
  const contentBuild = await build({
    entryPoints: [join(root, 'src/models/artifact-content-diff.ts')],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const contentModule = await import(`data:text/javascript;base64,${Buffer.from(contentBuild.outputFiles[0].text).toString('base64')}`);
  const content = contentModule.artifactContentComparison(model.changes[0], preview);
  assert.equal(content.title, 'Workbook content and structure changes');
  assert.equal(content.hunks[0].lines.filter((line) => line.kind === 'removed').length, 3);
  assert.equal(content.hunks[0].lines.filter((line) => line.kind === 'added').length, 3);
  assert.deepEqual(content.sectionLabels, ['Budget', 'Forecast']);
  assert.deepEqual(
    content.hunks[0].lines.filter((line) => line.section).map((line) => [line.kind, line.text]),
    [['context', 'Budget'], ['context', 'Forecast']],
  );
  const forecast = contentModule.artifactSectionHunks(content, 'Forecast');
  assert.equal(forecast.length, 1);
  assert.equal(forecast[0].lines[0].section, true);
  assert.equal(forecast[0].lines[0].text, 'Forecast');
  assert.equal(forecast[0].lines.some((line) => line.text.includes('A1')), false);
  assert.throws(() => contentModule.artifactSectionHunks(content, 'Hidden sheet'), /not in the exact content comparison/);
  assert.match(content.note, /never executes/);
  const presentationProjection = {
    ...projection,
    bundle_changes: [{
      ...projection.bundle_changes[0],
      path_before: 'finance/briefing.pptx',
      path_after: 'finance/briefing.pptx',
    }],
  };
  const presentationModel = adapter.reviewWorkbenchFromProjection(
    'Finance',
    'Saved version',
    presentationProjection,
    authority,
  );
  const presentationEnvelope = {
    generation: 5,
    bundle: presentationProjection.bundle,
    changeId: presentationProjection.bundle_changes[0].object_id,
    kind: 'presentation',
    requestedPage: 1,
    before: {
      side: 'before',
      versionId: presentationProjection.bundle_changes[0].before.version_id,
      contentDigest: presentationProjection.bundle_changes[0].before.content_digest,
      imageDataUrl: 'data:image/png;base64,YmVmb3Jl',
      pageNumber: null,
      pageCount: null,
      textSource: 'mesh-pptx-slide-text-v1',
      textLines: ['Revenue 100'],
      textSections: [{ label: 'Slide 1 · Old title', lineStart: 0, lineCount: 1 }],
      textTruncated: false,
    },
    after: {
      side: 'after',
      versionId: presentationProjection.bundle_changes[0].after.version_id,
      contentDigest: presentationProjection.bundle_changes[0].after.content_digest,
      imageDataUrl: 'data:image/png;base64,YWZ0ZXI=',
      pageNumber: null,
      pageCount: null,
      textSource: 'mesh-pptx-slide-text-v1',
      textLines: ['Revenue 120'],
      textSections: [{ label: 'Slide 1 · New title', lineStart: 0, lineCount: 1 }],
      textTruncated: false,
    },
    beforeAbsentPage: null,
    afterAbsentPage: null,
    beforeError: null,
    afterError: null,
  };
  const presentationPreview = previewModule.reviewArtifactPreviewEnvelope(
    presentationEnvelope,
    5,
    presentationProjection.bundle,
    presentationModel,
  );
  const presentationContent = contentModule.artifactContentComparison(
    presentationModel.changes[0],
    presentationPreview,
  );
  assert.deepEqual(presentationContent.sectionLabels, ['Slide 1']);
  const slide = contentModule.artifactSectionHunks(presentationContent, 'Slide 1');
  assert.equal(slide[0].lines[0].section, true);
  assert.deepEqual(
    slide[0].lines.filter((line) => line.kind !== 'context').map((line) => [line.kind, line.text]),
    [
      ['removed', 'Slide 1 · Old title'],
      ['removed', 'Revenue 100'],
      ['added', 'Slide 1 · New title'],
      ['added', 'Revenue 120'],
    ],
  );
  const documentProjection = {
    ...projection,
    bundle_changes: [{
      ...projection.bundle_changes[0],
      path_before: 'people/plan.docx',
      path_after: 'people/plan.docx',
    }],
  };
  const documentModel = adapter.reviewWorkbenchFromProjection(
    'People',
    'Saved version',
    documentProjection,
    authority,
  );
  const documentEnvelope = {
    ...presentationEnvelope,
    generation: 6,
    bundle: documentProjection.bundle,
    changeId: documentProjection.bundle_changes[0].object_id,
    kind: 'document',
    before: {
      ...presentationEnvelope.before,
      textSource: 'mesh-docx-block-text-v1',
      textLines: ['Introductory context', 'Headcount 10'],
      textSections: [
        { label: 'Document opening', lineStart: 0, lineCount: 1 },
        { label: 'Section 1 · People plan', lineStart: 1, lineCount: 1 },
      ],
    },
    after: {
      ...presentationEnvelope.after,
      textSource: 'mesh-docx-block-text-v1',
      textLines: ['Introductory context', 'Headcount 12'],
      textSections: [
        { label: 'Document opening', lineStart: 0, lineCount: 1 },
        { label: 'Section 1 · Workforce plan', lineStart: 1, lineCount: 1 },
      ],
    },
  };
  const documentPreview = previewModule.reviewArtifactPreviewEnvelope(
    documentEnvelope,
    6,
    documentProjection.bundle,
    documentModel,
  );
  const documentContent = contentModule.artifactContentComparison(
    documentModel.changes[0],
    documentPreview,
  );
  assert.deepEqual(documentContent.sectionLabels, ['Document opening', 'Section 1']);
  const documentSection = contentModule.artifactSectionHunks(documentContent, 'Section 1');
  assert.deepEqual(
    documentSection[0].lines.filter((line) => line.kind !== 'context').map((line) => [line.kind, line.text]),
    [
      ['removed', 'Section 1 · People plan'],
      ['removed', 'Headcount 10'],
      ['added', 'Section 1 · Workforce plan'],
      ['added', 'Headcount 12'],
    ],
  );
  const documentOpeningEnvelope = {
    ...documentEnvelope,
    generation: 7,
    before: {
      ...documentEnvelope.before,
      textLines: ['Introductory context', 'Headcount 10'],
      textSections: [
        { label: 'Document opening', lineStart: 0, lineCount: 1 },
        { label: 'Section 1 · People plan', lineStart: 1, lineCount: 1 },
      ],
    },
    after: {
      ...documentEnvelope.after,
      textLines: ['Introductory context revised', 'Headcount 12'],
      textSections: [
        { label: 'Document opening', lineStart: 0, lineCount: 1 },
        { label: 'Section 1 · Workforce plan', lineStart: 1, lineCount: 1 },
      ],
    },
  };
  const documentOpeningPreview = previewModule.reviewArtifactPreviewEnvelope(
    documentOpeningEnvelope,
    7,
    documentProjection.bundle,
    documentModel,
  );
  const documentOpeningContent = contentModule.artifactContentComparison(
    documentModel.changes[0],
    documentOpeningPreview,
  );
  assert.deepEqual(documentOpeningContent.sectionLabels, ['Document opening', 'Section 1']);
  assert.deepEqual(
    contentModule.artifactSectionHunks(documentOpeningContent, 'Section 1')[0]
      .lines.filter((line) => line.kind !== 'context').map((line) => [line.kind, line.text]),
    [
      ['removed', 'Section 1 · People plan'],
      ['removed', 'Headcount 10'],
      ['added', 'Section 1 · Workforce plan'],
      ['added', 'Headcount 12'],
    ],
  );
  const emojiHeading = '📊'.repeat(57);
  const emojiLabel = `Section 1 · ${'📊'.repeat(56)}…`;
  const emojiDocumentPreview = previewModule.reviewArtifactPreviewEnvelope(
    {
      ...documentEnvelope,
      generation: 8,
      before: {
        ...documentEnvelope.before,
        textLines: [emojiHeading],
        textSections: [{ label: emojiLabel, lineStart: 0, lineCount: 1 }],
      },
      after: {
        ...documentEnvelope.after,
        textLines: [emojiHeading],
        textSections: [{ label: emojiLabel, lineStart: 0, lineCount: 1 }],
      },
    },
    8,
    documentProjection.bundle,
    documentModel,
  );
  assert.equal(emojiDocumentPreview.after?.imageDataUrl, documentEnvelope.after.imageDataUrl);
  assert.equal(emojiDocumentPreview.after?.textSections[0].label, emojiLabel);
  const pdfProjection = {
    ...projection,
    bundle_changes: [{
      ...projection.bundle_changes[0],
      path_before: 'finance/report.pdf',
      path_after: 'finance/report.pdf',
    }],
  };
  const pdfModel = adapter.reviewWorkbenchFromProjection('Finance', 'Saved version', pdfProjection, authority);
  const addedPageEnvelope = {
    generation: 4,
    bundle: pdfProjection.bundle,
    changeId: pdfProjection.bundle_changes[0].object_id,
    kind: 'pdf',
    requestedPage: 2,
    before: null,
    after: {
      side: 'after',
      versionId: pdfProjection.bundle_changes[0].after.version_id,
      contentDigest: pdfProjection.bundle_changes[0].after.content_digest,
      imageDataUrl: 'data:image/png;base64,YWZ0ZXI=',
      pageNumber: 2,
      pageCount: 3,
      textSource: 'macos-pdfkit-page-text-v1',
      textLines: ['Hiring plan approved', 'Risk owner Finance'],
      textSections: [{ label: 'Page 2', lineStart: 0, lineCount: 2 }],
      textTruncated: false,
    },
    beforeAbsentPage: {
      side: 'before',
      versionId: pdfProjection.bundle_changes[0].before.version_id,
      contentDigest: pdfProjection.bundle_changes[0].before.content_digest,
      pageCount: 1,
    },
    afterAbsentPage: null,
    beforeError: null,
    afterError: null,
  };
  const addedPagePreview = previewModule.reviewArtifactPreviewEnvelope(
    addedPageEnvelope,
    4,
    pdfProjection.bundle,
    pdfModel,
  );
  const boundedLargeDocumentPreview = previewModule.reviewArtifactPreviewEnvelope({
    ...addedPageEnvelope,
    after: { ...addedPageEnvelope.after, pageCount: 100 },
  }, 4, pdfProjection.bundle, pdfModel);
  assert.equal(boundedLargeDocumentPreview.after.pageCount, 100);
  assert.throws(
    () => previewModule.reviewArtifactPreviewEnvelope({
      ...addedPageEnvelope,
      after: { ...addedPageEnvelope.after, pageCount: 1_000_001 },
    }, 4, pdfProjection.bundle, pdfModel),
    /PDF preview page was invalid/,
  );
  const longNativePdfLine = 'A'.repeat(20_000);
  const longLinePreview = previewModule.reviewArtifactPreviewEnvelope({
    ...addedPageEnvelope,
    after: {
      ...addedPageEnvelope.after,
      textLines: [longNativePdfLine],
      textSections: [{ label: 'Page 2', lineStart: 0, lineCount: 1 }],
    },
  }, 4, pdfProjection.bundle, pdfModel);
  assert.equal(longLinePreview.after.textLines[0], longNativePdfLine);
  assert.equal(longLinePreview.after.imageDataUrl, addedPageEnvelope.after.imageDataUrl);
  assert.throws(
    () => previewModule.reviewArtifactPreviewEnvelope({
      ...addedPageEnvelope,
      after: {
        ...addedPageEnvelope.after,
        textLines: [`safe\u202eunsafe`],
        textSections: [{ label: 'Page 2', lineStart: 0, lineCount: 1 }],
      },
    }, 4, pdfProjection.bundle, pdfModel),
    /unsafe or unbounded/,
  );
  assert.throws(
    () => previewModule.reviewArtifactPreviewEnvelope({
      ...addedPageEnvelope,
      after: {
        ...addedPageEnvelope.after,
        textLines: ['A'.repeat(65_537), 'B'.repeat(65_537)],
        textSections: [{ label: 'Page 2', lineStart: 0, lineCount: 2 }],
      },
    }, 4, pdfProjection.bundle, pdfModel),
    /malformed or unbounded/,
  );
  const addedPageState = contentModule.artifactContentReviewState(
    pdfModel.changes[0],
    addedPagePreview,
    null,
  );
  assert.equal(addedPageState.kind, 'ready');
  assert.equal(addedPageState.comparison.hunks[0].lines.filter((line) => line.kind === 'added').length, 3);
  assert.throws(
    () => previewModule.reviewArtifactPreviewEnvelope({
      ...addedPageEnvelope,
      beforeAbsentPage: { ...addedPageEnvelope.beforeAbsentPage, contentDigest: '99'.repeat(32) },
    }, 4, pdfProjection.bundle, pdfModel),
    /did not match the reviewed artifact/,
  );
  assert.throws(
    () => previewModule.reviewArtifactPreviewEnvelope({
      ...addedPageEnvelope,
      beforeAbsentPage: { ...addedPageEnvelope.beforeAbsentPage, pageCount: 2 },
    }, 4, pdfProjection.bundle, pdfModel),
    /did not precede the requested page/,
  );
  assert.throws(
    () => previewModule.reviewArtifactPreviewEnvelope({
      ...previewEnvelope,
      after: { ...previewEnvelope.after, versionId: '99'.repeat(32) },
    }, 4, projection.bundle, model),
    /did not match the reviewed artifact/,
  );
  assert.throws(
    () => previewModule.reviewArtifactPreviewEnvelope({
      ...previewEnvelope,
      renderingAuthorizesApproval: true,
    }, 4, projection.bundle, model),
    /unrecognized or missing fields/,
  );
  assert.throws(
    () => previewModule.reviewArtifactPreviewEnvelope({
      ...previewEnvelope,
      after: { ...previewEnvelope.after, action: 'approve' },
    }, 4, projection.bundle, model),
    /unrecognized or missing fields/,
  );
  assert.throws(
    () => previewModule.reviewArtifactPreviewEnvelope({
      ...previewEnvelope,
      after: {
        ...previewEnvelope.after,
        textSections: [{ label: 'Budget', lineStart: 0, lineCount: 1 }],
      },
    }, 4, projection.bundle, model),
    /did not cover their lines/,
  );
  assert.throws(
    () => previewModule.reviewArtifactPreviewEnvelope({
      ...previewEnvelope,
      after: {
        ...previewEnvelope.after,
        textSections: [
          { label: 'Budget', lineStart: 0, lineCount: 1 },
          { label: 'Budget', lineStart: 1, lineCount: 1 },
          { label: 'Forecast', lineStart: 2, lineCount: 1 },
        ],
      },
    }, 4, projection.bundle, model),
    /did not match their format/,
  );
  assert.throws(
    () => previewModule.reviewArtifactPreviewEnvelope({
      ...previewEnvelope,
      before: null,
      beforeError: 'Unavailable \u202e approved',
    }, 4, projection.bundle, model),
    /unsafe or unbounded/,
  );
  const mismatchedSource = previewModule.reviewArtifactPreviewEnvelope({
    ...previewEnvelope,
    after: { ...previewEnvelope.after, textSource: 'macos-quick-look-visible-text' },
  }, 4, projection.bundle, model);
  assert.equal(contentModule.artifactContentComparison(model.changes[0], mismatchedSource), null);
  const failedSource = previewModule.reviewArtifactPreviewEnvelope({
    ...previewEnvelope,
    after: null,
    afterError: 'The current saved version could not be read safely.',
  }, 4, projection.bundle, model);
  assert.equal(contentModule.artifactContentReviewState(model.changes[0], null, null).kind, 'not-loaded');
  assert.equal(contentModule.artifactContentReviewState(model.changes[0], mismatchedSource, null).kind, 'incompatible');
  const failedState = contentModule.artifactContentReviewState(model.changes[0], failedSource, null);
  assert.equal(failedState.kind, 'failed');
  assert.deepEqual(failedState.messages, ['Current version: The current saved version could not be read safely.']);
  assert.equal(Object.isFrozen(failedState.messages), true);
  assert.throws(
    () => adapter.reviewWorkbenchFromProjection('Finance', 'Saved version', {
      ...projection,
      bundle_changes_not_listed: 1,
    }, authority),
    /incomplete, authoritative, or unbounded/,
  );
  assert.throws(
    () => adapter.reviewWorkbenchFromProjection('Finance', 'Saved version', {
      ...projection,
      projection_authorizes_approval: true,
    }, authority),
    /incomplete, authoritative, or unbounded/,
  );
});

test('opaque native text changes remain explicit inside mixed React review bundles', async () => {
  const adapterBuild = await build({
    entryPoints: [join(root, 'src/models/review-workbench-adapter.ts')],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const adapter = await import(`data:text/javascript;base64,${Buffer.from(adapterBuild.outputFiles[0].text).toString('base64')}`);
  const textBefore = '11'.repeat(32);
  const textAfter = '22'.repeat(32);
  const projection = {
    bundle: '33'.repeat(32),
    subject_operation: '44'.repeat(32),
    content_complete: true,
    projection_authorizes_approval: false,
    bundle_changes_not_listed: 0,
    bundle_changes: [{
      object_id: '55'.repeat(16),
      path_before: 'budget.xlsx',
      path_after: 'budget.xlsx',
      effect: 'content-written',
      body: 'binary',
      opaque_reason: null,
      before: { kind: 'binary', version_id: '66'.repeat(32), content_digest: '77'.repeat(32), byte_length: '100', line_count: null },
      after: { kind: 'binary', version_id: '88'.repeat(32), content_digest: '99'.repeat(32), byte_length: '101', line_count: null },
      verified_text: null,
    }, {
      object_id: 'aa'.repeat(16),
      path_before: 'policy.txt',
      path_after: 'policy.txt',
      effect: 'content-written',
      body: 'opaque',
      opaque_reason: 'above-line-ceiling',
      before: { kind: 'text', version_id: textBefore, content_digest: null, byte_length: null, line_count: '3' },
      after: { kind: 'text', version_id: textAfter, content_digest: null, byte_length: null, line_count: '4097' },
      verified_text: null,
    }],
  };
  const authority = {
    canRenderArtifactPreview: true,
    canInspectExactCopies: true,
    canRecordReview: true,
    canApprove: true,
    canApproveAndExport: false,
    canExportGit: false,
    canExportPrivateCopy: false,
    approvalReason: 'Ready for exact native approval.',
  };
  const model = adapter.reviewWorkbenchFromProjection('People', 'Saved version', projection, authority);
  assert.deepEqual(model.changes.map((change) => change.path), ['budget.xlsx', 'policy.txt']);
  const opaque = model.changes[1];
  assert.equal(opaque.kind, 'text');
  assert.equal(opaque.kindLabel, 'File');
  assert.equal(opaque.impact, 'Metadata only');
  assert.equal(opaque.comparisonLimitation, 'above-line-ceiling');
  assert.deepEqual(opaque.diffHunks, []);
  const unknownBinaryProjection = structuredClone(projection);
  unknownBinaryProjection.bundle_changes = [{
    ...projection.bundle_changes[0],
    path_before: '.DS_Store',
    path_after: '.DS_Store',
  }];
  const unknownBinary = adapter.reviewWorkbenchFromProjection(
    'People',
    'Saved version',
    unknownBinaryProjection,
    authority,
  ).changes[0];
  assert.equal(unknownBinary.kind, 'file');

  const unknownReason = structuredClone(projection);
  unknownReason.bundle_changes[1].opaque_reason = 'future-opaque-reason';
  assert.throws(
    () => adapter.reviewWorkbenchFromProjection('People', 'Saved version', unknownReason, authority),
    /opaque reason was unsupported/,
  );
  const inventedDiff = structuredClone(projection);
  inventedDiff.bundle_changes[1].verified_text = { source: 'before-after', hunks: [] };
  assert.throws(
    () => adapter.reviewWorkbenchFromProjection('People', 'Saved version', inventedDiff, authority),
    /opaque change invented a text comparison/,
  );
  const contentClassChanged = structuredClone(projection);
  contentClassChanged.bundle_changes[1].opaque_reason = 'content-class-changed';
  contentClassChanged.bundle_changes[1].after = {
    kind: 'binary',
    version_id: textAfter,
    content_digest: 'bb'.repeat(32),
    byte_length: '4097',
    line_count: null,
  };
  assert.equal(
    adapter.reviewWorkbenchFromProjection('People', 'Saved version', contentClassChanged, authority)
      .changes[1].comparisonLimitation,
    'content-class-changed',
  );

  const reviewBuild = await build({
    stdin: {
      contents: `
        import React from "react";
        import { ArtifactReview } from "./src/organisms/artifact-review.tsx";
        import { renderToStaticMarkup } from "react-dom/server";
        module.exports.render = (model) =>
          renderToStaticMarkup(<ArtifactReview model={model} onIntent={() => {}} />);
      `,
      resolveDir: root,
      loader: 'tsx',
    },
    bundle: true,
    format: 'cjs',
    platform: 'node',
    packages: 'external',
    write: false,
  });
  const reviewModule = { exports: {} };
  Function('require', 'module', 'exports', reviewBuild.outputFiles[0].text)(
    createRequire(import.meta.url),
    reviewModule,
    reviewModule.exports,
  );
  const html = reviewModule.exports.render({
    ...model,
    selectedChangeId: opaque.id,
    mode: 'content',
  });
  assert.match(html, /budget\.xlsx/);
  assert.match(html, /policy\.txt/);
  assert.match(html, /Text comparison unavailable/);
  assert.match(html, /exceeds Mesh(?:&#x27;|')s 4,096-line comparison limit/);
  assert.match(html, /saved-version metadata only, not file contents/);
  assert.match(html, /4097 lines/);
  assert.doesNotMatch(html, /Exact text/);
  const visualHtml = reviewModule.exports.render({
    ...model,
    selectedChangeId: opaque.id,
    mode: 'visual',
  });
  assert.match(visualHtml, /Text comparison unavailable/);
  assert.match(visualHtml, /saved-version metadata only, not file contents/);
});

test('the adapter preserves exact bounded text hunks for inline and split diff views', async () => {
  const output = await build({
    entryPoints: [join(root, 'src/models/review-workbench-adapter.ts')],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const adapter = await import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString('base64')}`);
  const beforeVersion = '12'.repeat(32);
  const afterVersion = '34'.repeat(32);
  const projection = {
    bundle: '56'.repeat(32),
    subject_operation: '78'.repeat(32),
    content_complete: true,
    projection_authorizes_approval: false,
    bundle_changes_not_listed: 0,
    bundle_changes: [{
      object_id: '90'.repeat(16),
      path_before: 'policy.txt',
      path_after: 'policy.txt',
      effect: 'content-written',
      body: 'text',
      before: { kind: 'text', version_id: beforeVersion, content_digest: null, byte_length: null, line_count: '2' },
      after: { kind: 'text', version_id: afterVersion, content_digest: null, byte_length: null, line_count: '2' },
      verified_text: {
        source: 'before-after',
        before: { version_id: beforeVersion, content_digest: 'ab'.repeat(32) },
        after: { version_id: afterVersion, content_digest: 'cd'.repeat(32) },
        hunks: [{
          before_start: 1,
          before_len: 2,
          after_start: 1,
          after_len: 2,
          lines: [
            { kind: 'context', before: 1, after: 1, text: 'Benefits policy' },
            { kind: 'removed', before: 2, after: null, text: 'Old allowance' },
            { kind: 'added', before: null, after: 2, text: 'New allowance' },
          ],
        }],
      },
    }],
  };
  const authority = {
    canRenderArtifactPreview: false,
    canInspectExactCopies: false,
    canRecordReview: false,
    canApprove: false,
    canApproveAndExport: false,
    canExportGit: false,
    canExportPrivateCopy: false,
    approvalReason: 'Native authority is disconnected.',
  };
  const model = adapter.reviewWorkbenchFromProjection('People', 'Saved version', projection, authority);
  assert.deepEqual(model.changes[0].diffHunks[0].lines.map(({ kind, before, after }) => ({ kind, before, after })), [
    { kind: 'context', before: 1, after: 1 },
    { kind: 'removed', before: 2, after: null },
    { kind: 'added', before: null, after: 2 },
  ]);
  assert.equal(Object.isFrozen(model.changes[0].diffHunks), true);
  assert.equal(Object.isFrozen(model.changes[0].diffHunks[0].lines), true);

  const wrongVersion = structuredClone(projection);
  wrongVersion.bundle_changes[0].verified_text.after.version_id = 'ef'.repeat(32);
  assert.throws(
    () => adapter.reviewWorkbenchFromProjection('People', 'Saved version', wrongVersion, authority),
    /identity did not match/,
  );
  const discontinuous = structuredClone(projection);
  discontinuous.bundle_changes[0].verified_text.hunks[0].lines[2].after = 3;
  assert.throws(
    () => adapter.reviewWorkbenchFromProjection('People', 'Saved version', discontinuous, authority),
    /not contiguous/,
  );
  const unsafe = structuredClone(projection);
  unsafe.bundle_changes[0].verified_text.hunks[0].lines[2].text = '\u202ereversed';
  assert.throws(
    () => adapter.reviewWorkbenchFromProjection('People', 'Saved version', unsafe, authority),
    /unsafe or unbounded/,
  );
});

test('the review reducer reveals exact text hunks across mixed bundles and never optimistically approves', async () => {
  const output = await build({
    entryPoints: [join(root, 'src/models/review-workbench.ts')],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const modelModule = await import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString('base64')}`);
  const change = Object.freeze({ id: 'change', kind: 'text', diffHunks: Object.freeze([]) });
  const artifact = Object.freeze({ id: 'artifact', kind: 'spreadsheet', diffHunks: Object.freeze([]) });
  const image = Object.freeze({ id: 'image', kind: 'image', diffHunks: Object.freeze([]) });
  const model = Object.freeze({
    changes: Object.freeze([artifact, change, image]),
    selectedChangeId: 'artifact',
    mode: 'visual',
    diffLayout: 'split',
    canApprove: true,
    canExportPrivateCopy: false,
  });
  const textSelected = modelModule.reduceReviewWorkbench(model, { type: 'select-change', changeId: 'change' });
  assert.equal(textSelected.mode, 'content', 'an XLSX-first bundle hid the selected exact text diff');
  const artifactSelected = modelModule.reduceReviewWorkbench(textSelected, { type: 'select-change', changeId: 'artifact' });
  assert.equal(artifactSelected.mode, 'content', 'artifact navigation discarded the person\'s content-view choice');
  const imageSelected = modelModule.reduceReviewWorkbench(textSelected, { type: 'select-change', changeId: 'image' });
  assert.equal(imageSelected.mode, 'visual', 'a raster image remained on its unavailable content surface');
  const inline = modelModule.reduceReviewWorkbench(textSelected, { type: 'change-diff-layout', layout: 'inline' });
  assert.equal(inline.diffLayout, 'inline');
  assert.equal(Object.isFrozen(inline), true);
  const refreshedAuthority = Object.freeze({
    ...model,
    canApprove: false,
    canExportPrivateCopy: true,
  });
  const reconciled = modelModule.reconcileReviewWorkbenchProjection(inline, refreshedAuthority);
  assert.equal(reconciled.selectedChangeId, 'change');
  assert.equal(reconciled.mode, 'content');
  assert.equal(reconciled.diffLayout, 'inline');
  assert.equal(reconciled.canApprove, false, 'a local review choice retained stale native authority');
  assert.equal(reconciled.canExportPrivateCopy, true);
  assert.equal(Object.isFrozen(reconciled), true);
  const removedSelection = modelModule.reconcileReviewWorkbenchProjection(inline, Object.freeze({
    ...refreshedAuthority,
    changes: Object.freeze([artifact]),
    selectedChangeId: 'artifact',
    mode: 'visual',
    diffLayout: 'split',
  }));
  assert.equal(removedSelection.selectedChangeId, 'artifact');
  assert.equal(removedSelection.mode, 'visual');
  assert.equal(removedSelection.diffLayout, 'split');
  const approval = modelModule.reduceReviewWorkbench(inline, { type: 'approve-version' });
  assert.equal(approval, inline);
  assert.throws(
    () => modelModule.reduceReviewWorkbench(model, { type: 'select-change', changeId: 'missing' }),
    /not in the frozen review model/,
  );
});

test('the production island accepts only fresh coordinator-owned action envelopes', async () => {
  const output = await build({
    entryPoints: [join(root, 'src/models/review-workbench-envelope.ts')],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const envelopeModule = await import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString('base64')}`);
  const projection = {
    bundle: '11'.repeat(32),
    subject_operation: '22'.repeat(32),
    content_complete: true,
    projection_authorizes_approval: false,
    bundle_changes_not_listed: 0,
    bundle_changes: [{
      object_id: '33'.repeat(16),
      path_before: 'brief.pdf',
      path_after: 'brief.pdf',
      effect: 'content-written',
      body: 'binary',
      before: { kind: 'binary', version_id: '44'.repeat(32), content_digest: '55'.repeat(32), byte_length: '10', line_count: null },
      after: { kind: 'binary', version_id: '66'.repeat(32), content_digest: '77'.repeat(32), byte_length: '12', line_count: null },
      verified_text: null,
    }],
  };
  const value = {
    generation: 8,
    workspaceName: 'Quarterly planning',
    versionLabel: 'Saved 22…',
    projection,
    authority: {
      canRenderArtifactPreview: true,
      canInspectExactCopies: false,
      canRecordReview: false,
      canApprove: false,
      canApproveAndExport: false,
      canExportGit: false,
      canExportPrivateCopy: false,
      approvalReason: 'Use the current review for native actions.',
    },
  };
  const accepted = envelopeModule.reviewWorkbenchEnvelope(value, 7);
  assert.equal(accepted.generation, 8);
  assert.equal(accepted.bundle, projection.bundle);
  assert.equal(Object.isFrozen(accepted), true);
  assert.throws(() => envelopeModule.reviewWorkbenchEnvelope(value, 8), /stale/);
  const missingGitAuthority = structuredClone(value);
  delete missingGitAuthority.authority.canExportGit;
  assert.throws(
    () => envelopeModule.reviewWorkbenchEnvelope({ ...missingGitAuthority, generation: 9 }, 8),
    /unrecognized or missing fields/,
  );
  const actionable = envelopeModule.reviewWorkbenchEnvelope({
    ...value,
    generation: 9,
    authority: { ...value.authority, canApprove: true },
  }, 8);
  assert.equal(actionable.model.canApprove, true);
  const exportable = envelopeModule.reviewWorkbenchEnvelope({
    ...value,
    generation: 10,
    authority: { ...value.authority, canExportPrivateCopy: true },
  }, 9);
  assert.equal(exportable.model.canExportPrivateCopy, true);
  assert.throws(
    () => envelopeModule.reviewWorkbenchEnvelope({ ...value, generation: 9, action: 'approve' }, 8),
    /unrecognized or missing fields/,
  );
  assert.throws(
    () => envelopeModule.reviewWorkbenchEnvelope({
      ...value,
      generation: 9,
      authority: { ...value.authority, nativeApproval: true },
    }, 8),
    /unrecognized or missing fields/,
  );
  assert.throws(
    () => envelopeModule.reviewWorkbenchEnvelope({
      ...value,
      generation: 9,
      authority: { ...value.authority, canApprove: 'yes' },
    }, 8),
    /malformed review authority/,
  );
  assert.throws(
    () => envelopeModule.reviewWorkbenchEnvelope({
      ...value,
      generation: 9,
      authority: { ...value.authority, canExportPrivateCopy: 'yes' },
    }, 8),
    /malformed review authority/,
  );
});

test('the production route contract is closed, complete, and owns static page topology', async () => {
  const output = await build({
    entryPoints: [join(root, 'src/models/production-route.ts')],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const route = await import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString('base64')}`);
  const expected = [
    ['workspaces', 'Workspaces', 'Workspaces', false, 'workspace-entry-next', 'host-only', [['workspace-entry', 'Preparing workspace controls…']]],
    ['import', 'Import', 'Import folder', false, 'import-workbench-next', 'import-chooser', [['import-workbench', 'Preparing folder import…']]],
    ['current', 'Current', 'Current workspace', true, 'workspace-current-next', 'host-only', [['workspace-overview', 'Preparing workspace summary…'], ['workspace-current', 'Preparing current workspace…']]],
    ['files', 'Files', 'Files', true, 'workspace-files-next', 'host-only', [['workspace-files', 'Preparing files…']]],
    ['changes', 'Changes', 'Changes', true, 'workspace-changes-next', 'changes-workflow', [['workspace-changes', 'Preparing changes…']]],
    ['review', 'Review', 'Review', true, 'review-workbench-next', 'host-only', [['review-workbench', 'Preparing review…']]],
    ['versions', 'Versions', 'Versions', true, 'workspace-versions-next', 'saved-version', [['workspace-versions', 'Preparing versions…']]],
    ['update', 'Update destination', 'Update destination', true, 'workspace-destination-next', 'update-destination', [['workspace-destination', 'Preparing destination…']]],
    ['restore', 'Restore', 'Restore', true, 'workspace-restore-next', 'host-only', [['workspace-restore', 'Preparing restore…']]],
  ];
  assert.deepEqual(
    route.productionRoutes.map((item) => [
      item.id,
      item.label,
      item.pageLabel,
      item.workspaceRequired,
      item.focusHostId,
      item.focusSelectorPolicy,
      item.slots.map((slot) => [slot.name, slot.loadingLabel]),
    ]),
    expected,
  );
  assert.equal(new Set(route.productionRoutes.map((item) => item.id)).size, 9);
  assert.equal(Object.isFrozen(route.productionRoutes), true);
  for (const item of route.productionRoutes) {
    assert.deepEqual(
      Object.keys(item),
      ['id', 'label', 'pageLabel', 'workspaceRequired', 'focusHostId', 'focusSelectorPolicy', 'slots'],
    );
    assert.equal(Object.isFrozen(item), true);
    assert.equal(Object.isFrozen(item.slots), true);
    assert.strictEqual(route.productionRoute(item.id), item);
    assert.equal(route.productionRouteIsAvailable(item.id, true), true);
    assert.equal(route.productionRouteIsAvailable(item.id, false), !item.workspaceRequired);
  }
  for (const forged of [null, '', 'Review', 'invented', 1, {}, ['review']]) {
    assert.equal(route.productionRoute(forged), null);
    assert.equal(route.productionRouteIsAvailable(forged, true), false);
  }
  const selectedVersion = `[data-mesh-version-operation="${'a'.repeat(64)}"]:not(:disabled)`;
  assert.equal(route.productionRouteFocusSelector('versions', null), null);
  assert.equal(
    route.productionRouteFocusSelector('versions', '[role="radio"][tabindex="0"]:not(:disabled)'),
    '[role="radio"][tabindex="0"]:not(:disabled)',
  );
  assert.equal(route.productionRouteFocusSelector('versions', selectedVersion), selectedVersion);
  for (const forged of ['[', 'button', selectedVersion.replace('a', 'A'), 1, {}, undefined]) {
    assert.equal(route.productionRouteFocusSelector('versions', forged), undefined);
  }
  assert.equal(route.productionRouteFocusSelector('review', '[role="button"]'), undefined);
  assert.equal(
    route.productionRouteFocusSelector('import', '[data-mesh-import-choose]'),
    '[data-mesh-import-choose]',
  );
  for (const forged of ['[', 'button', '[data-mesh-import-preview]', '[data-mesh-proof="destination-choose"]']) {
    assert.equal(route.productionRouteFocusSelector('import', forged), undefined);
  }
  for (const selector of [
    'textarea',
    '[data-mesh-work-action="scan-files"]',
    '[data-mesh-work-action="save-all-private"]',
    '[data-mesh-work-field="missingSource"]',
    '[data-mesh-native-queue]',
  ]) {
    assert.equal(route.productionRouteFocusSelector('changes', selector), selector);
  }
  for (const forged of ['[', 'button', '[data-mesh-work-field="moveTarget"]', '[data-mesh-work-action="delete-entry"]']) {
    assert.equal(route.productionRouteFocusSelector('changes', forged), undefined);
  }

  const html = readFileSync(join(root, '..', 'ui', 'index.html'), 'utf8');
  const routedHosts = route.productionRoutes.flatMap((item) => item.slots.map((slot) => [item.id, slot.name]));
  assert.equal(routedHosts.length, 10, 'the Current page must retain both of its exact island slots');
  for (const item of route.productionRoutes) {
    assert.equal(
      (html.match(new RegExp(`id="${item.focusHostId}"`, 'g')) || []).length,
      1,
      `${item.focusHostId} must identify exactly one focus host`,
    );
  }
  for (const [, slot] of routedHosts) {
    assert.equal((html.match(new RegExp(`slot="${slot}"`, 'g')) || []).length, 1, `${slot} must have one static host`);
  }

  const store = readFileSync(join(root, 'src/models/production-shell-store.ts'), 'utf8');
  const navigation = readFileSync(join(root, 'src/organisms/production-navigation.tsx'), 'utf8');
  const page = readFileSync(join(root, 'src/pages/production-workspace-page.tsx'), 'utf8');
  const view = readFileSync(join(root, 'src/views/workspace-view.tsx'), 'utf8');
  assert.doesNotMatch(store, /\.\.\/views\//, 'a model must not import route truth from a view');
  assert.match(store, /productionRouteIsAvailable/);
  assert.match(navigation, /productionRoutes/);
  assert.match(navigation, /productionRouteIsAvailable\(page\.id, workspaceReady\)/);
  assert.doesNotMatch(navigation, /const pages/);
  assert.match(page, /productionRoutes\.map/);
  assert.doesNotMatch(page, /const hostIds|activePage === "workspaces"|activePage === "restore"/);
  assert.doesNotMatch(view, /export type WorkspacePageId/);
});

test('Current is a dedicated route page that preserves its two-slot contract', async () => {
  const currentPage = readFileSync(join(root, 'src/pages/current-workspace-page.tsx'), 'utf8');
  const productionPage = readFileSync(join(root, 'src/pages/production-workspace-page.tsx'), 'utf8');
  assert.match(currentPage, /Extract<ProductionRoute, \{ id: "current" \}>/);
  assert.match(currentPage, /<WorkspaceView[\s\S]*<div className="grid gap-4">[\s\S]*route\.slots\.map/);
  assert.match(productionPage, /import \{ CurrentWorkspacePage \}/);
  assert.match(
    productionPage,
    /case "current":[\s\S]*<CurrentWorkspacePage[^>]*route=\{route\}/,
    'the route registry must select the dedicated Current page',
  );
  assert.doesNotMatch(
    productionPage,
    /route\.slots\.map/,
    'the shell must not retain the Current page slot composition inline',
  );

  const output = await build({
    stdin: {
      contents: `
        import React from "react";
        import { renderToStaticMarkup } from "react-dom/server";
        import { CurrentWorkspacePage } from "./src/pages/current-workspace-page";
        import { productionRoutes } from "./src/models/production-route";
        const route = productionRoutes.find((candidate) => candidate.id === "current");
        module.exports = {
          render: (active, overviewFailed, currentFailed) => renderToStaticMarkup(
            <CurrentWorkspacePage
              active={active}
              route={route}
              surfaceFailures={{
                "workspace-overview": overviewFailed,
                "workspace-current": currentFailed,
              }}
            />
          ),
        };
      `,
      resolveDir: root,
      loader: 'tsx',
    },
    bundle: true,
    format: 'cjs',
    platform: 'node',
    packages: 'external',
    write: false,
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', output.outputFiles[0].text)(
    createRequire(import.meta.url),
    module,
    module.exports,
  );

  const active = module.exports.render(true, false, false);
  assert.match(active, /aria-label="Current workspace"/);
  assert.match(active, /data-mesh-page-active="true"/);
  assert.doesNotMatch(active, /<section[^>]* hidden=""/);
  assert.match(active, /<div class="grid gap-4">/);
  assert.ok(
    active.indexOf('slot name="workspace-overview"') < active.indexOf('slot name="workspace-current"'),
    'Overview must remain before Current in the route-owned layout',
  );
  assert.match(active, /Preparing workspace summary…/);
  assert.match(active, /Preparing current workspace…/);

  const inactive = module.exports.render(false, false, false);
  assert.match(inactive, /<section[^>]* hidden=""[^>]*data-mesh-page-active="false"/);

  const failed = module.exports.render(true, true, false);
  assert.match(failed, /data-mesh-slot-failure="workspace-overview" role="alert"/);
  assert.match(failed, /This page could not finish rendering safely\./);
  assert.match(failed, /Reload Mesh/);
  assert.doesNotMatch(failed, /data-mesh-slot-failure="workspace-current"/);

  const currentFailed = module.exports.render(true, false, true);
  assert.match(currentFailed, /data-mesh-slot-failure="workspace-current" role="alert"/);
  assert.match(currentFailed, /This page could not finish rendering safely\./);
  assert.match(currentFailed, /Reload Mesh/);
  assert.doesNotMatch(currentFailed, /data-mesh-slot-failure="workspace-overview"/);
});

test('Restore is a dedicated route page with one registry-owned island boundary', async () => {
  const restorePage = readFileSync(join(root, 'src/pages/restore-workspace-page.tsx'), 'utf8');
  const productionPage = readFileSync(join(root, 'src/pages/production-workspace-page.tsx'), 'utf8');
  assert.match(restorePage, /Extract<ProductionRoute, \{ id: "restore" \}>/);
  assert.match(restorePage, /<WorkspaceView[\s\S]*route\.pageLabel[\s\S]*<IslandSlot/);
  assert.match(productionPage, /import \{ RestoreWorkspacePage \}/);
  assert.match(
    productionPage,
    /case "restore":[\s\S]*<RestoreWorkspacePage[^>]*route=\{route\}/,
    'the route registry must select the dedicated Restore page',
  );

  const output = await build({
    stdin: {
      contents: `
        import React from "react";
        import { renderToStaticMarkup } from "react-dom/server";
        import { RestoreWorkspacePage } from "./src/pages/restore-workspace-page";
        import { productionRoutes } from "./src/models/production-route";
        const route = productionRoutes.find((candidate) => candidate.id === "restore");
        module.exports = {
          route,
          render: (active, failed) => renderToStaticMarkup(
            <RestoreWorkspacePage
              active={active}
              route={route}
              surfaceFailures={{ "workspace-restore": failed }}
            />
          ),
        };
      `,
      resolveDir: root,
      loader: 'tsx',
    },
    bundle: true,
    format: 'cjs',
    platform: 'node',
    packages: 'external',
    write: false,
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', output.outputFiles[0].text)(
    createRequire(import.meta.url),
    module,
    module.exports,
  );

  assert.equal(module.exports.route.pageLabel, 'Restore');
  assert.equal(module.exports.route.focusHostId, 'workspace-restore-next');
  assert.deepEqual(module.exports.route.slots.map((slot) => slot.name), ['workspace-restore']);
  const active = module.exports.render(true, false);
  assert.match(active, /aria-label="Restore"/);
  assert.match(active, /data-mesh-page-active="true"/);
  assert.doesNotMatch(active, /<section[^>]* hidden=""/);
  assert.match(active, /slot name="workspace-restore"/);
  assert.match(active, /Preparing restore…/);
  assert.doesNotMatch(active, /data-mesh-slot-failure="workspace-restore"/);

  const inactive = module.exports.render(false, false);
  assert.match(inactive, /<section[^>]* hidden=""[^>]*data-mesh-page-active="false"/);

  const failed = module.exports.render(true, true);
  assert.match(failed, /data-mesh-slot-failure="workspace-restore" role="alert"/);
  assert.match(failed, /This page could not finish rendering safely\./);
  assert.match(failed, /Reload Mesh/);
  assert.doesNotMatch(failed, /Preparing restore…/);
});

test('Versions is a dedicated route page with its saved-version focus contract', async () => {
  const versionsPage = readFileSync(join(root, 'src/pages/versions-workspace-page.tsx'), 'utf8');
  const productionPage = readFileSync(join(root, 'src/pages/production-workspace-page.tsx'), 'utf8');
  assert.match(versionsPage, /Extract<ProductionRoute, \{ id: "versions" \}>/);
  assert.match(versionsPage, /<WorkspaceView[\s\S]*route\.pageLabel[\s\S]*<IslandSlot/);
  assert.match(productionPage, /import \{ VersionsWorkspacePage \}/);
  assert.match(
    productionPage,
    /case "versions":[\s\S]*<VersionsWorkspacePage[^>]*route=\{route\}/,
    'the route registry must select the dedicated Versions page',
  );

  const output = await build({
    stdin: {
      contents: `
        import React from "react";
        import { renderToStaticMarkup } from "react-dom/server";
        import { VersionsWorkspacePage } from "./src/pages/versions-workspace-page";
        import { productionRoutes } from "./src/models/production-route";
        const route = productionRoutes.find((candidate) => candidate.id === "versions");
        module.exports = {
          route,
          render: (active, failed) => renderToStaticMarkup(
            <VersionsWorkspacePage
              active={active}
              route={route}
              surfaceFailures={{ "workspace-versions": failed }}
            />
          ),
        };
      `,
      resolveDir: root,
      loader: 'tsx',
    },
    bundle: true,
    format: 'cjs',
    platform: 'node',
    packages: 'external',
    write: false,
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', output.outputFiles[0].text)(
    createRequire(import.meta.url),
    module,
    module.exports,
  );

  assert.equal(module.exports.route.pageLabel, 'Versions');
  assert.equal(module.exports.route.focusHostId, 'workspace-versions-next');
  assert.equal(module.exports.route.focusSelectorPolicy, 'saved-version');
  assert.deepEqual(module.exports.route.slots.map((slot) => slot.name), ['workspace-versions']);
  const active = module.exports.render(true, false);
  assert.match(active, /aria-label="Versions"/);
  assert.match(active, /data-mesh-page-active="true"/);
  assert.doesNotMatch(active, /<section[^>]* hidden=""/);
  assert.match(active, /slot name="workspace-versions"/);
  assert.match(active, /Preparing versions…/);
  assert.doesNotMatch(active, /data-mesh-slot-failure="workspace-versions"/);

  const inactive = module.exports.render(false, false);
  assert.match(inactive, /<section[^>]* hidden=""[^>]*data-mesh-page-active="false"/);

  const failed = module.exports.render(true, true);
  assert.match(failed, /data-mesh-slot-failure="workspace-versions" role="alert"/);
  assert.match(failed, /This page could not finish rendering safely\./);
  assert.match(failed, /Reload Mesh/);
  assert.doesNotMatch(failed, /Preparing versions…/);
});

test('Review is a dedicated route page that leaves review-state rendering inside its organism', async () => {
  const reviewPage = readFileSync(join(root, 'src/pages/review-workspace-page.tsx'), 'utf8');
  const productionPage = readFileSync(join(root, 'src/pages/production-workspace-page.tsx'), 'utf8');
  const reviewStatusOrganism = readFileSync(join(root, 'src/organisms/review-page-status.tsx'), 'utf8');
  assert.match(reviewPage, /Extract<ProductionRoute, \{ id: "review" \}>/);
  assert.match(reviewPage, /<WorkspaceView[\s\S]*route\.pageLabel[\s\S]*<IslandSlot/);
  assert.doesNotMatch(reviewPage, /ReviewState|ready|empty|unavailable|onIntent/);
  assert.match(reviewStatusOrganism, /ReviewPageStatusModel/);
  assert.match(reviewStatusOrganism, /review-\$\{model\.state\}/);
  assert.match(reviewStatusOrganism, /model\.title/);
  assert.match(reviewStatusOrganism, /model\.description/);
  assert.match(productionPage, /import \{ ReviewWorkspacePage \}/);
  assert.match(
    productionPage,
    /case "review":[\s\S]*<ReviewWorkspacePage[^>]*route=\{route\}/,
    'the route registry must select the dedicated Review page',
  );

  const output = await build({
    stdin: {
      contents: `
        import React from "react";
        import { renderToStaticMarkup } from "react-dom/server";
        import { ReviewWorkspacePage } from "./src/pages/review-workspace-page";
        import { productionRoutes } from "./src/models/production-route";
        const route = productionRoutes.find((candidate) => candidate.id === "review");
        module.exports = {
          route,
          render: (active, failed) => renderToStaticMarkup(
            <ReviewWorkspacePage
              active={active}
              route={route}
              surfaceFailures={{ "review-workbench": failed }}
            />
          ),
        };
      `,
      resolveDir: root,
      loader: 'tsx',
    },
    bundle: true,
    format: 'cjs',
    platform: 'node',
    packages: 'external',
    write: false,
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', output.outputFiles[0].text)(
    createRequire(import.meta.url),
    module,
    module.exports,
  );

  assert.equal(module.exports.route.pageLabel, 'Review');
  assert.equal(module.exports.route.focusHostId, 'review-workbench-next');
  assert.deepEqual(module.exports.route.slots.map((slot) => slot.name), ['review-workbench']);
  const active = module.exports.render(true, false);
  assert.match(active, /aria-label="Review"/);
  assert.match(active, /data-mesh-page-active="true"/);
  assert.doesNotMatch(active, /<section[^>]* hidden=""/);
  assert.match(active, /slot name="review-workbench"/);
  assert.match(active, /Preparing review…/);
  assert.doesNotMatch(active, /data-mesh-slot-failure="review-workbench"/);

  const inactive = module.exports.render(false, false);
  assert.match(inactive, /<section[^>]* hidden=""[^>]*data-mesh-page-active="false"/);

  const failed = module.exports.render(true, true);
  assert.match(failed, /data-mesh-slot-failure="review-workbench" role="alert"/);
  assert.match(failed, /This page could not finish rendering safely\./);
  assert.match(failed, /Reload Mesh/);
  assert.doesNotMatch(failed, /Preparing review…/);
});

test('Files and Changes are independent dedicated route pages', async () => {
  const filesPage = readFileSync(join(root, 'src/pages/files-workspace-page.tsx'), 'utf8');
  const changesPage = readFileSync(join(root, 'src/pages/changes-workspace-page.tsx'), 'utf8');
  const productionPage = readFileSync(join(root, 'src/pages/production-workspace-page.tsx'), 'utf8');
  assert.match(filesPage, /Extract<ProductionRoute, \{ id: "files" \}>/);
  assert.match(changesPage, /Extract<ProductionRoute, \{ id: "changes" \}>/);
  for (const page of [filesPage, changesPage]) {
    assert.match(page, /<WorkspaceView[\s\S]*route\.pageLabel[\s\S]*<IslandSlot/);
    assert.doesNotMatch(page, /document\.|onIntent|requestProductionPage/);
  }
  assert.match(productionPage, /import \{ FilesWorkspacePage \}/);
  assert.match(productionPage, /import \{ ChangesWorkspacePage \}/);
  assert.match(
    productionPage,
    /case "files":[\s\S]*<FilesWorkspacePage[^>]*route=\{route\}/,
    'the route registry must select the dedicated Files page',
  );
  assert.match(
    productionPage,
    /case "changes":[\s\S]*<ChangesWorkspacePage[^>]*route=\{route\}/,
    'the route registry must select the dedicated Changes page',
  );

  const output = await build({
    stdin: {
      contents: `
        import React from "react";
        import { renderToStaticMarkup } from "react-dom/server";
        import { FilesWorkspacePage } from "./src/pages/files-workspace-page";
        import { ChangesWorkspacePage } from "./src/pages/changes-workspace-page";
        import { productionRoutes } from "./src/models/production-route";
        const filesRoute = productionRoutes.find((candidate) => candidate.id === "files");
        const changesRoute = productionRoutes.find((candidate) => candidate.id === "changes");
        const render = (Page, route, active, failed) => renderToStaticMarkup(
          <Page
            active={active}
            route={route}
            surfaceFailures={{ [route.slots[0].name]: failed }}
          />
        );
        module.exports = {
          filesRoute,
          changesRoute,
          renderFiles: (active, failed) => render(FilesWorkspacePage, filesRoute, active, failed),
          renderChanges: (active, failed) => render(ChangesWorkspacePage, changesRoute, active, failed),
        };
      `,
      resolveDir: root,
      loader: 'tsx',
    },
    bundle: true,
    format: 'cjs',
    platform: 'node',
    packages: 'external',
    write: false,
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', output.outputFiles[0].text)(
    createRequire(import.meta.url),
    module,
    module.exports,
  );

  const cases = [
    [module.exports.filesRoute, module.exports.renderFiles, 'Files', 'workspace-files-next', 'workspace-files', 'Preparing files…'],
    [module.exports.changesRoute, module.exports.renderChanges, 'Changes', 'workspace-changes-next', 'workspace-changes', 'Preparing changes…'],
  ];
  for (const [route, render, label, focusHostId, slot, loadingLabel] of cases) {
    assert.equal(route.pageLabel, label);
    assert.equal(route.focusHostId, focusHostId);
    assert.deepEqual(route.slots.map((candidate) => candidate.name), [slot]);

    const active = render(true, false);
    assert.match(active, new RegExp(`aria-label="${label}"`));
    assert.match(active, /data-mesh-page-active="true"/);
    assert.doesNotMatch(active, /<section[^>]* hidden=""/);
    assert.match(active, new RegExp(`slot name="${slot}"`));
    assert.match(active, new RegExp(loadingLabel));
    assert.doesNotMatch(active, new RegExp(`data-mesh-slot-failure="${slot}"`));

    const inactive = render(false, false);
    assert.match(inactive, /<section[^>]* hidden=""[^>]*data-mesh-page-active="false"/);

    const failed = render(true, true);
    assert.match(failed, new RegExp(`data-mesh-slot-failure="${slot}" role="alert"`));
    assert.match(failed, /This page could not finish rendering safely\./);
    assert.match(failed, /Reload Mesh/);
    assert.doesNotMatch(failed, new RegExp(loadingLabel));
  }
});

test('the production page layer maps every registry route to a dedicated module', async () => {
  const workspacesPage = readFileSync(join(root, 'src/pages/workspaces-workspace-page.tsx'), 'utf8');
  const importPage = readFileSync(join(root, 'src/pages/import-workspace-page.tsx'), 'utf8');
  const updatePage = readFileSync(join(root, 'src/pages/update-destination-workspace-page.tsx'), 'utf8');
  const productionPage = readFileSync(join(root, 'src/pages/production-workspace-page.tsx'), 'utf8');
  assert.match(workspacesPage, /Extract<ProductionRoute, \{ id: "workspaces" \}>/);
  assert.match(importPage, /Extract<ProductionRoute, \{ id: "import" \}>/);
  assert.match(updatePage, /Extract<ProductionRoute, \{ id: "update" \}>/);
  for (const page of [importPage, updatePage]) {
    assert.match(page, /<WorkspaceView[\s\S]*route\.pageLabel[\s\S]*<IslandSlot/);
    assert.doesNotMatch(page, /document\.|onIntent|requestProductionPage/);
  }
  assert.match(productionPage, /import \{ ImportWorkspacePage \}/);
  assert.match(productionPage, /import \{ UpdateDestinationWorkspacePage \}/);

  const routePages = new Map([
    ['workspaces', ['workspaces-workspace-page.tsx', 'WorkspacesWorkspacePage']],
    ['import', ['import-workspace-page.tsx', 'ImportWorkspacePage']],
    ['current', ['current-workspace-page.tsx', 'CurrentWorkspacePage']],
    ['files', ['files-workspace-page.tsx', 'FilesWorkspacePage']],
    ['changes', ['changes-workspace-page.tsx', 'ChangesWorkspacePage']],
    ['review', ['review-workspace-page.tsx', 'ReviewWorkspacePage']],
    ['versions', ['versions-workspace-page.tsx', 'VersionsWorkspacePage']],
    ['update', ['update-destination-workspace-page.tsx', 'UpdateDestinationWorkspacePage']],
    ['restore', ['restore-workspace-page.tsx', 'RestoreWorkspacePage']],
  ]);
  for (const [id, [page, component]] of routePages) {
    assert.equal(statSync(join(root, 'src/pages', page)).isFile(), true, `${page} must remain a page module`);
    assert.match(
      productionPage,
      new RegExp(`case "${id}":[\\s\\S]*<${component}[^>]*route=\\{route\\}`),
      `${id} must map to ${component}`,
    );
  }
  assert.match(productionPage, /route satisfies never/);
  assert.doesNotMatch(productionPage, /const slot = route\.slots\[0\]/);

  const output = await build({
    stdin: {
      contents: `
        import React from "react";
        import { renderToStaticMarkup } from "react-dom/server";
        import { WorkspacesWorkspacePage } from "./src/pages/workspaces-workspace-page";
        import { ImportWorkspacePage } from "./src/pages/import-workspace-page";
        import { UpdateDestinationWorkspacePage } from "./src/pages/update-destination-workspace-page";
        import { productionRoutes } from "./src/models/production-route";
        const workspacesRoute = productionRoutes.find((candidate) => candidate.id === "workspaces");
        const importRoute = productionRoutes.find((candidate) => candidate.id === "import");
        const updateRoute = productionRoutes.find((candidate) => candidate.id === "update");
        const render = (Page, route, active, failed) => renderToStaticMarkup(
          <Page
            active={active}
            route={route}
            surfaceFailures={{ [route.slots[0].name]: failed }}
          />
        );
        module.exports = {
          routeIds: productionRoutes.map((route) => route.id),
          workspacesRoute,
          importRoute,
          updateRoute,
          renderWorkspaces: (active, failed) => render(WorkspacesWorkspacePage, workspacesRoute, active, failed),
          renderImport: (active, failed) => render(ImportWorkspacePage, importRoute, active, failed),
          renderUpdate: (active, failed) => render(UpdateDestinationWorkspacePage, updateRoute, active, failed),
        };
      `,
      resolveDir: root,
      loader: 'tsx',
    },
    bundle: true,
    format: 'cjs',
    platform: 'node',
    packages: 'external',
    write: false,
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', output.outputFiles[0].text)(
    createRequire(import.meta.url),
    module,
    module.exports,
  );
  assert.deepEqual(module.exports.routeIds, [...routePages.keys()]);

  const cases = [
    [module.exports.workspacesRoute, module.exports.renderWorkspaces, 'Workspaces', 'workspace-entry-next', 'workspace-entry', 'Preparing workspace controls…'],
    [module.exports.importRoute, module.exports.renderImport, 'Import folder', 'import-workbench-next', 'import-workbench', 'Preparing folder import…'],
    [module.exports.updateRoute, module.exports.renderUpdate, 'Update destination', 'workspace-destination-next', 'workspace-destination', 'Preparing destination…'],
  ];
  for (const [route, render, label, focusHostId, slot, loadingLabel] of cases) {
    assert.equal(route.pageLabel, label);
    assert.equal(route.focusHostId, focusHostId);
    assert.deepEqual(route.slots.map((candidate) => candidate.name), [slot]);

    const active = render(true, false);
    assert.match(active, new RegExp(`aria-label="${label}"`));
    assert.match(active, /data-mesh-page-active="true"/);
    assert.doesNotMatch(active, /<section[^>]* hidden=""/);
    assert.match(active, new RegExp(`slot name="${slot}"`));
    assert.match(active, new RegExp(loadingLabel));
    assert.doesNotMatch(active, new RegExp(`data-mesh-slot-failure="${slot}"`));

    const inactive = render(false, false);
    assert.match(inactive, /<section[^>]* hidden=""[^>]*data-mesh-page-active="false"/);

    const failed = render(true, true);
    assert.match(failed, new RegExp(`data-mesh-slot-failure="${slot}" role="alert"`));
    assert.match(failed, /This page could not finish rendering safely\./);
    assert.match(failed, /Reload Mesh/);
    assert.doesNotMatch(failed, new RegExp(loadingLabel));
  }
});

test('the production Tauri frontend exposes only the React page shell', () => {
  const tauri = JSON.parse(readFileSync(join(root, '..', 'src-tauri', 'tauri.conf.json'), 'utf8'));
  const desktopPackage = JSON.parse(readFileSync(join(root, '..', 'package.json'), 'utf8'));
  const legacyHtml = readFileSync(join(root, '..', 'ui', 'index.html'), 'utf8');
  const legacyController = readFileSync(join(root, '..', 'ui', 'app.js'), 'utf8');
  const island = readFileSync(join(root, 'src', 'island.tsx'), 'utf8');
  const productionPage = readFileSync(join(root, 'src', 'pages', 'production-workspace-page.tsx'), 'utf8');
  const productionNavigation = readFileSync(join(root, 'src', 'organisms', 'production-navigation.tsx'), 'utf8');
  const productionRoute = readFileSync(join(root, 'src', 'models', 'production-route.ts'), 'utf8');
  const productionLayout = readFileSync(join(root, 'src', 'layouts', 'production-workspace-layout.tsx'), 'utf8');
  const productionFocus = readFileSync(join(root, 'src', 'lib', 'production-focus.ts'), 'utf8');
  const workspaceView = readFileSync(join(root, 'src', 'views', 'workspace-view.tsx'), 'utf8');
  assert.equal(tauri.build.frontendDist, '../ui');
  assert.equal(tauri.build.beforeBuildCommand, 'npm run ui:next:build:island');
  assert.equal(desktopPackage.scripts['ui:next:build:island'], 'npm --prefix ui-next run build:island');
  assert.match(legacyHtml, /id="mesh-app-next"/);
  assert.doesNotMatch(legacyHtml, /id="main-content"|coordinator-only/);
  assert.doesNotMatch(legacyController, /\$\('main-content'\)|getElementById\(['"]main-content['"]\)/);
  const staticIslandHosts = [
    ['workspace-chrome-next', 'workspace-header', 'div'],
    ['workspace-entry-next', 'workspace-entry', 'div'],
    ['import-workbench-next', 'import-workbench', 'div'],
    ['workspace-overview-next', 'workspace-overview', 'section'],
    ['workspace-current-next', 'workspace-current', 'div'],
    ['workspace-files-next', 'workspace-files', 'div'],
    ['workspace-changes-next', 'workspace-changes', 'div'],
    ['review-workbench-next', 'review-workbench', 'div'],
    ['workspace-versions-next', 'workspace-versions', 'div'],
    ['workspace-destination-next', 'workspace-destination', 'div'],
    ['workspace-restore-next', 'workspace-restore', 'div'],
    ['confirmation-dialog-next', 'confirmation-dialog', 'div'],
  ];
  const shellStart = legacyHtml.indexOf('<div id="mesh-app-next" aria-label="Mesh">');
  const bodyEnd = legacyHtml.lastIndexOf('</body>');
  const staticShell = legacyHtml.slice(shellStart, bodyEnd).trim();
  assert.ok(shellStart >= 0 && bodyEnd > shellStart);
  for (const [id, slot] of staticIslandHosts) {
    assert.equal((legacyHtml.match(new RegExp(`id="${id}"`, 'g')) || []).length, 1, `${id} must be unique`);
    assert.match(staticShell, new RegExp(`id="${id}"[^>]*slot="${slot}"`), `${id} must have its static slot`);
  }
  const directStaticChildren = staticIslandHosts
    .map(([id, slot, tag]) => `<${tag} id="${id}"[^>]*slot="${slot}"[^>]*><\\/${tag}>`)
    .join('\\s*');
  assert.match(
    staticShell,
    new RegExp(`^<div id="mesh-app-next" aria-label="Mesh">\\s*${directStaticChildren}\\s*<\\/div>\\s*$`),
    'every island host must be a direct, statically slotted child of the React shell',
  );
  assert.match(legacyHtml, /id="review-workbench-next"/);
  assert.match(legacyHtml, /id="workspace-chrome-next"/);
  assert.doesNotMatch(legacyHtml, /id="workspace-navigation-next"/);
  assert.match(legacyHtml, /id="workspace-overview-next"/);
  assert.doesNotMatch(legacyHtml, /id="next-action-card"|id="next-action-button"|id="next-version-button"/);
  assert.doesNotMatch(legacyController, /\$\('next-action-card'\)|\$\('next-action-button'\)|\$\('next-version-button'\)/);
  assert.doesNotMatch(
    legacyHtml,
    /id="recent-workspace"|id="recent-workspace-hint"|id="open-recent-workspace"|id="forget-recent-workspace"/,
  );
  assert.doesNotMatch(
    legacyController,
    /\$\('recent-workspace'\)|\$\('recent-workspace-hint'\)|\$\('open-recent-workspace'\)|\$\('forget-recent-workspace'\)/,
  );
  assert.match(workspaceView, /failed[\s\S]*This page could not finish rendering safely[\s\S]*Reload Mesh/);
  assert.match(legacyHtml, /id="workspace-versions-next"/);
  assert.match(legacyHtml, /id="workspace-restore-next"/);
  assert.match(legacyHtml, /review-workbench-next\/review-workbench-island\.js/);
  assert.match(island, /<ProductionWorkspacePage \/>/);
  assert.doesNotMatch(island, /productionHost\.append\(surface\)/);
  assert.doesNotMatch(island, /surface\.slot\s*=|Object\.entries\(slots\)/);
  assert.match(island, /children\.length === STATIC_ISLAND_HOSTS\.length/);
  assert.match(island, /surface\.id === id[\s\S]*surface\.getAttribute\("slot"\) === slot[\s\S]*querySelectorAll\(`#\$\{id\}`\)\.length === 1/);
  assert.match(island, /data-mesh-react-shell-active/);
  assert.ok(
    island.indexOf('if (!productionHost || !hasExactStaticIslandHosts(productionHost))')
      < island.indexOf('productionHost.attachShadow({ mode: "open" })'),
    'the static island topology must fail closed before the React shell attaches',
  );
  assert.ok(
    island.indexOf('flushSync(() => productionRoot.render(<ProductionWorkspacePage />))')
      < island.indexOf('productionHost.setAttribute("data-mesh-react-shell-active", "true")'),
    'the visible React shell must commit before it advertises readiness',
  );
  assert.match(productionLayout, /id="mesh-react-main"/);
  assert.match(productionLayout, /id="mesh-react-page-content"/);
  assert.match(productionPage, /productionRoutes\.map/);
  assert.match(productionPage, /const active = activePage === route\.id/);
  assert.match(productionPage, /active=\{active\}/);
  assert.match(productionPage, /focusProductionRoute\(document, request\)/);
  assert.match(productionFocus, /productionRoute\(request\.page\)/);
  assert.match(productionLayout, /onClick=\{\(event\) => activateWorkspaceSkipLink\(event, mainRef\.current\)\}/);
  assert.match(productionPage, /<ProductionNavigation/);
  assert.match(
    productionPage,
    /useEffect\(\(\) => \{\s*activateProductionNoticeReplay\(\);\s*\}, \[\]\);/,
    'retained notice replay must begin only after the empty live regions commit',
  );
  assert.ok(
    productionPage.lastIndexOf('<slot name="confirmation-dialog" />')
      > productionPage.lastIndexOf('</ProductionWorkspaceLayout>'),
    'the confirmation must be a sibling of the visible page it isolates',
  );
  assert.match(productionPage, /onNavigate=\{\(page\) => flushSync\(\(\) => requestProductionPage\(page\)\)\}/);
  assert.match(productionRoute, /id: "current",[\s\S]*?label: "Current"/);
  assert.match(productionRoute, /id: "update",[\s\S]*?label: "Update destination"/);
  assert.match(productionNavigation, /aria-label=\{`\$\{nativeChangeCount\} folder changes`\}/);
  assert.doesNotMatch(productionPage, /<slot name="workspace-navigation"/);
  assert.match(workspaceView, /data-mesh-page-active/);
  assert.match(workspaceView, /data-mesh-slot-failure/);
  assert.match(workspaceView, /window\.location\.reload\(\)/);
  assert.match(island, /attachShadow\(\{ mode: "open" \}\)/);
  assert.match(island, /useLayoutEffect\(onCommit, \[onCommit\]\)/);
  assert.match(
    island,
    /key=\{candidate\.bundle\}/,
    'the same exact review bundle must survive background verification without remounting',
  );
  assert.match(island, /reconcileReviewWorkbenchProjection\(current, source\)/);
  assert.doesNotMatch(
    island,
    /\n\s{6}root\.render\([\s\S]*?\n\s{6}\);\n\s{6}document\.dispatchEvent\(new CustomEvent\([A-Z_]*MOUNTED_EVENT/,
    'mounted must be emitted by a committed React tree, never immediately after root.render',
  );
  assert.match(island, /workspaceOverviewEnvelope/);
  assert.match(island, /workspaceChromeEnvelope/);
  assert.match(island, /WorkspaceHeader/);
  assert.doesNotMatch(island, /WorkspaceNavigation/);
  assert.match(island, /workspaceVersionsEnvelope/);
  assert.match(island, /workspaceRestoreEnvelope/);
  assert.match(island, /WorkspaceRestore/);
  assert.match(island, /generation: exactGeneration/);
  assert.match(island, /styles\.css\?inline/);
  assert.doesNotMatch(island, /innerHTML/);
});

test('the production page focus fallback reaches the visible main landmark through its shadow host', async () => {
  const output = await build({
    entryPoints: [join(root, 'src/lib/production-focus.ts')],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const module = await import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString('base64')}`);
  const lookups = [];
  const focusOptions = [];
  const main = { focus: (options) => focusOptions.push(options) };
  const changesFocusOptions = [];
  const changesTarget = { focus: (options) => changesFocusOptions.push(options) };
  let changesHidden = true;
  const changesHost = {
    classList: { contains: (value) => value === 'hidden' && changesHidden },
    shadowRoot: {
      querySelector(selector) {
        return selector === '[data-mesh-work-action="scan-files"]' ? changesTarget : null;
      },
    },
  };
  const host = {
    shadowRoot: {
      getElementById(id) {
        lookups.push(id);
        return id === 'mesh-react-main' ? main : null;
      },
    },
  };
  const documentRoot = {
    getElementById(id) {
      lookups.push(id);
      return id === 'mesh-app-next' ? host : id === 'workspace-changes-next' ? changesHost : null;
    },
  };

  assert.equal(module.focusProductionMain(documentRoot), true);
  assert.deepEqual(lookups, ['mesh-app-next', 'mesh-react-main']);
  assert.deepEqual(focusOptions, [{ preventScroll: true }]);
  const changesRequest = { page: 'changes', selector: '[data-mesh-work-action="scan-files"]' };
  assert.equal(module.focusProductionRoute(documentRoot, changesRequest), true);
  assert.deepEqual(changesFocusOptions, []);
  assert.deepEqual(
    focusOptions,
    [{ preventScroll: true }, { preventScroll: true }],
    'a not-yet-committed Changes page must keep focus on the visible main landmark',
  );
  changesHidden = false;
  assert.equal(module.focusProductionRoute(documentRoot, changesRequest), true);
  assert.deepEqual(
    changesFocusOptions,
    [{ preventScroll: true }],
    'the retained exact selector did not focus after the delayed Changes page commit',
  );
  assert.equal(module.focusProductionMain({ getElementById: () => null }), false);
});

test('the production skip link explicitly focuses the main landmark inside its shadow root', async () => {
  const output = await build({
    entryPoints: [join(root, 'src/layouts/production-workspace-layout.tsx')],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const module = await import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString('base64')}`);
  let prevented = 0;
  const focusCalls = [];
  const event = { preventDefault: () => { prevented += 1; } };
  const main = { focus: (options) => focusCalls.push(options) };

  assert.equal(module.activateWorkspaceSkipLink(event, main), true);
  assert.equal(prevented, 1);
  assert.deepEqual(focusCalls, [{ preventScroll: true }]);
  assert.equal(module.activateWorkspaceSkipLink(event, null), false);
  assert.equal(prevented, 2, 'the closed skip-link handler allowed a hash navigation into light DOM');
  assert.deepEqual(focusCalls, [{ preventScroll: true }]);
});

test('the production page store retains coordinator state before the React shell commits', async () => {
  const output = await build({
    entryPoints: [join(root, 'src/models/production-shell-store.ts')],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const previousDocument = globalThis.document;
  const previousCustomEvent = globalThis.CustomEvent;
  class TestCustomEvent extends Event {
    constructor(type, init = {}) {
      super(type);
      this.detail = init.detail;
    }
  }
  globalThis.document = new EventTarget();
  globalThis.CustomEvent = TestCustomEvent;
  try {
    let buildIdentityRequests = 0;
    let noticeSnapshotRequests = 0;
    document.addEventListener('mesh:build-identity-available', () => {
      buildIdentityRequests += 1;
    });
    document.addEventListener('mesh:notice-snapshot-request', () => {
      noticeSnapshotRequests += 1;
      document.dispatchEvent(new CustomEvent('mesh:notice-projection', {
        detail: {
          schema: 'mesh.notice/v1', generation: 7,
          message: 'Retained before React loaded.', error: false, proof: null,
        },
      }));
    });
    const source = output.outputFiles[0].text;
    const store = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);
    assert.equal(buildIdentityRequests, 1, 'a late shell did not request the retained build identity');
    assert.equal(noticeSnapshotRequests, 0, 'notice replay ran before a stable live region could mount');
    assert.equal(store.productionShellSnapshot().notice, null);
    store.activateProductionNoticeReplay();
    assert.equal(noticeSnapshotRequests, 1, 'a late shell did not request the retained notice');
    assert.deepEqual(store.productionShellSnapshot().notice, {
      generation: 7, message: 'Retained before React loaded.', error: false, proof: null,
    });
    const unavailableSnapshot = store.productionShellSnapshot();
    assert.equal(store.requestProductionPage('invented'), false, 'a forged route passed the exported runtime guard');
    assert.equal(store.requestProductionPage('review'), false, 'a workspace route was accepted before a workspace existed');
    document.dispatchEvent(new CustomEvent('mesh:workspace-page-request', {
      detail: { page: 'review', selector: null },
    }));
    assert.strictEqual(
      store.productionShellSnapshot(),
      unavailableSnapshot,
      'an unavailable coordinator request changed route or focus identity',
    );
    document.dispatchEvent(new CustomEvent('mesh:workspace-chrome-projection', {
      detail: { workspaceIdentity: 1, chrome: { workspaceReady: true, nativeChangeCount: 3 } },
    }));
    let identicalChromePublishes = 0;
    const stopObservingChrome = store.subscribeProductionShell(() => {
      identicalChromePublishes += 1;
    });
    const retainedChromeSnapshot = store.productionShellSnapshot();
    document.dispatchEvent(new CustomEvent('mesh:workspace-chrome-projection', {
      detail: { workspaceIdentity: 1, chrome: { workspaceReady: true, nativeChangeCount: 3 } },
    }));
    assert.strictEqual(
      store.productionShellSnapshot(),
      retainedChromeSnapshot,
      'an identical chrome projection replaced the production shell snapshot',
    );
    assert.equal(identicalChromePublishes, 0, 'an identical chrome projection rerendered the production shell');
    stopObservingChrome();
    assert.equal(store.requestProductionPage('review', 7), false, 'a malformed focus selector changed the route');
    const beforeMalformedSelector = store.productionShellSnapshot();
    assert.doesNotThrow(() => {
      document.dispatchEvent(new CustomEvent('mesh:workspace-page-request', {
        detail: { page: 'versions', selector: '[' },
      }));
    });
    assert.strictEqual(
      store.productionShellSnapshot(),
      beforeMalformedSelector,
      'a malformed event selector changed page or focus state instead of failing closed',
    );
    document.dispatchEvent(new CustomEvent('mesh:notice-projection', {
      detail: {
        schema: 'mesh.notice/v1', generation: 8,
        message: 'Workspace verified.', error: false, proof: null,
      },
    }));
    document.dispatchEvent(new CustomEvent('mesh:workspace-page-request', {
      detail: { page: 'review', selector: null },
    }));
    assert.deepEqual(store.productionShellSnapshot(), {
      activePage: 'review',
      workspaceReady: true,
      nativeChangeCount: 3,
      notice: { generation: 8, message: 'Workspace verified.', error: false, proof: null },
      buildIdentity: {
        label: 'Build identity unavailable',
        title: 'Mesh refused a malformed or incomplete build identity.',
      },
      focusRequest: { page: 'review', selector: null, sequence: 1 },
      surfaceFailures: {},
    });
    document.dispatchEvent(new CustomEvent('mesh:notice-projection', {
      detail: {
        schema: 'mesh.notice/v1', generation: 7,
        message: 'Stale failure.', error: true, proof: null,
      },
    }));
    document.dispatchEvent(new CustomEvent('mesh:notice-projection', {
      detail: {
        schema: 'mesh.notice/v1', generation: 9,
        message: 'Forged proof.', error: false, proof: 'invented',
      },
    }));
    document.dispatchEvent(new CustomEvent('mesh:notice-projection', {
      detail: {
        schema: 'mesh.notice/v1', generation: 9,
        message: 'Contradictory proof.', error: true, proof: 'agent-handoff-rescanned',
      },
    }));
    assert.deepEqual(store.productionShellSnapshot().notice, {
      generation: 8, message: 'Workspace verified.', error: false, proof: null,
    });
    document.dispatchEvent(new CustomEvent('mesh:workspace-page-request', {
      detail: { page: 'invented', selector: null },
    }));
    assert.equal(store.productionShellSnapshot().activePage, 'review');
    document.dispatchEvent(new CustomEvent('mesh:build-identity-projection', {
      detail: { label: 'Build 0123456789ab', title: 'Exact source revision 0123456789abcdef0123456789abcdef01234567' },
    }));
    assert.deepEqual(store.productionShellSnapshot().buildIdentity, {
      label: 'Build 0123456789ab',
      title: 'Exact source revision 0123456789abcdef0123456789abcdef01234567',
    });
    document.dispatchEvent(new CustomEvent('mesh:build-identity-projection', {
      detail: { label: 'forged', title: 'forged', extra: true },
    }));
    assert.equal(store.productionShellSnapshot().buildIdentity.label, 'Build 0123456789ab');
    store.requestProductionPage('workspaces');
    document.dispatchEvent(new CustomEvent('mesh:workspace-chrome-projection', {
      detail: { workspaceIdentity: 1, chrome: { workspaceReady: true, nativeChangeCount: 4 } },
    }));
    assert.equal(store.productionShellSnapshot().activePage, 'workspaces', 'a refresh stole an explicitly selected page');
    store.requestProductionPage('import', '[data-mesh-import-choose]');
    document.dispatchEvent(new CustomEvent('mesh:workspace-chrome-projection', {
      detail: { workspaceIdentity: 1, chrome: { workspaceReady: true, nativeChangeCount: 0 } },
    }));
    assert.equal(
      store.productionShellSnapshot().activePage,
      'import',
      'a same-workspace refresh stole the explicitly selected Import page',
    );
    assert.deepEqual(
      store.productionShellSnapshot().focusRequest,
      { page: 'import', selector: '[data-mesh-import-choose]', sequence: 3 },
      'a same-workspace refresh replaced the existing focus request',
    );
    const acceptedImportRequest = store.productionShellSnapshot();
    document.dispatchEvent(new CustomEvent('mesh:workspace-page-request', {
      detail: { page: 'import', selector: '[data-mesh-import-preview]' },
    }));
    assert.strictEqual(
      store.productionShellSnapshot(),
      acceptedImportRequest,
      'an unapproved Import selector crossed the production route boundary',
    );
    store.requestProductionPage('current');
    document.dispatchEvent(new CustomEvent('mesh:workspace-chrome-projection', {
      detail: { workspaceIdentity: 2, chrome: { workspaceReady: false, nativeChangeCount: 0 } },
    }));
    assert.equal(
      store.productionShellSnapshot().activePage,
      'workspaces',
      'closing the workspace left an unavailable explicit page active',
    );
    assert.deepEqual(
      store.productionShellSnapshot().focusRequest,
      { page: 'workspaces', selector: null, sequence: 5 },
      'closing the workspace did not request focus for the exact fallback page',
    );
    const closedSnapshot = store.productionShellSnapshot();
    assert.equal(store.requestProductionPage('versions'), false);
    document.dispatchEvent(new CustomEvent('mesh:workspace-page-request', {
      detail: { page: 'versions', selector: '[role="radio"]' },
    }));
    assert.strictEqual(
      store.productionShellSnapshot(),
      closedSnapshot,
      'a delayed workspace route request survived identity close',
    );
    document.dispatchEvent(new CustomEvent('mesh:workspace-chrome-projection', {
      detail: { workspaceIdentity: 3, chrome: { workspaceReady: true, nativeChangeCount: 0 } },
    }));
    assert.equal(store.productionShellSnapshot().activePage, 'current', 'a new workspace did not open on Current');
    assert.deepEqual(
      store.productionShellSnapshot().focusRequest,
      { page: 'current', selector: null, sequence: 6 },
      'switching workspace identity did not request focus for Current',
    );
    for (const selector of [
      '[data-mesh-proof="destination-draft"]',
      '[data-mesh-proof="destination-choose"]',
      '[data-mesh-proof="destination-preview-all"]',
    ]) {
      document.dispatchEvent(new CustomEvent('mesh:workspace-page-request', {
        detail: { page: 'update', selector },
      }));
      assert.equal(store.productionShellSnapshot().activePage, 'update');
      assert.equal(
        store.productionShellSnapshot().focusRequest?.selector,
        selector,
        'the production store rejected an exact coordinator-owned Update destination control',
      );
    }
    const acceptedUpdateRequest = store.productionShellSnapshot();
    document.dispatchEvent(new CustomEvent('mesh:workspace-page-request', {
      detail: { page: 'update', selector: '[data-mesh-proof="destination-confirm-all"]' },
    }));
    assert.strictEqual(
      store.productionShellSnapshot(),
      acceptedUpdateRequest,
      'an unapproved Update destination selector crossed the production route boundary',
    );
    for (const selector of [
      'textarea',
      '[data-mesh-work-action="scan-files"]',
      '[data-mesh-work-action="save-all-private"]',
      '[data-mesh-work-field="missingSource"]',
      '[data-mesh-native-queue]',
    ]) {
      document.dispatchEvent(new CustomEvent('mesh:workspace-page-request', {
        detail: { page: 'changes', selector },
      }));
      assert.equal(store.productionShellSnapshot().activePage, 'changes');
      assert.equal(
        store.productionShellSnapshot().focusRequest?.selector,
        selector,
        'the production store rejected an exact coordinator-owned Changes control',
      );
    }
    const acceptedChangesRequest = store.productionShellSnapshot();
    document.dispatchEvent(new CustomEvent('mesh:workspace-page-request', {
      detail: { page: 'changes', selector: '[data-mesh-work-action="delete-entry"]' },
    }));
    assert.strictEqual(
      store.productionShellSnapshot(),
      acceptedChangesRequest,
      'an unapproved Changes selector crossed the production route boundary',
    );
    document.dispatchEvent(new CustomEvent('mesh:workspace-current-projection', {
      detail: { generation: 8 },
    }));
    document.dispatchEvent(new CustomEvent('mesh:workspace-current-rejected', {
      detail: { generation: 7 },
    }));
    assert.deepEqual(store.productionShellSnapshot().surfaceFailures, {}, 'a stale rejection changed the shell');
    document.dispatchEvent(new CustomEvent('mesh:workspace-current-rejected', {
      detail: { generation: 8 },
    }));
    assert.deepEqual(store.productionShellSnapshot().surfaceFailures, { 'workspace-current': true });
    document.dispatchEvent(new CustomEvent('mesh:workspace-current-mounted', {
      detail: { generation: 8 },
    }));
    assert.deepEqual(store.productionShellSnapshot().surfaceFailures, {});
    document.dispatchEvent(new CustomEvent('mesh:workspace-overview-projection', {
      detail: { generation: 21 },
    }));
    document.dispatchEvent(new CustomEvent('mesh:workspace-overview-rejected', {
      detail: { generation: 21 },
    }));
    assert.deepEqual(
      store.productionShellSnapshot().surfaceFailures,
      { 'workspace-overview': true },
      'an exact Overview rejection did not expose the outer React failure fallback',
    );
  } finally {
    globalThis.document = previousDocument;
    globalThis.CustomEvent = previousCustomEvent;
  }
});

test('the production notice primes stable empty live regions before retained replay is activated', async () => {
  const output = await build({
    stdin: {
      contents: `
        import React from "react";
        import { renderToStaticMarkup } from "react-dom/server";
        import { ProductionWorkspacePage } from "./src/pages/production-workspace-page";
        import { activateProductionNoticeReplay } from "./src/models/production-shell-store";
        module.exports = {
          activateProductionNoticeReplay,
          renderPage: () => renderToStaticMarkup(<ProductionWorkspacePage />),
        };
      `,
      resolveDir: root,
      loader: 'tsx',
    },
    bundle: true,
    format: 'cjs',
    platform: 'node',
    packages: 'external',
    write: false,
  });
  const previousDocument = globalThis.document;
  const previousCustomEvent = globalThis.CustomEvent;
  class TestCustomEvent extends Event {
    constructor(type, init = {}) {
      super(type);
      this.detail = init.detail;
    }
  }
  const documentRoot = new EventTarget();
  let snapshotRequests = 0;
  documentRoot.addEventListener('mesh:notice-snapshot-request', () => {
    snapshotRequests += 1;
    documentRoot.dispatchEvent(new TestCustomEvent('mesh:notice-projection', {
      detail: {
        schema: 'mesh.notice/v1', generation: 1,
        message: 'Retained before the shell loaded.', error: false, proof: null,
      },
    }));
  });
  globalThis.document = documentRoot;
  globalThis.CustomEvent = TestCustomEvent;
  try {
    const module = { exports: {} };
    Function('require', 'module', 'exports', output.outputFiles[0].text)(
      createRequire(import.meta.url),
      module,
      module.exports,
    );
    const primed = module.exports.renderPage();
    assert.equal(snapshotRequests, 0);
    assert.match(primed, /data-mesh-proof="production-notice"/);
    assert.match(primed, /role="status" aria-live="polite" aria-atomic="true"><\/div>/);
    assert.match(primed, /role="alert" aria-live="assertive" aria-atomic="true"><\/div>/);
    assert.doesNotMatch(primed, /Retained before the shell loaded\./);

    module.exports.activateProductionNoticeReplay();
    const populated = module.exports.renderPage();
    assert.equal(snapshotRequests, 1);
    assert.match(populated, /whitespace-pre-line/);
    assert.match(populated, /role="status" aria-live="polite" aria-atomic="true">Retained before the shell loaded\.<\/div>/);
    assert.match(populated, /role="alert" aria-live="assertive" aria-atomic="true"><\/div>/);

    documentRoot.dispatchEvent(new TestCustomEvent('mesh:notice-projection', {
      detail: {
        schema: 'mesh.notice/v1', generation: 2,
        message: 'Saved workspace unavailable\nTry again or forget this shortcut.', error: true, proof: null,
      },
    }));
    const failed = module.exports.renderPage();
    assert.match(failed, /role="status" aria-live="polite" aria-atomic="true"><\/div>/);
    assert.match(failed, /role="alert" aria-live="assertive" aria-atomic="true">Saved workspace unavailable\nTry again or forget this shortcut\.<\/div>/);
    module.exports.activateProductionNoticeReplay();
    assert.equal(snapshotRequests, 1, 'remount activation requested a duplicate retained announcement');
  } finally {
    globalThis.document = previousDocument;
    globalThis.CustomEvent = previousCustomEvent;
  }
});

test('bare React islands preserve same-workspace visual continuity and reject a generation that never commits', async () => {
  const legacyController = readFileSync(join(root, '..', 'ui', 'app.js'), 'utf8');
  const island = readFileSync(join(root, 'src', 'island.tsx'), 'utf8');
  const continuitySurfaces = [
    ['reviewWorkbenchNextPending', 'reviewWorkbenchNextMounted', 'installReviewWorkbenchNextVisibility', 'mesh:review-workbench-projection'],
    ['workspaceOverviewNextPending', 'workspaceOverviewNextMounted', 'installWorkspaceOverviewNextVisibility', 'mesh:workspace-overview-projection'],
    ['workspaceChromeNextPending', 'workspaceChromeNextMounted', 'installWorkspaceChromeNextVisibility', 'mesh:workspace-chrome-projection'],
    ['workspaceEntryNextPending', 'workspaceEntryNextMounted', 'installWorkspaceEntryNextVisibility', 'mesh:workspace-entry-projection'],
    ['workspaceCurrentNextPending', 'workspaceCurrentNextMounted', 'installWorkspaceCurrentNextVisibility', 'mesh:workspace-current-projection'],
    ['workspaceWorkNextPending', 'workspaceWorkNextMounted', 'installWorkspaceWorkNextVisibility', 'mesh:workspace-files-changes-projection'],
    ['workspaceDestinationNextPending', 'workspaceDestinationNextMounted', 'installWorkspaceDestinationNextVisibility', 'mesh:workspace-destination-projection'],
    ['workspaceVersionsNextPending', 'workspaceVersionsNextMounted', 'installWorkspaceVersionsNextVisibility', 'mesh:workspace-versions-projection'],
    ['workspaceRestoreNextPending', 'workspaceRestoreNextMounted', 'installWorkspaceRestoreNextVisibility', 'mesh:workspace-restore-projection'],
  ];
  for (const [pending, mounted, install, event] of continuitySurfaces) {
    const pendingAt = legacyController.indexOf(`${pending} = Object.freeze(`);
    const projectionAt = legacyController.indexOf(`'${event}'`, pendingAt);
    const transition = legacyController.slice(pendingAt, projectionAt);
    assert.ok(pendingAt >= 0 && projectionAt > pendingAt, `${event} must publish one coordinator projection`);
    assert.match(transition, /continuityKey/);
    assert.match(
      transition,
      new RegExp(`${mounted}\\?\\.continuityKey !== continuityKey[\\s\\S]*${mounted} = null;[\\s\\S]*${install}\\(\\);`),
      `${event} must retain only a mount bound to the same exact physical workspace`,
    );
    assert.match(
      legacyController,
      new RegExp(`projectionHasVisibleContinuity\\(\\s*${pending},\\s*${mounted},?\\s*\\)`),
      `${event} must use the bounded continuity predicate for visibility`,
    );
  }
  assert.match(legacyController, /workspace\.root/);
  assert.match(legacyController, /workspace\.installation/);
  assert.match(
    legacyController,
    /function installWorkspaceOverviewNextVisibility\(\)[\s\S]*host\.classList\.toggle\('hidden', !exactMount\)/,
    'a withheld first Overview commit must leave its assigned host hidden so the outer slot stays in Preparing',
  );
  for (const [pending, mounted, install, event] of [
    ['importWorkbenchNextPending', 'importWorkbenchNextMounted', 'installImportWorkbenchNextVisibility', 'mesh:import-workbench-projection'],
  ]) {
    const pendingAt = legacyController.indexOf(`${pending} = Object.freeze(`);
    const projectionAt = legacyController.indexOf(`'${event}'`, pendingAt);
    const transition = legacyController.slice(pendingAt, projectionAt);
    assert.ok(pendingAt >= 0 && projectionAt > pendingAt, `${event} must publish one coordinator projection`);
    assert.match(
      transition,
      new RegExp(`interactionGeneration !== ${mounted}\\?\\.generation[\\s\\S]*${mounted} = null;[\\s\\S]*${install}\\(\\);`),
      `${event} may preserve a mounted surface only for its exact coordinator-accepted interaction generation`,
    );
  }
  assert.match(island, /createIslandLiveness/);
  assert.match(island, /class IslandRenderBoundary/);
  assert.match(island, /this\.props\.liveness\.fail\(this\.props\.generation, error\)/);
  assert.doesNotMatch(
    island,
    /<IslandRenderBoundary key=\{generation\}/,
    'a healthy same-identity projection must not remount the component subtree just to refresh liveness',
  );
  assert.equal(
    (island.match(/guardedProjection\(/g) || []).length,
    11,
    'all ten remaining guarded roots, including the paired Files and Changes surfaces, require an exact-generation boundary',
  );

  const output = await build({
    entryPoints: [join(root, 'src/models/island-liveness.ts')],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const module = await import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString('base64')}`);
  const timers = new Map();
  const cleared = new Set();
  const rejected = [];
  let timerId = 0;
  const liveness = module.createIslandLiveness({
    reject: (generation, reason) => rejected.push([generation, reason]),
    schedule: (callback) => {
      timerId += 1;
      timers.set(timerId, callback);
      return timerId;
    },
    cancel: (id) => cleared.add(id),
  });

  liveness.begin(1);
  assert.equal(liveness.commit(1), true);
  liveness.fail(1, new Error('mounted surface failed'));
  assert.deepEqual(rejected, [[1, 'mounted surface failed']]);
  liveness.begin(2);
  const secondTimer = timerId;
  timers.get(secondTimer)();
  assert.deepEqual(rejected.at(-1), [2, 'The React surface did not commit in time.']);
  assert.equal(liveness.commit(2), false, 'a timed-out late commit cannot regain authority');

  liveness.begin(3);
  const supersededTimer = timerId;
  liveness.begin(4);
  assert.equal(cleared.has(supersededTimer), true, 'supersession must cancel only the prior timer');
  timers.get(supersededTimer)();
  assert.equal(rejected.length, 2, 'a stale timer rejected the current generation');
  assert.equal(liveness.commit(3), false, 'a stale commit regained authority');
  assert.equal(liveness.commit(4), true);

  liveness.begin(5);
  liveness.fail(4, new Error('stale render failed'));
  assert.equal(rejected.length, 2, 'a stale render error rejected the current generation');
  liveness.fail(5, new Error('render failed'));
  assert.deepEqual(rejected.at(-1), [5, 'render failed']);
  assert.equal(liveness.commit(5), false, 'an uncaught render failure admitted a late commit');

  liveness.begin(6, ['files', 'changes']);
  const pairedTimer = timerId;
  assert.equal(liveness.commit(6, 'files'), false);
  assert.equal(liveness.commit(6, 'files'), false, 'a duplicate root commit satisfied a paired surface');
  timers.get(pairedTimer)();
  assert.deepEqual(rejected.at(-1), [6, 'The React surface did not commit in time.']);
  assert.equal(liveness.commit(6, 'changes'), false, 'a partial paired render regained authority after timeout');
});

test('the production workspace chrome accepts only bounded presentation', async () => {
  const output = await build({
    entryPoints: [join(root, 'src/models/workspace-chrome.ts')],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const module = await import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString('base64')}`);
  const chrome = {
    serviceState: 'ready',
    serviceLabel: 'Local service ready',
    workspaceReady: true,
    nativeChangeCount: 3,
  };
  const accepted = module.workspaceChromeEnvelope({ generation: 4, workspaceIdentity: 7, chrome }, 3);
  assert.equal(accepted.workspaceIdentity, 7);
  assert.equal(accepted.model.workspaceReady, true);
  assert.equal(accepted.model.nativeChangeCount, 3);
  assert.equal(Object.isFrozen(accepted.model), true);
  assert.throws(
    () => module.workspaceChromeEnvelope({ generation: 4, workspaceIdentity: 7, chrome }, 4),
    /stale or invalid/,
  );
  assert.throws(
    () => module.workspaceChromeEnvelope({ generation: 5, workspaceIdentity: 7, chrome: { ...chrome, serviceLabel: 'Ready\u202eexe' } }, 4),
    /unsafe or unbounded/,
  );
});

test('the production workspace entry page accepts only fresh bounded state and closed coordinator intents', async () => {
  const output = await build({
    entryPoints: [join(root, 'src/models/workspace-entry.ts')],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const module = await import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString('base64')}`);
  const entry = {
    mode: 'empty',
    eyebrow: 'LOCAL WORKSPACE',
    title: 'Your folder, with a private history.',
    description: 'Choose one ordinary folder. The original stays untouched.',
    disclosureLabel: 'Return to a recent Mesh workspace',
    disclosureOpen: true,
    chooseLabel: 'Choose a folder',
    canChoose: true,
    canChooseManaged: true,
    retryLabel: 'Refresh',
    canRetry: true,
    openPath: '',
    canEditPath: true,
    canOpenPath: false,
    recents: [{ path: '/private/one.mesh', label: 'Payroll · Managed workspace', state: 'unavailable' }],
    selectedRecentPath: '/private/one.mesh',
    canSelectRecent: true,
    recentHint: 'Forgetting removes only this navigation shortcut.',
    recentOpenLabel: 'Switch workspace',
    canOpenRecent: true,
    canForgetRecent: true,
    forgetRecentTitle: '',
  };
  const accepted = module.workspaceEntryEnvelope({ generation: 8, entry }, 7);
  assert.equal(accepted.model.recents[0].label, 'Payroll · Managed workspace');
  assert.equal(accepted.model.recents[0].state, 'unavailable');
  assert.equal(Object.isFrozen(accepted.model.recents), true);
  assert.deepEqual(module.workspaceEntryIntent({ type: 'choose-folder' }), { type: 'choose-folder' });
  assert.deepEqual(module.workspaceEntryIntent({ type: 'retry' }), { type: 'retry' });
  assert.deepEqual(module.workspaceEntryIntent({ type: 'set-disclosure', open: true }), {
    type: 'set-disclosure',
    open: true,
  });
  assert.deepEqual(module.workspaceEntryIntent({ type: 'update-managed-path', path: '/private/two.mesh' }), {
    type: 'update-managed-path',
    path: '/private/two.mesh',
  });
  assert.deepEqual(module.workspaceEntryIntent({ type: 'open-managed-path', path: '/private/two.mesh' }), {
    type: 'open-managed-path',
    path: '/private/two.mesh',
  });
  assert.throws(
    () => module.workspaceEntryEnvelope({ generation: 8, entry }, 8),
    /stale or invalid/,
  );
  assert.throws(
    () => module.workspaceEntryEnvelope({ generation: 9, entry: { ...entry, selectedRecentPath: '/forged' } }, 8),
    /not in the projected list/,
  );
  assert.throws(
    () => module.workspaceEntryEnvelope({
      generation: 9,
      entry: { ...entry, recents: [{ ...entry.recents[0], state: 'missing' }] },
    }, 8),
    /recent workspace state was invalid/,
  );
  assert.throws(
    () => module.workspaceEntryIntent({ type: 'open-recent', path: '/private/one.mesh', invoke: true }),
    /unrecognized or missing fields/,
  );
  assert.throws(
    () => module.workspaceEntryIntent({ type: 'update-managed-path', path: `safe\u202eevil` }),
    /unsafe or unbounded/,
  );
  assert.throws(
    () => module.workspaceEntryIntent({ type: 'open-managed-path' }),
    /unrecognized or missing fields/,
  );
  assert.throws(
    () => module.workspaceEntryIntent({ type: 'set-disclosure', open: 'true' }),
    /not boolean/,
  );

  const pickerBuild = await build({
    stdin: {
      contents: `
        import React from "react";
        import { renderToStaticMarkup } from "react-dom/server";
        import { RecentWorkspacePicker } from "./src/molecules/recent-workspace-picker.tsx";
        module.exports.render = (props) => renderToStaticMarkup(
          React.createElement(RecentWorkspacePicker, { ...props, onIntent: () => {} }),
        );
      `,
      resolveDir: root,
      loader: 'js',
    },
    bundle: true,
    format: 'cjs',
    platform: 'node',
    packages: 'external',
    write: false,
  });
  const picker = { exports: {} };
  Function('require', 'module', 'exports', pickerBuild.outputFiles[0].text)(
    createRequire(import.meta.url),
    picker,
    picker.exports,
  );
  const markup = picker.exports.render({
    recents: accepted.model.recents,
    selectedPath: '/private/one.mesh',
    canSelect: true,
    hint: 'Restore its saved folder and try again, or choose Forget from list.',
    openLabel: 'Try again',
    canOpen: true,
    canForget: true,
    forgetTitle: '',
  });
  assert.match(markup, /Unavailable · Payroll · Managed workspace/);
  assert.match(markup, />Try again</);
  assert.match(markup, /Restore its saved folder and try again/);
});

test('the production Restore panel accepts only fresh bounded choices, readable previews, and closed intents', async () => {
  const output = await build({
    entryPoints: [join(root, 'src/models/workspace-restore.ts')],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const module = await import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString('base64')}`);
  const restore = {
    files: [{ id: 'object-1', path: 'finance/forecast.xlsx', label: 'finance/forecast.xlsx · 2 saved versions', format: 'Excel' }],
    selectedFileId: 'object-1',
    versions: [{ id: 'version-1', label: 'Saved version · version-1' }],
    selectedVersionId: 'version-1',
    canSelectFile: true,
    canSelectVersion: true,
    canPreview: true,
    canApply: true,
    canUndo: false,
    hint: 'Choose an earlier version to preview its exact retained bytes.',
    preview: {
      filePath: 'finance/forecast.xlsx',
      format: 'Excel',
      currentVersion: 'current-version',
      targetVersion: 'version-1',
      change: 'Replace this file in the managed working folder with exact retained bytes.',
      historyNote: 'Private history stays unchanged until you choose Save privately.',
      undoNote: 'Available immediately after this restore.',
    },
    undoLabel: 'Undo last restore',
  };
  const accepted = module.workspaceRestoreEnvelope({ generation: 5, restore }, 4);
  assert.equal(accepted.model.preview.format, 'Excel');
  assert.equal(Object.isFrozen(accepted.model.files), true);
  assert.deepEqual(module.workspaceRestoreIntent({ type: 'preview' }), { type: 'preview' });
  assert.deepEqual(module.workspaceRestoreIntent({ type: 'select-version', id: 'version-1' }), { type: 'select-version', id: 'version-1' });
  assert.throws(
    () => module.workspaceRestoreEnvelope({ generation: 5, restore }, 5),
    /stale or invalid/,
  );
  assert.throws(
    () => module.workspaceRestoreEnvelope({ generation: 6, restore: { ...restore, selectedVersionId: 'forged' } }, 5),
    /not in the projected choices/,
  );
  assert.throws(
    () => module.workspaceRestoreEnvelope({ generation: 6, restore: { ...restore, files: [{ ...restore.files[0], format: 'Spreadsheet' }] } }, 5),
    /format was invalid/,
  );
  assert.throws(
    () => module.workspaceRestoreIntent({ type: 'apply', force: true }),
    /unrecognized or missing fields/,
  );
});

test('the production workspace version navigator accepts only verified bounded points and intents', async () => {
  const organism = readFileSync(join(root, 'src', 'organisms', 'workspace-version-navigator.tsx'), 'utf8');
  assert.match(organism, /role="radiogroup"/);
  assert.match(organism, /role="radio"/);
  assert.match(organism, /data-mesh-version-operation=\{item\.operation\}/);
  assert.match(organism, /className="sr-only" role="status" aria-live="polite" aria-atomic="true"/);
  assert.doesNotMatch(organism, /<section[^>]*aria-live=/s);
  assert.doesNotMatch(organism.slice(organism.indexOf('function VersionList')), /aria-live=/);
  assert.match(organism, /aria-busy=\{model\.previewState === "loading"\}/);
  assert.match(organism, /id="workspace-version-custom-location"/);
  assert.match(organism, /onIntent\(\{ type: "set-custom-location", path \}\)/);
  assert.match(
    organism,
    /rovingSelectionIndex\(model\.versions\.length, index, event\.key, "all-wrap"\)/,
    'saved-point radio arrows must wrap instead of scrolling the page at a list boundary',
  );
  const output = await build({
    entryPoints: [join(root, 'src/models/workspace-versions.ts')],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const module = await import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString('base64')}`);
  const operation = '91'.repeat(32);
  const ready = {
    historyMode: 'linear',
    versions: [{ operation, ordinal: 2, relation: 'current', label: 'Current saved workspace · point 2' }],
    selectedOperation: operation,
    previewState: 'ready',
    previewTitle: 'Current saved workspace · point 2',
    previewSummary: '2 files · 1 folder · 4.2 KB. Exact retained content verified.',
    changeBasis: 'previous-point',
    basisOrdinal: 1,
    changes: ['Changed · forecast.xlsx'],
    entries: ['forecast.xlsx · 4.2 KB bytes'],
    canSelect: true,
    canOpen: true,
    canStartCodex: true,
    customLocation: '',
    canUseCustomLocation: true,
    openLabel: 'Open in working folder',
    codexLabel: 'Start Codex on this point',
  };
  const accepted = module.workspaceVersionsEnvelope({ generation: 4, versions: ready }, 3);
  assert.equal(accepted.model.selectedOperation, operation);
  assert.equal(accepted.model.canSelect, true);
  assert.equal(accepted.model.changeBasis, 'previous-point');
  assert.equal(accepted.model.basisOrdinal, 1);
  assert.equal(Object.isFrozen(accepted.model.versions), true);
  assert.deepEqual(module.workspaceVersionsIntent({ type: 'select-version', operation }), { type: 'select-version', operation });
  assert.deepEqual(module.workspaceVersionsIntent({ type: 'open-version', operation }), { type: 'open-version', operation });
  assert.deepEqual(
    module.workspaceVersionsIntent({ type: 'set-custom-location', path: '/managed/version copy ' }),
    { type: 'set-custom-location', path: '/managed/version copy ' },
  );
  assert.throws(
    () => module.workspaceVersionsIntent({ type: 'set-custom-location', path: '/managed/unsafe\npath' }),
    /unsafe or unbounded/,
  );
  assert.throws(
    () => module.workspaceVersionsEnvelope({ generation: 4, versions: ready }, 4),
    /stale or invalid/,
  );
  assert.throws(
    () => module.workspaceVersionsEnvelope({
      generation: 5,
      versions: { ...ready, previewState: 'loading', canOpen: true, changes: [] },
    }, 4),
    /authority was offered without an exact verified preview/,
  );
  assert.throws(
    () => module.workspaceVersionsEnvelope({
      generation: 5,
      versions: { ...ready, selectedOperation: '92'.repeat(32) },
    }, 4),
    /not in the bounded list/,
  );
  const unavailable = module.workspaceVersionsEnvelope({
    generation: 5,
    versions: {
      ...ready,
      selectedOperation: null,
      previewState: 'choose',
      previewTitle: 'Choose a saved workspace point',
      previewSummary: 'Selection is paused while Mesh verifies the workspace.',
      changeBasis: null,
      basisOrdinal: null,
      changes: [],
      entries: [],
      canSelect: false,
      canOpen: false,
      canStartCodex: false,
    },
  }, 4);
  assert.equal(unavailable.model.canSelect, false);
  assert.match(organism, /disabled=\{!model\.canSelect\}/);
  const navigatorBuild = await build({
    stdin: {
      contents: `
        import React from "react";
        import { renderToStaticMarkup } from "react-dom/server";
        import { WorkspaceVersionNavigator } from "./src/organisms/workspace-version-navigator.tsx";
        module.exports.render = (model) => renderToStaticMarkup(
          React.createElement(WorkspaceVersionNavigator, { model, generation: 4, onIntent: () => {} }),
        );
      `,
      resolveDir: root,
      loader: 'js',
    },
    bundle: true,
    format: 'cjs',
    platform: 'node',
    packages: 'external',
    write: false,
  });
  const navigator = { exports: {} };
  Function('require', 'module', 'exports', navigatorBuild.outputFiles[0].text)(
    createRequire(import.meta.url),
    navigator,
    navigator.exports,
  );
  const combined = navigator.exports.render({
    ...accepted.model,
    historyMode: 'concurrent',
    changeBasis: 'combined-history',
    basisOrdinal: null,
    changes: [],
  });
  assert.match(combined, /Combined saved contents/);
  assert.match(combined, /This point combines concurrent saved work\. Inspect the complete file list below\./);
  assert.doesNotMatch(combined, /No visible file or folder changes/);
  assert.throws(
    () => module.workspaceVersionsEnvelope({
      generation: 5,
      versions: { ...ready, changeBasis: 'combined-history', basisOrdinal: 1 },
    }, 4),
    /change-basis truth did not match/,
  );
  assert.throws(
    () => module.workspaceVersionsIntent({ type: 'open-version', operation, destination: '/tmp/escape' }),
    /unrecognized or missing fields/,
  );
});

test('the production workspace overview accepts only bounded closed projections and intents', async () => {
  const statusCatalog = JSON.parse(readFileSync(join(root, '..', 'src/strings/status.json'), 'utf8'));
  const organism = readFileSync(join(root, 'src/organisms/workspace-overview.tsx'), 'utf8');
  const output = await build({
    entryPoints: [join(root, 'src/models/workspace-overview.ts')],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const module = await import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString('base64')}`);
  const overview = {
    workspaceName: 'Compensation planning',
    state: 'Ready for review',
    workingFolder: '/Users/finance/Compensation planning',
    recordSummary: '12 durable records',
    privateVersion: 'Saved point 4 · 4b7c5f1…',
    sharedVersion: 'Approved point 3 · 2a6e901…',
    nativeChangeCount: 3,
    savedVersionCount: 4,
    nextActionTitle: 'Review 3 native changes',
    nextActionDescription: 'Inspect the exact folder bytes before sharing this version.',
    nextActionLabel: 'Inspect changes',
    nextActionDisabled: false,
    canOpenAnotherVersion: true,
    canOpenFolder: true,
    canFindChanges: true,
    canOpenReview: true,
    canReturnWorkspace: true,
    canCopyDiagnostics: true,
  };
  const accepted = module.workspaceOverviewEnvelope({ generation: 7, overview }, 6);
  assert.equal(accepted.model.state, 'Ready for review');
  assert.equal(accepted.model.canOpenAnotherVersion, true);
  assert.equal(accepted.model.canReturnWorkspace, true);
  assert.equal(Object.isFrozen(accepted.model), true);
  const productStates = statusCatalog.states.map((state) => state.status);
  assert.deepEqual(productStates, [
    'Working', 'Saved privately', 'Available to team', 'Ready for review', 'Needs attention', 'Approved',
  ]);
  for (const [index, state] of productStates.entries()) {
    const candidate = module.workspaceOverviewEnvelope({
      generation: 20 + index,
      overview: { ...overview, state },
    }, 19 + index);
    assert.equal(candidate.model.state, state, `the React overview rejected the product state ${state}`);
  }
  assert.deepEqual(module.workspaceOverviewIntent({ type: 'open-review' }), { type: 'open-review' });
  assert.deepEqual(module.workspaceOverviewIntent({ type: 'open-another-version' }), { type: 'open-another-version' });
  assert.deepEqual(module.workspaceOverviewIntent({ type: 'return-workspace' }), { type: 'return-workspace' });
  assert.deepEqual(module.workspaceOverviewIntent({ type: 'copy-diagnostics' }), { type: 'copy-diagnostics' });
  assert.throws(
    () => module.workspaceOverviewEnvelope({ generation: 7, overview }, 7),
    /stale or invalid/,
  );
  assert.throws(
    () => module.workspaceOverviewEnvelope({ generation: 8, overview: { ...overview, state: 'Synced' } }, 7),
    /closed product vocabulary/,
  );
  assert.throws(
    () => module.workspaceOverviewEnvelope({ generation: 8, overview: { ...overview, workspaceName: 'Finance\u202eexe' } }, 7),
    /unsafe or unbounded/,
  );
  assert.throws(
    () => module.workspaceOverviewIntent({ type: 'open-review', native: true }),
    /unrecognized or missing fields/,
  );
  assert.match(organism, /model\.canOpenAnotherVersion/);
  assert.match(organism, /onIntent\(\{ type: "open-another-version" \}\)/);
});

test('the production import workbench keeps preview and confirmation authority outside React', async () => {
  const organism = readFileSync(join(root, 'src/organisms/import-workbench.tsx'), 'utf8');
  assert.match(organism, /Choose another folder/);
  assert.match(organism, /onIntent\(\{ type: "choose-folder" \}\)/);
  const output = await build({
    entryPoints: [join(root, 'src/models/import-workbench.ts')],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const module = await import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString('base64')}`);
  const select = {
    phase: 'select',
    sourcePath: '',
    fileCount: '0',
    folderCount: '0',
    byteCount: '0',
    summary: '',
    scopeNote: 'Choose an ordinary folder. Mesh verifies every included file.',
    files: [],
    destinationPath: '',
    confirmLabel: 'Create workspace and open folder',
    busy: false,
    canChoose: true,
    canPreviewPath: true,
    canEditDestination: false,
    canChooseDestination: false,
    canConfirm: false,
  };
  const accepted = module.importWorkbenchEnvelope({ generation: 2, import: select }, 1);
  assert.equal(accepted.model.phase, 'select');
  assert.equal(accepted.model.busy, false);
  assert.equal(Object.isFrozen(accepted.model.files), true);
  assert.deepEqual(module.importWorkbenchIntent({ type: 'preview-path', path: '/Users/finance/Plan ' }), {
    type: 'preview-path',
    path: '/Users/finance/Plan ',
  });
  assert.deepEqual(module.importWorkbenchIntent({ type: 'source-draft', path: '' }), {
    type: 'source-draft',
    path: '',
  });
  assert.deepEqual(module.importWorkbenchIntent({ type: 'destination-draft', path: '/Users/finance/Mesh private ' }), {
    type: 'destination-draft',
    path: '/Users/finance/Mesh private ',
  });
  assert.deepEqual(module.importWorkbenchIntent({ type: 'choose-destination' }), {
    type: 'choose-destination',
  });
  assert.match(organism, /onIntent\(\{ type: "source-draft", path: draft \}\)/);
  assert.match(organism, /onIntent\(\{ type: "destination-draft", path: draft \}\)/);
  assert.match(organism, /onIntent\(\{ type: "choose-destination" \}\)/);
  assert.doesNotMatch(
    organism,
    /path\.trim\(\)/,
    'a valid filesystem name ending in a space must not be redirected to a different sibling',
  );
  assert.deepEqual(module.importWorkbenchIntent({ type: 'choose-folder' }), {
    type: 'choose-folder',
  });
  assert.throws(
    () => module.importWorkbenchEnvelope({ generation: 2, import: select }, 2),
    /stale or invalid/,
  );
  assert.throws(
    () => module.importWorkbenchEnvelope({ generation: 3, import: { ...select, summary: 'invented' } }, 2),
    /contradicted/,
  );
  assert.throws(
    () => module.importWorkbenchEnvelope({ generation: 3, import: { ...select, busy: 'yes' } }, 2),
    /phase contradicted its preview facts/,
  );
  assert.throws(
    () => module.importWorkbenchEnvelope({ generation: 3, import: { ...select, busy: true } }, 2),
    /phase contradicted its preview facts/,
  );
  assert.match(organism, /aria-busy=\{model\.busy\}/);
  assert.match(organism, /Large projects can take several minutes/);
  assert.throws(
    () => module.importWorkbenchIntent({ type: 'confirm-import', summary: 'forged' }),
    /unrecognized or missing fields/,
  );
  assert.throws(
    () => module.importWorkbenchIntent({ type: 'preview-path', path: '/Users/hr/Plan\u202eexe' }),
    /unsafe or unbounded/,
  );
  assert.throws(
    () => module.importWorkbenchIntent({ type: 'source-draft', path: '/Users/hr/Plan\u202eexe' }),
    /unsafe or unbounded/,
  );
});

test('a delayed import preview moves focus only while its initiating control still owns it', async () => {
  const island = readFileSync(join(root, 'src/island.tsx'), 'utf8');
  const organism = readFileSync(join(root, 'src/organisms/import-workbench.tsx'), 'utf8');
  assert.match(island, /pendingFocus = captureImportFocusRequest\(shadow, exactGeneration\)/);
  assert.match(island, /mesh:import-workbench-external-focus/);
  assert.match(organism, /data-mesh-import-choose/);
  assert.match(island, /pendingFocus = advanceImportFocusRequest\(/);
  assert.match(island, /authorizeImportReviewFocus\(pendingFocus, previousGeneration, shadow, document\)/);
  assert.match(organism, /reviewFocusAuthorization\?\.consume\(\)/);
  const output = await build({
    entryPoints: [join(root, 'src/models/import-focus.ts')],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const module = await import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString('base64')}`);
  const previewButton = { isConnected: true };
  const importSurface = { activeElement: previewButton };

  let request = module.captureImportFocusRequest(importSurface, 7);
  request = module.advanceImportFocusRequest(request, 7, 8);
  importSurface.activeElement = null;
  assert.equal(
    module.consumeImportReviewFocus(request, 8, importSurface),
    false,
    'a user who deliberately focused Current or Files while previewing must keep that focus',
  );

  importSurface.activeElement = previewButton;
  request = module.captureImportFocusRequest(importSurface, 9);
  request = module.advanceImportFocusRequest(request, 9, 10);
  const focusListeners = new Set();
  const focusEvents = {
    addEventListener(type, listener) {
      if (type === 'focusin') focusListeners.add(listener);
    },
    removeEventListener(type, listener) {
      if (type === 'focusin') focusListeners.delete(listener);
    },
    moveFocus(next) {
      importSurface.activeElement = next;
      for (const listener of [...focusListeners]) listener({ type: 'focusin' });
    },
  };
  let authorization = module.authorizeImportReviewFocus(request, 10, importSurface, focusEvents);
  assert.ok(authorization, 'the exact initiating control should authorize the pending review commit');
  focusEvents.moveFocus({ isConnected: true });
  await Promise.resolve();
  assert.equal(
    authorization.consume(),
    false,
    'focus moved after projection acceptance but before the layout effect must not be stolen',
  );

  importSurface.activeElement = previewButton;
  request = module.captureImportFocusRequest(importSurface, 11);
  authorization = module.authorizeImportReviewFocus(request, 11, importSurface, focusEvents);
  assert.ok(authorization);
  focusEvents.moveFocus(null);
  previewButton.isConnected = false;
  importSurface.activeElement = null;
  await Promise.resolve();
  assert.equal(
    authorization.consume(),
    true,
    'the expected commit-time removal of the still-owned control preserves intended heading focus',
  );

  assert.equal(
    module.authorizeImportReviewFocus(request, 9, importSurface, focusEvents),
    null,
    'an older interaction generation cannot authorize a later focus move',
  );
});

test('the source-owned confirmation accepts only fresh bounded projections and exact intents', async () => {
  const output = await build({
    entryPoints: [join(root, 'src/models/confirmation.ts')],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const module = await import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString('base64')}`);
  const confirmation = {
    title: 'Update 2 proven files?',
    description: 'Update two reviewed files in /Users/finance/Quarterly?',
    confirmLabel: 'Update 2 changed files',
    cancelLabel: 'Keep reviewing',
    tone: 'destructive',
  };
  const accepted = module.confirmationEnvelope({ generation: 3, confirmation }, 2);
  assert.equal(accepted.model.title, confirmation.title);
  assert.equal(Object.isFrozen(accepted.model), true);
  assert.deepEqual(module.confirmationIntent({ type: 'confirm' }), { type: 'confirm' });
  assert.deepEqual(module.confirmationIntent({ type: 'cancel' }), { type: 'cancel' });
  assert.throws(
    () => module.confirmationEnvelope({ generation: 3, confirmation }, 3),
    /stale or invalid/,
  );
  assert.throws(
    () => module.confirmationEnvelope({ generation: 4, confirmation: { ...confirmation, authority: true } }, 3),
    /unrecognized or missing fields/,
  );
  assert.throws(
    () => module.confirmationEnvelope({ generation: 4, confirmation: { ...confirmation, title: 'Safe\u202eexe' } }, 3),
    /unsafe or unbounded/,
  );
  assert.throws(
    () => module.confirmationIntent({ type: 'confirm', destination: '/forged' }),
    /unrecognized or missing fields/,
  );
  assert.equal(module.confirmationKeyboardAction('Escape', false, false, false), 'cancel');
  assert.equal(module.confirmationKeyboardAction('Tab', true, true, false), 'focus-confirm');
  assert.equal(module.confirmationKeyboardAction('Tab', false, false, true), 'focus-cancel');
  assert.equal(module.confirmationKeyboardAction('Enter', false, false, true), null);
});

test('confirmation focus restoration crosses open shadow roots without stealing newer focus', async () => {
  const output = await build({
    entryPoints: [join(root, 'src/organisms/confirmation-dialog.tsx')],
    bundle: true,
    format: 'cjs',
    platform: 'node',
    write: false,
  });
  const require = createRequire(import.meta.url);
  const module = { exports: {} };
  Function('require', 'module', 'exports', output.outputFiles[0].text)(require, module, module.exports);
  const OriginalHTMLElement = globalThis.HTMLElement;
  class FakeHTMLElement {
    constructor({ connected = true, contains = () => false } = {}) {
      this.isConnected = connected;
      this.shadowRoot = null;
      this.focusCalls = [];
      this.contains = contains;
    }
    focus(options) { this.focusCalls.push(options); }
  }
  globalThis.HTMLElement = FakeHTMLElement;
  try {
    const trigger = new FakeHTMLElement();
    const islandHost = new FakeHTMLElement();
    islandHost.shadowRoot = { activeElement: trigger };
    assert.equal(module.exports.deepestActiveElement({ activeElement: islandHost }), trigger);
    assert.equal(module.exports.deepestActiveElement({ activeElement: trigger }), trigger);

    const dialogButton = new FakeHTMLElement();
    const confirmationHost = new FakeHTMLElement();
    confirmationHost.shadowRoot = { activeElement: dialogButton };
    const dialog = new FakeHTMLElement({ contains: (candidate) => candidate === dialogButton });
    const documentRoot = { activeElement: confirmationHost };
    assert.equal(module.exports.restoreDialogFocus(documentRoot, trigger, dialog), true);
    assert.deepEqual(trigger.focusCalls, [{ preventScroll: true }]);

    const lightDomTrigger = new FakeHTMLElement();
    documentRoot.activeElement = dialogButton;
    assert.equal(module.exports.restoreDialogFocus(documentRoot, lightDomTrigger, dialog), true);
    assert.deepEqual(lightDomTrigger.focusCalls, [{ preventScroll: true }]);

    const disconnectedTrigger = new FakeHTMLElement({ connected: false });
    assert.equal(module.exports.restoreDialogFocus(documentRoot, disconnectedTrigger, dialog), false);
    assert.deepEqual(disconnectedTrigger.focusCalls, []);

    documentRoot.activeElement = new FakeHTMLElement();
    assert.equal(module.exports.restoreDialogFocus(documentRoot, trigger, dialog), false);
    assert.equal(trigger.focusCalls.length, 1, 'a newer external focus target must not be stolen');

    const visiblePage = new FakeHTMLElement();
    visiblePage.inert = false;
    const requestedIds = [];
    const shadowRequestedIds = [];
    const productionHost = new FakeHTMLElement();
    productionHost.shadowRoot = {
      getElementById(id) {
        shadowRequestedIds.push(id);
        return id === 'mesh-react-page-content' ? visiblePage : null;
      },
    };
    const pageDocument = {
      getElementById(id) {
        requestedIds.push(id);
        return id === 'mesh-app-next' ? productionHost : null;
      },
    };
    assert.equal(
      pageDocument.getElementById('mesh-react-page-content'),
      null,
      'the document must not invent access to an element inside the production shadow root',
    );
    requestedIds.length = 0;
    const restoreIsolation = module.exports.isolateVisiblePage(pageDocument);
    assert.deepEqual(requestedIds, ['mesh-app-next']);
    assert.deepEqual(shadowRequestedIds, ['mesh-react-page-content']);
    assert.equal(visiblePage.inert, true, 'the visible React page remained interactive behind confirmation');
    restoreIsolation();
    assert.equal(visiblePage.inert, false, 'closing confirmation did not restore prior page isolation');
    visiblePage.inert = true;
    module.exports.isolateVisiblePage(pageDocument)();
    assert.equal(visiblePage.inert, true, 'closing confirmation weakened pre-existing page isolation');
  } finally {
    if (OriginalHTMLElement === undefined) delete globalThis.HTMLElement;
    else globalThis.HTMLElement = OriginalHTMLElement;
  }
});

test('the confirmation dialog defaults to safety, traps focus, and isolates the page', () => {
  const dialog = readFileSync(join(root, 'src/organisms/confirmation-dialog.tsx'), 'utf8');
  const button = readFileSync(join(root, 'src/atoms/button.tsx'), 'utf8');
  assert.match(dialog, /aria-modal="true"/);
  assert.match(dialog, /aria-labelledby/);
  assert.match(dialog, /cancelButton\.current\?\.focus\(\{ preventScroll: true \}\)/);
  assert.match(dialog, /page\.inert = true/);
  assert.match(dialog, /page\.inert = wasInert/);
  assert.match(dialog, /getElementById\("mesh-app-next"\)/);
  assert.match(dialog, /shadowRoot\s*\?\.\s*getElementById\("mesh-react-page-content"\)/);
  assert.match(dialog, /isolateVisiblePage\(document\)/);
  assert.match(dialog, /deepestActiveElement\(document\)/);
  assert.match(dialog, /restoreDialogFocus\(document, previousFocus, mountedDialog\)/);
  assert.match(dialog, /confirmationKeyboardAction/);
  assert.match(dialog, /event\.preventDefault\(\)/);
  assert.doesNotMatch(dialog, /onClick=\{[^}]*backdrop|onClick=\{\(\) => onIntent\(\{ type: "cancel" \}\)\}[^\n]*data-mesh-proof="confirmation-backdrop"/);
  assert.match(button, /forwardRef<HTMLButtonElement, ButtonProps>/);
  assert.match(button, /min-h-11/);
  assert.match(button, /focus-visible:ring-2/);
});

test('the alpha workbench exposes keyboard focus, audible diff meaning, and AA component boundaries', () => {
  const navigator = readFileSync(join(root, 'src/molecules/change-navigator.tsx'), 'utf8');
  const review = readFileSync(join(root, 'src/organisms/artifact-review.tsx'), 'utf8');
  const styles = readFileSync(join(root, 'src/styles.css'), 'utf8');
  assert.match(navigator, /aria-pressed=\{selected\}/);
  assert.match(navigator, /const roving = selected \|\| \(selectedIndex < 0 && index === 0\)/);
  assert.match(navigator, /tabIndex=\{roving \? 0 : -1\}/);
  assert.match(navigator, /placeholder="Search changed files"/);
  assert.match(navigator, />All statuses<\/option>/);
  assert.match(navigator, />All types<\/option>/);
  assert.match(navigator, /focus-visible:ring-2/);
  assert.match(review, /aria-busy=\{artifactPreviewLoading\}/);
  assert.match(review, /role=\{failed \? "alert" : "status"\}/);
  assert.match(review, /lineStatus\(line\)/);
  assert.match(review, /No corresponding \$\{side\.toLowerCase\(\)\} line/);

  const token = (name) => {
    const match = styles.match(new RegExp(`--${name}:\\s*(#[0-9a-f]{6})`, 'u'));
    assert.ok(match, `missing ${name} token`);
    return match[1];
  };
  const luminance = (color) => {
    const channels = [1, 3, 5].map((start) => Number.parseInt(color.slice(start, start + 2), 16) / 255)
      .map((value) => value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4);
    return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2];
  };
  const contrast = (left, right) => {
    const values = [luminance(left), luminance(right)].sort((a, b) => b - a);
    return (values[0] + 0.05) / (values[1] + 0.05);
  };
  const border = token('border');
  for (const surface of ['background', 'card', 'muted', 'secondary']) {
    assert.ok(contrast(border, token(surface)) >= 3, `border contrast failed on ${surface}`);
  }
});

test('artifact renderer failures produce one atomic alert and keep retry available', async () => {
  const localRequire = createRequire(import.meta.url);
  const output = await build({
    stdin: {
      contents: `
        import React from "react";
        import { renderToStaticMarkup } from "react-dom/server";
        import { ArtifactReview } from "./src/organisms/artifact-review.tsx";
        module.exports.renderReview = (props) =>
          renderToStaticMarkup(React.createElement(ArtifactReview, props));
      `,
      resolveDir: root,
      loader: 'js',
    },
    bundle: true,
    format: 'cjs',
    platform: 'node',
    packages: 'external',
    write: false,
  });
  const review = { exports: {} };
  Function('require', 'module', 'exports', output.outputFiles[0].text)(
    localRequire,
    review,
    review.exports,
  );
  const change = {
    id: 'artifact',
    path: 'finance/report.pdf',
    kind: 'pdf',
    kindLabel: 'PDF',
    summary: 'One page changed',
    impact: 'Review the changed page',
    beforeLabel: 'Earlier version',
    afterLabel: 'Current version',
    beforeValues: [],
    afterValues: [],
    diffHunks: [],
    beforeVersionId: '44'.repeat(32),
    beforeContentDigest: '55'.repeat(32),
    afterVersionId: '66'.repeat(32),
    afterContentDigest: '77'.repeat(32),
  };
  const model = {
    workspaceName: 'Finance',
    versionLabel: 'Saved point',
    bundleLabel: 'Bundle 123',
    changes: [change],
    selectedChangeId: change.id,
    mode: 'visual',
    diffLayout: 'split',
    canRenderArtifactPreview: true,
    canInspectExactCopies: true,
    canRecordReview: false,
    canApprove: false,
    canApproveAndExport: false,
    canExportGit: false,
    canExportPrivateCopy: false,
    approvalReason: 'Review before approving.',
  };
  const render = (overrides = {}) => review.exports.renderReview({
    model,
    onIntent() {},
    artifactPreview: null,
    artifactPreviewLoading: false,
    artifactPreviewError: null,
    ...overrides,
  });
  const alertCount = (html) => (html.match(/role="alert"/gu) || []).length;

  const opaque = render({
    model: {
      ...model,
      changes: [{
        ...change,
        path: '.DS_Store',
        kind: 'file',
        kindLabel: 'File',
      }],
    },
  });
  const opaqueOpen = opaque.match(/<button[^>]*>Open in default app<\/button>/u)?.[0];
  const opaqueReveal = opaque.match(/<button[^>]*>Reveal in Finder<\/button>/u)?.[0];
  assert.match(opaqueOpen, /disabled=""/u);
  assert.match(opaqueOpen, /aria-describedby="before-default-app-unavailable"/u);
  assert.doesNotMatch(opaqueReveal, /disabled=""/u);
  assert.match(opaque, /Default-app opening stays unavailable until native inspection admits this exact content type/u);
  const unadmittedPdfOpen = render().match(/<button[^>]*>Open in default app<\/button>/u)?.[0];
  assert.match(unadmittedPdfOpen, /disabled=""/u);
  const beforePreview = {
    side: 'before',
    versionId: change.beforeVersionId,
    contentDigest: change.beforeContentDigest,
    imageDataUrl: 'data:image/png;base64,AA==',
    pageNumber: 1,
    pageCount: 1,
  };
  const afterPreview = {
    side: 'after',
    versionId: change.afterVersionId,
    contentDigest: change.afterContentDigest,
    imageDataUrl: 'data:image/png;base64,AA==',
    pageNumber: 1,
    pageCount: 1,
  };
  const pdfPreview = {
    changeId: change.id,
    requestedPage: 1,
    before: beforePreview,
    after: afterPreview,
    beforeAbsentPage: null,
    afterAbsentPage: null,
    beforeError: null,
    afterError: null,
  };
  const admittedPdf = render({ artifactPreview: pdfPreview });
  const admittedPdfOpen = admittedPdf.match(/<button[^>]*>Open in default app<\/button>/u)?.[0];
  assert.doesNotMatch(admittedPdfOpen, /disabled=""/u);
  const absentPdf = render({
    artifactPreview: {
      ...pdfPreview,
      changeId: change.id,
      requestedPage: 2,
      before: null,
      after: { ...afterPreview, pageNumber: 2, pageCount: 2 },
      beforeAbsentPage: {
        side: 'before',
        versionId: change.beforeVersionId,
        contentDigest: change.beforeContentDigest,
        pageCount: 1,
      },
      afterAbsentPage: null,
      beforeError: null,
      afterError: null,
    },
  });
  const absentPdfOpen = absentPdf.match(/<button[^>]*>Open in default app<\/button>/u)?.[0];
  assert.doesNotMatch(absentPdfOpen, /disabled=""/u);

  const imageChange = {
    ...change,
    id: 'saved-image',
    path: 'assets/hero.png',
    kind: 'image',
    kindLabel: 'Image',
  };
  const imageModel = { ...model, changes: [imageChange], selectedChangeId: imageChange.id };
  const imageWithoutEvidence = render({ model: imageModel });
  const imageOpenWithoutEvidence = imageWithoutEvidence.match(/<button[^>]*>Open in default app<\/button>/u)?.[0];
  const imageRevealWithoutEvidence = imageWithoutEvidence.match(/<button[^>]*>Reveal in Finder<\/button>/u)?.[0];
  assert.match(imageOpenWithoutEvidence, /disabled=""/u);
  assert.doesNotMatch(imageRevealWithoutEvidence, /disabled=""/u);
  const imagePreview = {
    ...pdfPreview,
    changeId: imageChange.id,
    before: { ...beforePreview, pageNumber: null, pageCount: null },
    after: { ...afterPreview, pageNumber: null, pageCount: null },
  };
  const admittedImage = render({ model: imageModel, artifactPreview: imagePreview });
  const admittedImageOpen = admittedImage.match(/<button[^>]*>Open in default app<\/button>/u)?.[0];
  assert.doesNotMatch(admittedImageOpen, /disabled=""/u);
  const crossChangeImage = render({
    model: imageModel,
    artifactPreview: { ...imagePreview, changeId: 'different-change' },
  });
  const crossChangeOpen = crossChangeImage.match(/<button[^>]*>Open in default app<\/button>/u)?.[0];
  assert.match(crossChangeOpen, /disabled=""/u);

  const rejected = render({ artifactPreviewError: 'Native renderer failed.' });
  assert.equal(alertCount(rejected), 1);
  assert.match(rejected, /role="alert" aria-atomic="true">Native renderer failed\.<\/p>/u);
  const retry = rejected.match(/<button[^>]*>Try visual comparison again<\/button>/u)?.[0];
  assert.ok(retry, 'renderer failure must leave a named retry control in the same toolbar');
  assert.doesNotMatch(retry, /\sdisabled(?:[=\s>])/u);

  const sideFailures = render({
    artifactPreview: {
      changeId: change.id,
      requestedPage: 1,
      before: null,
      after: null,
      beforeAbsentPage: null,
      afterAbsentPage: null,
      beforeError: 'Earlier renderer failed.',
      afterError: 'Current renderer failed.',
    },
  });
  assert.equal(alertCount(sideFailures), 1);
  assert.match(
    sideFailures,
    /Earlier version: Earlier renderer failed\. Current version: Current renderer failed\./u,
  );

  const mixedSideFailure = render({
    artifactPreview: {
      changeId: change.id,
      requestedPage: 2,
      before: {
        side: 'before',
        versionId: change.beforeVersionId,
        contentDigest: change.beforeContentDigest,
        imageDataUrl: 'data:image/png;base64,AA==',
        pageNumber: 2,
        pageCount: 4,
      },
      after: null,
      beforeAbsentPage: null,
      afterAbsentPage: null,
      beforeError: null,
      afterError: 'Current renderer failed on page 2.',
    },
  });
  const pageLabel = mixedSideFailure.match(/<strong[^>]*data-mesh-proof="pdf-page-status"[^>]*>[^<]*<\/strong>/u)?.[0];
  assert.ok(pageLabel, 'mixed-side failure must retain a visible current-page label');
  assert.doesNotMatch(pageLabel, /role="status"|aria-live=|aria-atomic=/u);
  const comparisonAnnouncements = alertCount(mixedSideFailure)
    + (mixedSideFailure.match(/data-mesh-proof="pdf-page-status"[^>]*(?:role="status"|aria-live=)/gu) || []).length;
  assert.equal(comparisonAnnouncements, 1, 'the exact renderer alert must be the sole comparison announcement');
  assert.match(
    mixedSideFailure,
    /role="alert" aria-atomic="true">Current version: Current renderer failed on page 2\.<\/p>/u,
  );

  assert.equal(alertCount(render()), 0, 'an unloaded comparison is ordinary state, not an alert');
  const absentPages = render({
    artifactPreview: {
      changeId: change.id,
      requestedPage: 2,
      before: null,
      after: null,
      beforeAbsentPage: {
        side: 'before',
        versionId: change.beforeVersionId,
        contentDigest: change.beforeContentDigest,
        pageCount: 1,
      },
      afterAbsentPage: {
        side: 'after',
        versionId: change.afterVersionId,
        contentDigest: change.afterContentDigest,
        pageCount: 1,
      },
      beforeError: null,
      afterError: null,
    },
  });
  assert.equal(alertCount(absentPages), 0, 'an absent PDF page is descriptive, not an alert');
});

test('PDF content review keeps exact page navigation available', async () => {
  const localRequire = createRequire(import.meta.url);
  const output = await build({
    stdin: {
      contents: `
        import React from "react";
        import { renderToStaticMarkup } from "react-dom/server";
        import { ArtifactReview } from "./src/organisms/artifact-review.tsx";
        module.exports.renderReview = (props) =>
          renderToStaticMarkup(React.createElement(ArtifactReview, props));
      `,
      resolveDir: root,
      loader: 'js',
    },
    bundle: true,
    format: 'cjs',
    platform: 'node',
    packages: 'external',
    write: false,
  });
  const review = { exports: {} };
  Function('require', 'module', 'exports', output.outputFiles[0].text)(
    localRequire,
    review,
    review.exports,
  );
  const beforeVersionId = '44'.repeat(32);
  const afterVersionId = '66'.repeat(32);
  const change = {
    id: 'artifact',
    path: 'finance/report.pdf',
    kind: 'pdf',
    kindLabel: 'PDF',
    summary: 'A later page changed',
    impact: 'Review every changed page',
    beforeLabel: 'Earlier version',
    afterLabel: 'Current version',
    beforeValues: [],
    afterValues: [],
    diffHunks: [],
    beforeVersionId,
    beforeContentDigest: '55'.repeat(32),
    afterVersionId,
    afterContentDigest: '77'.repeat(32),
  };
  const side = (label, versionId, contentDigest, text) => ({
    side: label,
    versionId,
    contentDigest,
    imageDataUrl: 'data:image/png;base64,AA==',
    pageNumber: 2,
    pageCount: 4,
    textSource: 'macos-pdfkit-page-text-v1',
    textLines: [text],
    textSections: [{ label: 'Page 2', lineStart: 0, lineCount: 1 }],
    textTruncated: false,
  });
  const html = review.exports.renderReview({
    model: {
      workspaceName: 'Finance',
      versionLabel: 'Saved point',
      bundleLabel: 'Bundle 123',
      changes: [change],
      selectedChangeId: change.id,
      mode: 'content',
      diffLayout: 'inline',
      canRenderArtifactPreview: true,
      canInspectExactCopies: true,
      canRecordReview: false,
      canApprove: false,
      canApproveAndExport: false,
      canExportGit: false,
      canExportPrivateCopy: false,
      approvalReason: 'Review before approving.',
    },
    onIntent() {},
    artifactPreview: {
      changeId: change.id,
      requestedPage: 2,
      before: side('before', beforeVersionId, change.beforeContentDigest, 'Before page two'),
      after: side('after', afterVersionId, change.afterContentDigest, 'After page two'),
      beforeAbsentPage: null,
      afterAbsentPage: null,
      beforeError: null,
      afterError: null,
    },
    artifactPreviewLoading: false,
    artifactPreviewError: null,
  });
  assert.match(html, /aria-label="PDF page content comparison"/);
  assert.match(html, /Page 2 of 4 is shown\./);
  assert.match(html, />Previous page</);
  assert.match(html, />Next page</);
});

test('PDF page navigation announces async page changes without disabling the focused control', async () => {
  const localRequire = createRequire(import.meta.url);
  const output = await build({
    stdin: {
      contents: `
        import React from "react";
        import { renderToStaticMarkup } from "react-dom/server";
        import { PdfPageNavigation } from "./src/organisms/artifact-review.tsx";
        module.exports.renderNavigation = (props) =>
          renderToStaticMarkup(React.createElement(PdfPageNavigation, props));
        module.exports.navigation = (props) => PdfPageNavigation(props);
      `,
      resolveDir: root,
      loader: 'js',
    },
    bundle: true,
    format: 'cjs',
    platform: 'node',
    packages: 'external',
    write: false,
  });
  const navigation = { exports: {} };
  Function('require', 'module', 'exports', output.outputFiles[0].text)(
    localRequire,
    navigation,
    navigation.exports,
  );
  const calls = [];
  const props = {
    label: 'PDF page content comparison',
    currentPage: 2,
    totalPages: 4,
    loading: false,
    failed: false,
    canLoad: true,
    onLoad: (pageNumber) => calls.push(pageNumber),
  };

  const pageTwo = navigation.exports.renderNavigation(props);
  assert.match(pageTwo, /data-mesh-proof="pdf-page-status"/);
  assert.match(pageTwo, /role="status"/);
  assert.match(pageTwo, /aria-live="polite"/);
  assert.match(pageTwo, /aria-atomic="true"/);
  assert.match(pageTwo, />Page 2 of 4 is shown\.<\/strong>/);

  const tree = navigation.exports.navigation(props);
  const next = tree.props.children[0].props.children[2];
  next.props.onClick();
  assert.deepEqual(calls, [3]);

  const loadingProps = { ...props, loading: true };
  const loading = navigation.exports.renderNavigation(loadingProps);
  assert.match(loading, />Loading PDF page comparison\. Page 2 of 4 remains shown\.<\/strong>/);
  assert.match(loading, /aria-disabled="true"[^>]*>Next page<\/button>/);
  assert.doesNotMatch(loading, /disabled=""[^>]*>Next page<\/button>/);
  const loadingTree = navigation.exports.navigation(loadingProps);
  const loadingNext = loadingTree.props.children[0].props.children[2];
  loadingNext.props.onClick();
  assert.deepEqual(calls, [3], 'a pending request must not dispatch a duplicate page load');

  const failed = navigation.exports.renderNavigation({ ...props, failed: true });
  assert.match(failed, />PDF page comparison could not be loaded\. Page 2 of 4 remains selected\.<\/strong>/);

  const pageThree = navigation.exports.renderNavigation({ ...props, currentPage: 3 });
  assert.match(pageThree, /data-mesh-proof="pdf-page-status"/);
  assert.match(pageThree, />Page 3 of 4 is shown\.<\/strong>/);

  const lastPage = navigation.exports.renderNavigation({ ...props, currentPage: 4 });
  assert.match(lastPage, /aria-disabled="true"[^>]*>Next page<\/button>/);
  assert.doesNotMatch(lastPage, /disabled=""[^>]*>Next page<\/button>/);
});

test('PDF page navigation stops truthfully at the native preview bound', async () => {
  const localRequire = createRequire(import.meta.url);
  const output = await build({
    stdin: {
      contents: `
        import React from "react";
        import { renderToStaticMarkup } from "react-dom/server";
        import { ArtifactReview } from "./src/organisms/artifact-review.tsx";
        module.exports.renderReview = (props) =>
          renderToStaticMarkup(React.createElement(ArtifactReview, props));
      `,
      resolveDir: root,
      loader: 'js',
    },
    bundle: true,
    format: 'cjs',
    platform: 'node',
    packages: 'external',
    write: false,
  });
  const review = { exports: {} };
  Function('require', 'module', 'exports', output.outputFiles[0].text)(
    localRequire,
    review,
    review.exports,
  );
  const version = '88'.repeat(32);
  const digest = '99'.repeat(32);
  const change = {
    id: 'bounded-pdf',
    path: 'annual-report.pdf',
    kind: 'pdf',
    kindLabel: 'PDF',
    summary: 'A later page changed',
    impact: 'Review every changed page',
    beforeLabel: 'Earlier version',
    afterLabel: 'Current version',
    beforeValues: [],
    afterValues: [],
    diffHunks: [],
    beforeVersionId: version,
    beforeContentDigest: digest,
    afterVersionId: version,
    afterContentDigest: digest,
  };
  const side = (label) => ({
    side: label,
    versionId: version,
    contentDigest: digest,
    imageDataUrl: 'data:image/png;base64,AA==',
    pageNumber: 64,
    pageCount: 100,
    textSource: 'macos-pdfkit-page-text-v1',
    textLines: ['Page 64'],
    textSections: [{ label: 'Page 64', lineStart: 0, lineCount: 1 }],
    textTruncated: false,
  });
  const html = review.exports.renderReview({
    model: {
      workspaceName: 'Finance',
      versionLabel: 'Saved point',
      bundleLabel: 'Bundle 123',
      changes: [change],
      selectedChangeId: change.id,
      mode: 'content',
      diffLayout: 'inline',
      canRenderArtifactPreview: true,
      canInspectExactCopies: true,
      canRecordReview: false,
      canApprove: false,
      canApproveAndExport: false,
      canExportGit: false,
      canExportPrivateCopy: false,
      approvalReason: 'Review before approving.',
    },
    onIntent() {},
    artifactPreview: {
      changeId: change.id,
      requestedPage: 64,
      before: side('before'),
      after: side('after'),
      beforeAbsentPage: null,
      afterAbsentPage: null,
      beforeError: null,
      afterError: null,
    },
    artifactPreviewLoading: false,
    artifactPreviewError: null,
  });
  assert.match(html, /Page 64 of 100/);
  assert.match(html, /preview is limited to the first 64 pages/i);
  assert.match(html, /<button[^>]*disabled[^>]*>Next page<\/button>/);
  assert.match(html, /Inspect exact copies/);
});

test('review choices use one tab stop with deterministic arrow, Home, and End movement', async () => {
  const output = await build({
    entryPoints: [join(root, 'src/models/roving-selection.ts')],
    bundle: true,
    format: 'esm',
    platform: 'node',
    write: false,
  });
  const roving = await import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString('base64')}`);
  assert.equal(roving.rovingSelectionIndex(40, 12, 'ArrowDown', 'vertical-clamp'), 13);
  assert.equal(roving.rovingSelectionIndex(40, 0, 'ArrowUp', 'vertical-clamp'), 0);
  assert.equal(roving.rovingSelectionIndex(40, 12, 'Home', 'vertical-clamp'), 0);
  assert.equal(roving.rovingSelectionIndex(40, 12, 'End', 'vertical-clamp'), 39);
  assert.equal(roving.rovingSelectionIndex(2, 1, 'ArrowRight', 'all-wrap'), 0);
  assert.equal(roving.rovingSelectionIndex(2, 0, 'ArrowLeft', 'all-wrap'), 1);
  assert.equal(roving.rovingSelectionIndex(2, 0, 'Tab', 'all-wrap'), null);
  assert.equal(roving.rovingSelectionIndex(0, 0, 'ArrowDown', 'vertical-clamp'), null);
});
