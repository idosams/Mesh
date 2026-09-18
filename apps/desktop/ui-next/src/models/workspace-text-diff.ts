export const WORKSPACE_TEXT_DIFF_MAX_LINES = 4_096;
export const WORKSPACE_TEXT_DIFF_MAX_CHARACTERS = 262_144;
const WORKSPACE_TEXT_DIFF_CONTEXT_LINES = 3;

export type WorkspaceTextDiffLine = Readonly<{
  kind: "context" | "removed" | "added";
  before: number | null;
  after: number | null;
  text: string;
  ending: "lf" | "crlf" | "none";
}>;

export type WorkspaceTextDiffHunk = Readonly<{
  beforeStart: number;
  beforeCount: number;
  afterStart: number;
  afterCount: number;
  lines: readonly WorkspaceTextDiffLine[];
}>;

export type WorkspaceTextDiff = Readonly<{
  kind: "ready" | "unchanged" | "unavailable";
  additions: number;
  deletions: number;
  hunks: readonly WorkspaceTextDiffHunk[];
  reason: string | null;
}>;

export type WorkspaceSplitDiffRow = Readonly<{
  before: WorkspaceTextDiffLine | null;
  after: WorkspaceTextDiffLine | null;
}>;

type SourceLine = Readonly<{
  key: string;
  text: string;
  number: number;
  ending: WorkspaceTextDiffLine["ending"];
}>;

function sourceLines(text: string): readonly SourceLine[] {
  if (text.length === 0) return Object.freeze([]);
  const lines: SourceLine[] = [];
  let cursor = 0;
  while (cursor < text.length) {
    const newline = text.indexOf("\n", cursor);
    const terminated = newline !== -1;
    let value = text.slice(cursor, terminated ? newline : text.length);
    const ending = !terminated ? "none" : value.endsWith("\r") ? "crlf" : "lf";
    if (ending === "crlf") value = value.slice(0, -1);
    lines.push(Object.freeze({
      key: `${ending}\u0000${value}`,
      text: value,
      number: lines.length + 1,
      ending,
    }));
    if (!terminated) break;
    cursor = newline + 1;
  }
  return Object.freeze(lines);
}

function diffLines(beforeLines: readonly SourceLine[], afterLines: readonly SourceLine[]): readonly WorkspaceTextDiffLine[] {
  const width = afterLines.length + 1;
  const matrix = new Uint16Array((beforeLines.length + 1) * width);
  const at = (before: number, after: number) => before * width + after;
  for (let before = beforeLines.length - 1; before >= 0; before -= 1) {
    for (let after = afterLines.length - 1; after >= 0; after -= 1) {
      matrix[at(before, after)] = beforeLines[before].key === afterLines[after].key
        ? matrix[at(before + 1, after + 1)] + 1
        : Math.max(matrix[at(before + 1, after)], matrix[at(before, after + 1)]);
    }
  }
  const result: WorkspaceTextDiffLine[] = [];
  let before = 0;
  let after = 0;
  while (before < beforeLines.length || after < afterLines.length) {
    const earlier = beforeLines[before];
    const current = afterLines[after];
    if (earlier && current && earlier.key === current.key) {
      result.push(Object.freeze({
        kind: "context",
        before: earlier.number,
        after: current.number,
        text: earlier.text,
        ending: earlier.ending,
      }));
      before += 1;
      after += 1;
    } else if (earlier && (!current || matrix[at(before + 1, after)] >= matrix[at(before, after + 1)])) {
      result.push(Object.freeze({
        kind: "removed",
        before: earlier.number,
        after: null,
        text: earlier.text,
        ending: earlier.ending,
      }));
      before += 1;
    } else if (current) {
      result.push(Object.freeze({
        kind: "added",
        before: null,
        after: current.number,
        text: current.text,
        ending: current.ending,
      }));
      after += 1;
    }
  }
  return Object.freeze(result);
}

function diffHunks(lines: readonly WorkspaceTextDiffLine[]): readonly WorkspaceTextDiffHunk[] {
  const changed = lines.flatMap((line, index) => line.kind === "context" ? [] : [index]);
  if (changed.length === 0) return Object.freeze([]);
  const ranges: Array<{ start: number; end: number }> = [];
  for (const index of changed) {
    const start = Math.max(0, index - WORKSPACE_TEXT_DIFF_CONTEXT_LINES);
    const end = Math.min(lines.length, index + WORKSPACE_TEXT_DIFF_CONTEXT_LINES + 1);
    const previous = ranges.at(-1);
    if (previous && start <= previous.end) previous.end = Math.max(previous.end, end);
    else ranges.push({ start, end });
  }
  return Object.freeze(ranges.map(({ start, end }) => {
    const hunkLines = Object.freeze(lines.slice(start, end));
    const beforeSeen = lines.slice(0, start).filter((line) => line.before !== null).length;
    const afterSeen = lines.slice(0, start).filter((line) => line.after !== null).length;
    const beforeCount = hunkLines.filter((line) => line.before !== null).length;
    const afterCount = hunkLines.filter((line) => line.after !== null).length;
    return Object.freeze({
      beforeStart: beforeCount === 0 ? beforeSeen : beforeSeen + 1,
      beforeCount,
      afterStart: afterCount === 0 ? afterSeen : afterSeen + 1,
      afterCount,
      lines: hunkLines,
    });
  }));
}

export function workspaceTextDiff(before: string, after: string): WorkspaceTextDiff {
  if (before === after) {
    return Object.freeze({ kind: "unchanged", additions: 0, deletions: 0, hunks: Object.freeze([]), reason: null });
  }
  if (before.length + after.length > WORKSPACE_TEXT_DIFF_MAX_CHARACTERS) {
    return Object.freeze({
      kind: "unavailable",
      additions: 0,
      deletions: 0,
      hunks: Object.freeze([]),
      reason: "The before and current text exceed the 256 KB comparison display limit.",
    });
  }
  const beforeLines = sourceLines(before);
  const afterLines = sourceLines(after);
  if (beforeLines.length + afterLines.length > WORKSPACE_TEXT_DIFF_MAX_LINES) {
    return Object.freeze({
      kind: "unavailable",
      additions: 0,
      deletions: 0,
      hunks: Object.freeze([]),
      reason: "The before and current text exceed the 4,096-line comparison display limit.",
    });
  }
  const lines = diffLines(beforeLines, afterLines);
  const additions = lines.filter((line) => line.kind === "added").length;
  const deletions = lines.filter((line) => line.kind === "removed").length;
  return Object.freeze({
    kind: additions || deletions ? "ready" : "unchanged",
    additions,
    deletions,
    hunks: diffHunks(lines),
    reason: null,
  });
}

export function workspaceSplitDiffRows(lines: readonly WorkspaceTextDiffLine[]): readonly WorkspaceSplitDiffRow[] {
  const rows: WorkspaceSplitDiffRow[] = [];
  let index = 0;
  while (index < lines.length) {
    const line = lines[index];
    if (line.kind === "context") {
      rows.push(Object.freeze({ before: line, after: line }));
      index += 1;
      continue;
    }
    const removed: WorkspaceTextDiffLine[] = [];
    const added: WorkspaceTextDiffLine[] = [];
    while (index < lines.length && lines[index].kind !== "context") {
      if (lines[index].kind === "removed") removed.push(lines[index]);
      else added.push(lines[index]);
      index += 1;
    }
    for (let offset = 0; offset < Math.max(removed.length, added.length); offset += 1) {
      rows.push(Object.freeze({ before: removed[offset] ?? null, after: added[offset] ?? null }));
    }
  }
  return Object.freeze(rows);
}
