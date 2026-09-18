import assert from 'node:assert/strict';
import test from 'node:test';

import {
  workspaceEditorBaselineAvailable,
  workspaceEditorBaselineText,
} from './workspace-editor-baseline.js';

test('tracked working text compares against native-projected durable text', () => {
  assert.equal(workspaceEditorBaselineText({
    text: 'working folder text\n',
    baseline_text: 'saved private text\n',
    native_untracked: false,
  }), 'saved private text\n');
  assert.equal(workspaceEditorBaselineAvailable({
    text: 'working folder text\n',
    baseline_text: 'saved private text\n',
    native_untracked: false,
  }), true);
});

test('new native text compares against an empty baseline', () => {
  assert.equal(workspaceEditorBaselineText({
    text: 'new agent file\n',
    native_untracked: true,
  }), '');
  assert.equal(workspaceEditorBaselineAvailable({
    text: 'new agent file\n',
    native_untracked: true,
  }), true);
});

test('an explicitly unavailable saved baseline never masquerades as unchanged working text', () => {
  const editor = {
    text: 'small working text\n',
    baseline_text: null,
    native_untracked: false,
  };
  assert.equal(workspaceEditorBaselineText(editor), '');
  assert.equal(workspaceEditorBaselineAvailable(editor), false);
});

test('older inspection fixtures fall back to their inspected working text', () => {
  assert.equal(workspaceEditorBaselineText({
    text: 'inspected text\n',
    native_untracked: false,
  }), 'inspected text\n');
  assert.equal(workspaceEditorBaselineAvailable({
    text: 'inspected text\n',
    native_untracked: false,
  }), true);
  assert.equal(workspaceEditorBaselineText(null), '');
  assert.equal(workspaceEditorBaselineAvailable(null), false);
});
