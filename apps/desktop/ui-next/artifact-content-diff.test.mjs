import assert from "node:assert/strict";
import { join } from "node:path";
import test from "node:test";
import { build } from "esbuild";

const root = new URL(".", import.meta.url).pathname;

async function loadArtifactContentDiff() {
  const output = await build({
    entryPoints: [join(root, "src/models/artifact-content-diff.ts")],
    bundle: true,
    format: "esm",
    platform: "node",
    write: false,
  });
  return import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString("base64")}`);
}

const cases = Object.freeze([
  Object.freeze({
    source: "mesh-xlsx-cell-formula-v1",
    kind: "spreadsheet",
    section: "Budget",
    completeTitle: "Workbook changed; extracted content and structure match",
  }),
  Object.freeze({
    source: "mesh-pptx-slide-text-v1",
    kind: "presentation",
    section: "Slide 1",
    completeTitle: "Presentation changed; extracted slide content and structure match",
  }),
  Object.freeze({
    source: "mesh-docx-block-text-v1",
    kind: "document",
    section: "Section 1 · People plan",
    completeTitle: "Document changed; extracted content and visibility match",
  }),
  Object.freeze({
    source: "macos-pdfkit-page-text-v1",
    kind: "pdf",
    section: "Page 1",
    completeTitle: "PDF changed; selected page text matches",
  }),
  Object.freeze({
    source: "macos-quick-look-visible-text",
    kind: "document",
    section: "Document",
    completeTitle: "Artifact changed; visible text matches",
  }),
]);

function comparisonFixture(candidate, { truncated, afterLine = "Retained extracted content" }) {
  const side = (sideName, line) => Object.freeze({
    side: sideName,
    versionId: sideName === "before" ? "11".repeat(32) : "22".repeat(32),
    contentDigest: sideName === "before" ? "33".repeat(32) : "44".repeat(32),
    imageDataUrl: "data:image/png;base64,YQ==",
    pageNumber: candidate.kind === "pdf" ? 1 : null,
    pageCount: candidate.kind === "pdf" ? 1 : null,
    textSource: candidate.source,
    textLines: Object.freeze([line]),
    textSections: Object.freeze([Object.freeze({
      label: candidate.section,
      lineStart: 0,
      lineCount: 1,
    })]),
    textTruncated: truncated,
  });
  const change = Object.freeze({
    id: `change:${candidate.source}`,
    kind: candidate.kind,
    beforeVersionId: "11".repeat(32),
    afterVersionId: "22".repeat(32),
    beforeContentDigest: "33".repeat(32),
    afterContentDigest: "44".repeat(32),
  });
  return Object.freeze({
    change,
    preview: Object.freeze({
      changeId: change.id,
      requestedPage: 1,
      before: side("before", "Retained extracted content"),
      after: side("after", afterLine),
      beforeAbsentPage: null,
      afterAbsentPage: null,
      beforeError: null,
      afterError: null,
    }),
  });
}

test("bounded artifact prefixes never claim that unseen content matches", async () => {
  const { artifactContentComparison } = await loadArtifactContentDiff();
  for (const candidate of cases) {
    const fixture = comparisonFixture(candidate, { truncated: true });
    const comparison = artifactContentComparison(fixture.change, fixture.preview);
    assert.ok(comparison, `${candidate.source} did not produce a comparison`);
    assert.doesNotMatch(comparison.title, /match|unchanged/iu, candidate.source);
    assert.match(comparison.title, /extracted .* prefix/iu, candidate.source);
    assert.match(comparison.note, /content beyond the extracted prefix was not compared/iu, candidate.source);
  }
});

test("complete artifact comparisons retain their format-specific match copy", async () => {
  const { artifactContentComparison } = await loadArtifactContentDiff();
  for (const candidate of cases) {
    const fixture = comparisonFixture(candidate, { truncated: false });
    const comparison = artifactContentComparison(fixture.change, fixture.preview);
    assert.ok(comparison, `${candidate.source} did not produce a comparison`);
    assert.equal(comparison.title, candidate.completeTitle);
    assert.doesNotMatch(comparison.note, /bounded limit|not compared/iu);
  }
});

test("identical saved artifact bytes cannot be reported as content changes", async () => {
  const { artifactContentComparison } = await loadArtifactContentDiff();
  for (const candidate of cases) {
    const fixture = comparisonFixture(candidate, {
      truncated: false,
      // Renderer output is supporting evidence, not stronger identity than the verified digest.
      // Deliberately disagree here so a renderer change cannot invent a content change.
      afterLine: "Different renderer extraction",
    });
    const digest = fixture.change.beforeContentDigest;
    const change = Object.freeze({ ...fixture.change, afterContentDigest: digest });
    const preview = Object.freeze({
      ...fixture.preview,
      after: Object.freeze({ ...fixture.preview.after, contentDigest: digest }),
    });

    const comparison = artifactContentComparison(change, preview);
    assert.ok(comparison, `${candidate.source} did not produce a comparison`);
    assert.match(comparison.title, /exact .* bytes match/iu, candidate.source);
    assert.doesNotMatch(comparison.title, /changed/iu, candidate.source);
    assert.match(
      comparison.note,
      /saved file bytes are identical.*path or file metadata.*not document content/iu,
      candidate.source,
    );
    assert.deepEqual(comparison.hunks, [], candidate.source);
    assert.deepEqual(comparison.sectionLabels, [], candidate.source);
  }
});

test("a hidden slide remains an explicit presentation structure change", async () => {
  const { artifactContentComparison } = await loadArtifactContentDiff();
  const fixture = comparisonFixture(cases[1], { truncated: false });
  const preview = Object.freeze({
    ...fixture.preview,
    after: Object.freeze({
      ...fixture.preview.after,
      textLines: Object.freeze(["<slide visibility: hidden>", "Retained extracted content"]),
      textSections: Object.freeze([Object.freeze({
        label: cases[1].section,
        lineStart: 0,
        lineCount: 2,
      })]),
    }),
  });

  const comparison = artifactContentComparison(fixture.change, preview);
  assert.ok(comparison);
  assert.equal(comparison.title, "Slide content and structure changes");
  assert.match(comparison.note, /slide visibility/iu);
  assert.deepEqual(
    comparison.hunks[0].lines
      .filter((line) => line.kind !== "context")
      .map(({ kind, text }) => ({ kind, text })),
    [{ kind: "added", text: "<slide visibility: hidden>" }],
  );
});

test("a hidden presentation object remains an explicit structure change", async () => {
  const { artifactContentComparison } = await loadArtifactContentDiff();
  const fixture = comparisonFixture(cases[1], { truncated: false });
  const preview = Object.freeze({
    ...fixture.preview,
    after: Object.freeze({
      ...fixture.preview.after,
      textLines: Object.freeze([
        "<object 2 visibility: hidden · Forecast box>",
        "Retained extracted content",
      ]),
      textSections: Object.freeze([Object.freeze({
        label: cases[1].section,
        lineStart: 0,
        lineCount: 2,
      })]),
    }),
  });

  const comparison = artifactContentComparison(fixture.change, preview);
  assert.ok(comparison);
  assert.equal(comparison.title, "Slide content and structure changes");
  assert.match(comparison.note, /object visibility/iu);
  assert.deepEqual(
    comparison.hunks[0].lines
      .filter((line) => line.kind !== "context")
      .map(({ kind, text }) => ({ kind, text })),
    [{ kind: "added", text: "<object 2 visibility: hidden · Forecast box>" }],
  );
});

test("visible marker-like slide text cannot erase a hidden-object change", async () => {
  const { artifactContentComparison } = await loadArtifactContentDiff();
  const fixture = comparisonFixture(cases[1], { truncated: false });
  const marker = "<object 2 visibility: hidden · Forecast box>";
  const preview = Object.freeze({
    ...fixture.preview,
    before: Object.freeze({
      ...fixture.preview.before,
      textLines: Object.freeze([`\\${marker.slice(0, -1)}\\>`]),
    }),
    after: Object.freeze({
      ...fixture.preview.after,
      textLines: Object.freeze([marker]),
    }),
  });

  const comparison = artifactContentComparison(fixture.change, preview);
  assert.ok(comparison);
  assert.equal(comparison.title, "Slide content and structure changes");
  assert.deepEqual(
    comparison.hunks[0].lines
      .filter((line) => line.kind !== "context")
      .map(({ kind, text }) => ({ kind, text })),
    [
      { kind: "removed", text: "\\<object 2 visibility: hidden · Forecast box\\>" },
      { kind: "added", text: marker },
    ],
  );
});

test("control whitespace and literal escapes remain different object-name changes", async () => {
  const { artifactContentComparison } = await loadArtifactContentDiff();
  const fixture = comparisonFixture(cases[1], { truncated: false });
  const controlWhitespace = String.raw`<object 2 visibility: hidden · Forecast\tQ1\rQ2\nQ3>`;
  const literalEscapes = String.raw`<object 2 visibility: hidden · Forecast\\tQ1\\rQ2\\nQ3>`;
  const preview = Object.freeze({
    ...fixture.preview,
    before: Object.freeze({
      ...fixture.preview.before,
      textLines: Object.freeze([controlWhitespace]),
    }),
    after: Object.freeze({
      ...fixture.preview.after,
      textLines: Object.freeze([literalEscapes]),
    }),
  });

  const comparison = artifactContentComparison(fixture.change, preview);
  assert.ok(comparison);
  assert.equal(comparison.title, "Slide content and structure changes");
  assert.deepEqual(
    comparison.hunks[0].lines
      .filter((line) => line.kind !== "context")
      .map(({ kind, text }) => ({ kind, text })),
    [
      { kind: "removed", text: controlWhitespace },
      { kind: "added", text: literalEscapes },
    ],
  );
});

test("presentation line-break structure remains distinct from spaces and literal escapes", async () => {
  const { artifactContentComparison } = await loadArtifactContentDiff();
  const fixture = comparisonFixture(cases[1], { truncated: false });
  const lineBreak = String.raw`less\nmore`;

  for (const replacement of ["less more", String.raw`less\\nmore`]) {
    const preview = Object.freeze({
      ...fixture.preview,
      before: Object.freeze({
        ...fixture.preview.before,
        textLines: Object.freeze([lineBreak]),
      }),
      after: Object.freeze({
        ...fixture.preview.after,
        textLines: Object.freeze([replacement]),
      }),
    });

    const comparison = artifactContentComparison(fixture.change, preview);
    assert.ok(comparison);
    assert.equal(comparison.title, "Slide content and structure changes");
    assert.deepEqual(
      comparison.hunks[0].lines
        .filter((line) => line.kind !== "context")
        .map(({ kind, text }) => ({ kind, text })),
      [
        { kind: "removed", text: lineBreak },
        { kind: "added", text: replacement },
      ],
    );
  }
});

test("presentation tab structure remains distinct from absence and literal escapes", async () => {
  const { artifactContentComparison } = await loadArtifactContentDiff();
  const fixture = comparisonFixture(cases[1], { truncated: false });
  const tab = String.raw`less\tmore`;

  for (const replacement of ["lessmore", String.raw`less\\tmore`]) {
    const preview = Object.freeze({
      ...fixture.preview,
      before: Object.freeze({
        ...fixture.preview.before,
        textLines: Object.freeze([tab]),
      }),
      after: Object.freeze({
        ...fixture.preview.after,
        textLines: Object.freeze([replacement]),
      }),
    });

    const comparison = artifactContentComparison(fixture.change, preview);
    assert.ok(comparison);
    assert.equal(comparison.title, "Slide content and structure changes");
    assert.deepEqual(
      comparison.hunks[0].lines
        .filter((line) => line.kind !== "context")
        .map(({ kind, text }) => ({ kind, text })),
      [
        { kind: "removed", text: tab },
        { kind: "added", text: replacement },
      ],
    );
  }
});

test("a Word line break remains distinct from a literal space", async () => {
  const { artifactContentComparison } = await loadArtifactContentDiff();
  const fixture = comparisonFixture(cases[2], { truncated: false });
  const lineBreak = String.raw`Approve\nthree hires`;
  const preview = Object.freeze({
    ...fixture.preview,
    before: Object.freeze({
      ...fixture.preview.before,
      textLines: Object.freeze(["Approve three hires"]),
    }),
    after: Object.freeze({
      ...fixture.preview.after,
      textLines: Object.freeze([lineBreak]),
    }),
  });

  const comparison = artifactContentComparison(fixture.change, preview);
  assert.ok(comparison);
  assert.equal(comparison.title, "Document content and visibility changes");
  assert.deepEqual(
    comparison.hunks[0].lines
      .filter((line) => line.kind !== "context")
      .map(({ kind, text }) => ({ kind, text })),
    [
      { kind: "removed", text: "Approve three hires" },
      { kind: "added", text: lineBreak },
    ],
  );
});

test("preserved Word boundary whitespace remains a legible document content change", async () => {
  const { artifactContentComparison } = await loadArtifactContentDiff();
  const fixture = comparisonFixture(cases[2], { truncated: false });
  const preview = Object.freeze({
    ...fixture.preview,
    after: Object.freeze({
      ...fixture.preview.after,
      textLines: Object.freeze([String.raw`\sRetained extracted content\s`]),
    }),
  });

  const comparison = artifactContentComparison(fixture.change, preview);
  assert.ok(comparison);
  assert.equal(comparison.title, "Document content and visibility changes");
  assert.match(comparison.note, /preserved whitespace/iu);
  assert.deepEqual(
    comparison.hunks[0].lines
      .filter((line) => line.kind !== "context")
      .map(({ kind, text }) => ({ kind, text })),
    [
      { kind: "removed", text: "Retained extracted content" },
      { kind: "added", text: String.raw`\sRetained extracted content\s` },
    ],
  );
});

test("a hidden Word run remains an explicit document visibility change", async () => {
  const { artifactContentComparison } = await loadArtifactContentDiff();
  const fixture = comparisonFixture(cases[2], { truncated: false });
  const preview = Object.freeze({
    ...fixture.preview,
    after: Object.freeze({
      ...fixture.preview.after,
      textLines: Object.freeze(["<hidden text: Retained extracted content>"]),
    }),
  });

  const comparison = artifactContentComparison(fixture.change, preview);
  assert.ok(comparison);
  assert.equal(comparison.title, "Document content and visibility changes");
  assert.match(comparison.note, /hidden-run visibility/iu);
  assert.match(comparison.note, /style-inherited visibility.*exact-copy inspection/iu);
  assert.deepEqual(
    comparison.hunks[0].lines
      .filter((line) => line.kind !== "context")
      .map(({ kind, text }) => ({ kind, text })),
    [
      { kind: "removed", text: "Retained extracted content" },
      { kind: "added", text: "<hidden text: Retained extracted content>" },
    ],
  );
});

test("a hidden worksheet row remains an explicit workbook structure change", async () => {
  const { artifactContentComparison } = await loadArtifactContentDiff();
  const fixture = comparisonFixture(cases[0], { truncated: false });
  const preview = Object.freeze({
    ...fixture.preview,
    after: Object.freeze({
      ...fixture.preview.after,
      textLines: Object.freeze(["<row 2 visibility: hidden>", "Retained extracted content"]),
      textSections: Object.freeze([Object.freeze({
        label: cases[0].section,
        lineStart: 0,
        lineCount: 2,
      })]),
    }),
  });

  const comparison = artifactContentComparison(fixture.change, preview);
  assert.ok(comparison);
  assert.equal(comparison.title, "Workbook content and structure changes");
  assert.match(comparison.note, /row visibility/iu);
  assert.match(comparison.note, /column visibility.*exact-copy inspection/iu);
  assert.deepEqual(
    comparison.hunks[0].lines
      .filter((line) => line.kind !== "context")
      .map(({ kind, text }) => ({ kind, text })),
    [{ kind: "added", text: "<row 2 visibility: hidden>" }],
  );
});

test("default-hidden worksheet rows remain an explicit workbook structure change", async () => {
  const { artifactContentComparison } = await loadArtifactContentDiff();
  const fixture = comparisonFixture(cases[0], { truncated: false });
  const preview = Object.freeze({
    ...fixture.preview,
    after: Object.freeze({
      ...fixture.preview.after,
      textLines: Object.freeze([
        "<default row visibility: hidden>",
        "<row 2 visibility: visible>",
        "Retained extracted content",
      ]),
      textSections: Object.freeze([Object.freeze({
        label: cases[0].section,
        lineStart: 0,
        lineCount: 3,
      })]),
    }),
  });

  const comparison = artifactContentComparison(fixture.change, preview);
  assert.ok(comparison);
  assert.equal(comparison.title, "Workbook content and structure changes");
  assert.match(comparison.note, /explicit and default row visibility/iu);
  assert.deepEqual(
    comparison.hunks[0].lines
      .filter((line) => line.kind !== "context")
      .map(({ kind, text }) => ({ kind, text })),
    [
      { kind: "added", text: "<default row visibility: hidden>" },
      { kind: "added", text: "<row 2 visibility: visible>" },
    ],
  );
});

test("a changed merged-cell range remains an explicit workbook structure change", async () => {
  const { artifactContentComparison } = await loadArtifactContentDiff();
  const fixture = comparisonFixture(cases[0], { truncated: false });
  const preview = Object.freeze({
    ...fixture.preview,
    before: Object.freeze({
      ...fixture.preview.before,
      textLines: Object.freeze(["A1\ttext\tQuarter", "<merged cells: A1:B1>"]),
      textSections: Object.freeze([Object.freeze({
        label: cases[0].section,
        lineStart: 0,
        lineCount: 2,
      })]),
    }),
    after: Object.freeze({
      ...fixture.preview.after,
      textLines: Object.freeze(["A1\ttext\tQuarter", "<merged cells: A1:C1>"]),
      textSections: Object.freeze([Object.freeze({
        label: cases[0].section,
        lineStart: 0,
        lineCount: 2,
      })]),
    }),
  });

  const comparison = artifactContentComparison(fixture.change, preview);
  assert.ok(comparison);
  assert.equal(comparison.title, "Workbook content and structure changes");
  assert.match(comparison.note, /merged ranges/iu);
  assert.deepEqual(
    comparison.hunks[0].lines
      .filter((line) => line.kind !== "context")
      .map(({ kind, text }) => ({ kind, text })),
    [
      { kind: "removed", text: "<merged cells: A1:B1>" },
      { kind: "added", text: "<merged cells: A1:C1>" },
    ],
  );
});

test("workbook control whitespace and literal escapes remain visible changes", async () => {
  const { artifactContentComparison } = await loadArtifactContentDiff();
  const fixture = comparisonFixture(cases[0], { truncated: false });
  const controlWhitespace = `A1\ttext\t${String.raw`Forecast\tQ1\rQ2\nQ3`}`;
  const literalEscapes = `A1\ttext\t${String.raw`Forecast\\tQ1\\rQ2\\nQ3`}`;
  const preview = Object.freeze({
    ...fixture.preview,
    before: Object.freeze({
      ...fixture.preview.before,
      textLines: Object.freeze([controlWhitespace]),
    }),
    after: Object.freeze({
      ...fixture.preview.after,
      textLines: Object.freeze([literalEscapes]),
    }),
  });

  const comparison = artifactContentComparison(fixture.change, preview);
  assert.ok(comparison);
  assert.equal(comparison.title, "Workbook content and structure changes");
  assert.deepEqual(
    comparison.hunks[0].lines
      .filter((line) => line.kind !== "context")
      .map(({ kind, text }) => ({ kind, text })),
    [
      { kind: "removed", text: controlWhitespace },
      { kind: "added", text: literalEscapes },
    ],
  );
});

test("formula result presence cannot be hidden by marker-like cached text", async () => {
  const { artifactContentComparison } = await loadArtifactContentDiff();
  const fixture = comparisonFixture(cases[0], { truncated: false });
  const missingResult = "A1\tformula\texpression\tpresent\tSUM(B1:B2)\tattributes\t0\tresult\tmissing";
  const cachedMarker = "A1\tformula\texpression\tpresent\tSUM(B1:B2)\tattributes\t0\tresult\tcached\ttext\t<not cached>";
  const preview = Object.freeze({
    ...fixture.preview,
    before: Object.freeze({
      ...fixture.preview.before,
      textLines: Object.freeze([missingResult]),
    }),
    after: Object.freeze({
      ...fixture.preview.after,
      textLines: Object.freeze([cachedMarker]),
    }),
  });

  const comparison = artifactContentComparison(fixture.change, preview);
  assert.ok(comparison);
  assert.equal(comparison.title, "Workbook content and structure changes");
  assert.deepEqual(
    comparison.hunks[0].lines
      .filter((line) => line.kind !== "context")
      .map(({ kind, text }) => ({ kind, text })),
    [
      { kind: "removed", text: missingResult },
      { kind: "added", text: cachedMarker },
    ],
  );
});

test("an explicit empty formula result remains different from an absent result", async () => {
  const { artifactContentComparison } = await loadArtifactContentDiff();
  const fixture = comparisonFixture(cases[0], { truncated: false });
  const missingResult = "A1\tformula\texpression\tpresent\tSUM(B1:B2)\tattributes\t0\tresult\tmissing";
  const cachedEmpty = "A1\tformula\texpression\tpresent\tSUM(B1:B2)\tattributes\t0\tresult\tcached\ttext\t";
  const preview = Object.freeze({
    ...fixture.preview,
    before: Object.freeze({ ...fixture.preview.before, textLines: Object.freeze([missingResult]) }),
    after: Object.freeze({ ...fixture.preview.after, textLines: Object.freeze([cachedEmpty]) }),
  });

  const comparison = artifactContentComparison(fixture.change, preview);
  assert.ok(comparison);
  assert.deepEqual(
    comparison.hunks[0].lines
      .filter((line) => line.kind !== "context")
      .map(({ kind, text }) => ({ kind, text })),
    [
      { kind: "removed", text: missingResult },
      { kind: "added", text: cachedEmpty },
    ],
  );
});

test("an explicit empty workbook cell remains different from no cell", async () => {
  const { artifactContentComparison } = await loadArtifactContentDiff();
  const fixture = comparisonFixture(cases[0], { truncated: false });
  const preview = Object.freeze({
    ...fixture.preview,
    before: Object.freeze({
      ...fixture.preview.before,
      textLines: Object.freeze(["<no cells or formulas>"]),
    }),
    after: Object.freeze({
      ...fixture.preview.after,
      textLines: Object.freeze(["A1\ttext\tvalue\tempty"]),
    }),
  });

  const comparison = artifactContentComparison(fixture.change, preview);
  assert.ok(comparison);
  assert.equal(comparison.title, "Workbook content and structure changes");
  assert.deepEqual(
    comparison.hunks[0].lines
      .filter((line) => line.kind !== "context")
      .map(({ kind, text }) => ({ kind, text })),
    [
      { kind: "removed", text: "<no cells or formulas>" },
      { kind: "added", text: "A1\ttext\tvalue\tempty" },
    ],
  );
});

test("an empty worksheet rename remains visible and independently filterable", async () => {
  const { artifactContentComparison, artifactSectionHunks } = await loadArtifactContentDiff();
  const fixture = comparisonFixture(cases[0], { truncated: false });
  const emptySide = (sideName, section) => Object.freeze({
    ...fixture.preview[sideName],
    textLines: Object.freeze(["<no cells or formulas>"]),
    textSections: Object.freeze([Object.freeze({
      label: section,
      lineStart: 0,
      lineCount: 1,
    })]),
  });
  const preview = Object.freeze({
    ...fixture.preview,
    before: emptySide("before", "Old planning"),
    after: emptySide("after", "New planning"),
  });

  const comparison = artifactContentComparison(fixture.change, preview);
  assert.ok(comparison);
  assert.equal(comparison.title, "Workbook content and structure changes");
  assert.deepEqual(comparison.sectionLabels, ["Old planning", "New planning"]);
  assert.deepEqual(
    artifactSectionHunks(comparison, "New planning")[0].lines.map(({ kind, text, section }) => ({ kind, text, section })),
    [
      { kind: "added", text: "New planning", section: true },
      { kind: "added", text: "<no cells or formulas>", section: false },
    ],
  );
});

test("a moved worksheet filter includes both its earlier and current positions", async () => {
  const { artifactContentComparison, artifactSectionHunks } = await loadArtifactContentDiff();
  const fixture = comparisonFixture(cases[0], { truncated: false });
  const workbookSide = (sideName, sections) => Object.freeze({
    ...fixture.preview[sideName],
    textLines: Object.freeze(sections.map((section) => `${section} retained value`)),
    textSections: Object.freeze(sections.map((section, index) => Object.freeze({
      label: section,
      lineStart: index,
      lineCount: 1,
    }))),
  });
  const preview = Object.freeze({
    ...fixture.preview,
    before: workbookSide("before", ["Planning", "Summary"]),
    after: workbookSide("after", ["Summary", "Planning"]),
  });

  const comparison = artifactContentComparison(fixture.change, preview);
  assert.ok(comparison);
  const planning = artifactSectionHunks(comparison, "Planning");
  assert.equal(planning.length, 2);
  assert.deepEqual(
    planning.map((hunk) => hunk.lines.map(({ kind, text, section }) => ({ kind, text, section }))),
    [
      [
        { kind: "removed", text: "Planning", section: true },
        { kind: "removed", text: "Planning retained value", section: false },
      ],
      [
        { kind: "added", text: "Planning", section: true },
        { kind: "added", text: "Planning retained value", section: false },
      ],
    ],
  );
});

test("found differences remain explicit when the extraction is also bounded", async () => {
  const { artifactContentComparison } = await loadArtifactContentDiff();
  const fixture = comparisonFixture(cases[0], {
    truncated: true,
    afterLine: "Changed retained content",
  });
  const comparison = artifactContentComparison(fixture.change, fixture.preview);
  assert.ok(comparison);
  assert.equal(comparison.title, "Workbook content and structure changes");
  assert.match(comparison.note, /bounded limit/iu);
  assert.equal(comparison.hunks[0].lines.some((line) => line.kind !== "context"), true);
});
