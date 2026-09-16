// The label and the description of every operation this application offers a person.
//
// Under `src/strings/` because `tools/program/vocab-lint/surfaces.json` lints this directory as
// user-facing copy with an exemption budget of zero. The catalogue itself — which operation calls
// which method, at which version — is `src/app/operations.ts`; only the words are here, so that
// the wiring can change without touching linted copy and the copy can change without touching
// wiring.

/** What a person reads for one operation. */
export type OperationCopy = {
  /** The label on the control. */
  readonly label: string;
  /** One sentence describing what it does. */
  readonly description: string;
};

/** Copy for every operation in `src/app/operations.ts`, by operation identifier. */
export const OPERATION_COPY: Record<string, OperationCopy> = {
  'service.check': {
    label: 'Check Mesh',
    description: 'Ask the Mesh background service whether it is running and ready.',
  },
  'service.startup': {
    label: 'Last start-up',
    description: 'See what Mesh found about your workspace the last time it started.',
  },
  'service.describe': {
    label: 'What this version offers',
    description: 'List the operations the running Mesh background service can carry out.',
  },
  'workspace.open': {
    label: 'Open a folder',
    description: 'Open a folder as a workspace and read back the work already saved in it.',
  },
  'workspace.show': {
    label: 'What is in this workspace',
    description: 'See how much work is saved in the open workspace, and what Mesh cannot show yet.',
  },
  'review.open': {
    label: 'Open a review',
    description: 'Open an exact review bundle for one saved change.',
  },
  'review.open-current': {
    label: 'Review current version',
    description: 'Compute and open an exact review of the current saved workspace.',
  },
  'review.approve': {
    label: 'Share approved change',
    description: 'Unavailable until this device can prove a human-held approval authority.',
  },
  'workspace.follow': {
    label: 'Keep this up to date',
    description: 'Have Mesh tell this window what it is doing, as it happens.',
  },
  'folder.import.preview': {
    label: 'Preview folder',
    description: 'Read and verify the selected folder before creating a managed copy.',
  },
  'folder.import.confirm': {
    label: 'Create private workspace',
    description: 'Verify the accepted summary, create durable private history, and open the managed copy.',
  },
  'folder.import.rollback': {
    label: 'Roll back managed copy',
    description: 'Remove an unchanged managed copy while preserving the original folder.',
  },
  'workspace.restore.preview': {
    label: 'Preview earlier version',
    description: 'Check the exact earlier-version plan without changing the workspace.',
  },
  'workspace.version.fork': {
    label: 'Open saved workspace',
    description: 'Reconstruct a saved workspace as a new independent native folder.',
  },
  'performance.counters': {
    label: 'Performance counters',
    description: 'Read the live measurements and the conditions needed to interpret them.',
  },
};
