# Synthetic native renderer fixtures

These four documents contain invented text and numbers, no customer or workspace data. They are
checked in so macOS integration tests run without an additional document-generation toolchain.
The fixture checks exercise real PDFKit and Quick Look processes plus the bounded content parsers.
They do not launch Office applications or evaluate spreadsheet formulas.

From the repository root on macOS:

```bash
npm run test:macos-renderers
```

- `review.pdf`: three pages with separately asserted text and page order.
- `review.pptx`: two slides with separately asserted content and section identity.
- `review.docx`: a title and paragraph with document-section extraction.
- `review.xlsx`: a Budget sheet with numeric cells and a formula plus an explicit cached result.

The PDF prefix-limit cases construct their own one-page and 65-page files in Rust. Every fixture
assertion uses exact expected text; these integration cases previously retained obsolete PDF and
formula expectations after their fixture/representation formats changed.

To regenerate the Office and three-page PDF examples, use `generate.py` in a development Python
environment with reportlab 4.4.9, python-pptx 1.0.2, python-docx 1.2.0, and XlsxWriter 3.2.9.
These are optional fixture-authoring dependencies, not product or test-runtime dependencies:

```bash
python3 tests/fixtures/artifact-renderers/generate.py /tmp/mesh-new-renderer-fixtures
```

The destination must not exist. Archive timestamps and producer metadata can differ after
regeneration; review the resulting documents before deliberately replacing the checked-in bytes.
For an alternate fixture campaign, set `MESH_TEST_PDF`, `MESH_TEST_PPTX`, `MESH_TEST_DOCX`, and
`MESH_TEST_XLSX` to the generated absolute paths before running the native tests. A missing or
incorrect alternate fixture fails rather than silently falling back.
