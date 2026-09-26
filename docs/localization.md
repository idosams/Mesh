# Desktop language support

The desktop alpha offers English and Hebrew. English is the default. Choose **עברית** in the
language selector beside the page navigation. Choose **English** to switch back. The preference
is saved locally under `mesh.ui.locale.v1`; if browser storage is unavailable it lasts for the
current session. Language changes update existing components without remounting the workspace
or clearing input drafts. No workspace records, file contents, paths, intent identifiers, or
native authorization decisions change with the language.

Hebrew covers the primary navigation, folder onboarding and import, empty-folder refusal,
file browsing and actions, saving controls, versions, restoration, destination controls,
and the main destructive confirmation consequences (file deletion, private-copy rollback,
folder creation, destination replacement/removal, and finishing or reopening an agent handoff).
The interface reads right to left; file paths, code and technical values retain left-to-right
ordering. Captured values in translated confirmation templates are directionally isolated and
are not normalized or changed. Native macOS dialogs use the operating system's language.

## Current limits

This is initial Hebrew support, not a claim that every message is translated. Some advanced
review/rendering explanations, dynamic progress/count summaries, detailed destination plans,
legacy browser-confirm fallbacks, and less common recovery/approval explanations remain in
English. Unknown safety text is shown verbatim rather than guessed. Native diagnostic errors
retain the exact English diagnostic with a Hebrew explanation; known empty-import and
empty-saved-version refusals have specific Hebrew guidance. Source code, filenames and file
contents are never machine-translated. The repository's full technical documentation remains
English; the release includes a Hebrew first-session guide.

## Maintenance and validation

Visible interface copy uses explicit `useTranslation` calls and a reviewed Hebrew catalog in
`apps/desktop/ui-next/src/lib/hebrew.ts`. Exact dynamic safety templates live in
`hebrew-safety.ts`. Add translations at presentation boundaries, not to daemon data or protocol
values. Do not translate a generic filename, user document, or arbitrary model object. New or
changed English safety templates need matching Hebrew review and tests; unmatched text falls
back to its original English.

`npm run verify:desktop` covers the catalog behavior, primary navigation and accessible labels,
locale persistence, denied storage, direction changes, unchanged paths and content, and exact
safety consequences. The packaged Files journey switches to Hebrew and back to English;
its release evidence is recorded in the release validation report. That check does not claim
complete visual or linguistic review of every screen.
