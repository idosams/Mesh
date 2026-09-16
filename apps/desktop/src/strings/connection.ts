// Every sentence this application shows about its link to the Mesh background service.
//
// `tools/program/vocab-lint/surfaces.json` lints every shipped `.ts` source under
// `apps/desktop/src/` as `source-strings` with an exemption budget of zero — this directory
// because copy written inline in a component is copy the gate never sees, and the rest of the
// application because an error message written inline at a `throw` site is the same omission
// wearing a different hat. Run it:
//
//   node tools/program/vocab-lint/lint.mjs --user-facing
//
// The rule these sentences are written to: the internal model is a version history, the user model
// is six words, and nine internal terms may never reach a person — the acyclic graph, the leading
// edge of somebody's work, the per-actor counter set, and the version-control words for a line of
// work, for saving, for moving work onto a newer base, for the pre-review area, for a named
// pointer and for the ordered record of operations. None of them is below.
//
// These sentences are about the LINK, not about anybody's work. Nothing here is one of the six
// status words, because a connection is not a state a piece of work can be in, and borrowing one
// of the six for it would put a seventh meaning on a word that has exactly one.

/** The user-facing sentence for every connection state, plus the two faults a person can see. */
export const CONNECTION_COPY = {
  /** Nothing has been opened yet. */
  idle: 'Mesh is not connected to the background service yet.',
  /** The first connection is being opened. */
  opening: 'Connecting to Mesh…',
  /** Connected and serving. */
  connected: 'Connected to Mesh.',
  /**
   * The link dropped and is being re-established.
   *
   * This sentence carries the promise the reconnection logic actually keeps: nothing you have done
   * is discarded while the link is down.
   */
  reconnecting: 'Reconnecting to Mesh. Your work is safe on this device and nothing is lost while this finishes.',
  /**
   * Nothing is listening, and this window has never reached the service on this endpoint.
   *
   * The same internal state as `reconnecting` — the client is retrying either way — and a
   * DIFFERENT sentence, because they are different situations for the person in front of the
   * screen. "Reconnecting" told somebody whose service was never started that a link they never
   * had was coming back. This one tells them what to do instead. `src/app/window.ts` is the only
   * place that chooses between the two, from whether the window has ever been connected.
   */
  notRunning:
    'The Mesh background service is not running on this device. Start it and this window will connect on its own.',
  /** The two ends cannot agree on a version of the service interface. */
  unusable:
    'This app and the Mesh background service are too far apart in age to talk to each other. Update both to the same release and try again.',
  /** The service answered a different request, session or version than this client offered. */
  protocolMismatch:
    'This app could not verify that the background service answered this exact connection. Restart both from the same Mesh installation and try again.',
  /** The application closed the link on purpose. */
  stopped: 'Mesh is closed. Nothing is running in the background for this window.',
  /** A reply arrived that answers no request this app made. */
  uncorrelated:
    'Mesh received an unexpected answer from the background service and ignored it. Nothing was changed.',
  /** The service offers a method only in a later interface version than the one in use. */
  methodTooNew:
    'This action needs a newer Mesh background service than the one running. Update Mesh and try again.',
  /** A one-time action was sent, but its reply was lost with the connection. */
  outcomeUnknown:
    'Mesh lost the connection after sending this action, so its outcome is unknown. Refresh the shared version before trying again.',
} as const;

/** The keys of {@link CONNECTION_COPY}, so a test can walk every sentence. */
export type ConnectionCopyKey = keyof typeof CONNECTION_COPY;
