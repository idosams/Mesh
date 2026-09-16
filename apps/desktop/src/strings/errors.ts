// Every sentence this application shows when something has gone wrong and a person has to read
// about it.
//
// Error text is copy. It was the half of the vocabulary rule that used to be missing here: the
// labels lived under `src/strings/` and were linted, while the sentences a person only ever meets
// on a bad day were written inline at the `throw` site, where the gate never saw them. Every
// product with this ambition has leaked its internals through an error message eventually, so the
// messages moved to where the lint already looks:
//
//   node tools/program/vocab-lint/lint.mjs --user-facing
//
// `tools/program/vocab-lint/surfaces.json` scans every shipped TypeScript source in this
// application with an exemption budget of zero, and `src/app/copy.test.ts` walks these sentences
// as well as the labels, so a term from the internal model fails the desktop build in two places.
//
// Each entry is a function rather than a string because these sentences name the thing that
// failed. What may be interpolated is bounded on purpose: an identifier this application defines,
// never a value that arrived from somewhere else. `src/app/status.ts` is where that rule is
// visible — an internal state it cannot map is carried on a field of the thrown error, not
// pasted into the sentence below, because a word this application has never seen is exactly the
// word the lint cannot check.

/** The words a person reads for each fault this application can raise. */
export const ERROR_COPY = {
  /** The interface asked for an operation that is not in its own catalogue. */
  unknownOperation: (id: string): string =>
    `\`${id}\` is not an operation this application offers. Nothing was sent and nothing was changed.`,

  /** The operation exists, but names a method the Mesh service surface does not have. */
  methodNotOnSurface: (id: string, method: string): string =>
    `\`${id}\` names \`${method}\`, which is not on the Mesh service surface. Nothing was sent and nothing was changed.`,

  /** The operation needs one thing from the person and did not get it. */
  argumentMissing: (id: string, what: string): string =>
    `\`${id}\` needs ${what} and did not get it. Nothing was sent and nothing was changed.`,

  /**
   * Mesh knows of a state it has no words for.
   *
   * It says nothing about which one. Naming it here would put an unreviewed word in front of a
   * person — the one thing this file exists to prevent — so the name rides on the error object
   * for whoever is reading a report, and the sentence stays inside the vocabulary.
   */
  unmappedInternalState: (): string =>
    'Mesh has no words for the state of this work and will not guess at it. This is a fault in the app rather than anything wrong with your work; nothing was changed. Please report it.',

  /**
   * The background service answered something this app cannot read.
   *
   * Which field is missing rides on the error object, for the same reason an unmapped state does:
   * a field name from a service this app has never met is exactly the word the vocabulary lint
   * cannot check, so it never reaches the sentence.
   */
  unreadableService: (): string =>
    'Mesh could not make sense of what the background service answered, so this window is showing the last reading it understood. Nothing was changed. Updating both the app and the service usually fixes this.',
} as const;

/** The keys of {@link ERROR_COPY}, so a test can walk every sentence. */
export type ErrorCopyKey = keyof typeof ERROR_COPY;
