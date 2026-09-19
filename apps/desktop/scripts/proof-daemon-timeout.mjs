const PROJECT_COPY_PROOF_METHODS = new Set([
  'folder.import.confirm',
  'review.open-current',
  'workspace.state',
  'workspace.version.fork',
]);

export function proofDaemonIdleTimeoutMs(method) {
  if (PROJECT_COPY_PROOF_METHODS.has(method)) return 300_000;
  if (method === 'folder.import.preview') return 60_000;
  return 1_000;
}
