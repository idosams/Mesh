export function workspaceEditorBaselineText(editor) {
  if (!editor || typeof editor.text !== 'string') return '';
  if (editor.native_untracked === true) return '';
  if (Object.hasOwn(editor, 'baseline_text')) {
    return typeof editor.baseline_text === 'string' ? editor.baseline_text : '';
  }
  return editor.text;
}

export function workspaceEditorBaselineAvailable(editor) {
  if (!editor || typeof editor.text !== 'string') return false;
  if (editor.native_untracked === true || !Object.hasOwn(editor, 'baseline_text')) return true;
  return typeof editor.baseline_text === 'string';
}
