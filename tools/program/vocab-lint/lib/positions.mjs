/** Offset-to-line/column arithmetic, shared by every extractor. */

/** Byte-offset of the start of each line, 0-indexed. */
export function lineStarts(text) {
  const starts = [0];
  for (let i = 0; i < text.length; i += 1) {
    if (text[i] === '\n') starts.push(i + 1);
  }
  return starts;
}

/** 1-based line and column for a 0-based character offset. */
export function positionAt(starts, offset) {
  let low = 0;
  let high = starts.length - 1;
  while (low < high) {
    const mid = Math.ceil((low + high) / 2);
    if (starts[mid] <= offset) low = mid;
    else high = mid - 1;
  }
  return { line: low + 1, column: offset - starts[low] + 1 };
}

/** The text of line `line` (1-based), without its terminator. */
export function lineText(text, starts, line) {
  const start = starts[line - 1] ?? 0;
  const end = line < starts.length ? starts[line] - 1 : text.length;
  return text.slice(start, end).replace(/\r$/, '');
}
