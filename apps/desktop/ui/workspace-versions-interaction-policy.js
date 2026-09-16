export function workspaceVersionsRetainedInteraction(actionKey) {
  return actionKey === 'set-custom-location'
    || (typeof actionKey === 'string' && actionKey.startsWith('select-version:'));
}
