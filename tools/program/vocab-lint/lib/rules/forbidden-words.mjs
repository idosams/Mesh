/**
 * Rule `forbidden-words` — the nine terms from charter §5 must not appear in a
 * user-facing string.
 *
 * In `prose` mode an explicit, reasoned allow region can exempt a passage that
 * must name a term in order to forbid it. In every other mode — shipped strings
 * — there is no exemption at all: a suppression directive found in a string
 * catalogue is itself a finding.
 */

import { FORBIDDEN_TERMS, matcherFor } from '../vocabulary.mjs';
import { isAllowed } from '../extract.mjs';
import { lineStarts, positionAt, lineText } from '../positions.mjs';

export const id = 'forbidden-words';
export const modes = ['prose', 'json-strings', 'status-catalog', 'source-strings'];

export function run(context) {
  return context.mode === 'prose' ? runProse(context) : runStrings(context);
}

function runProse({ path, text, allowRegions }) {
  const starts = lineStarts(text);
  const findings = [];
  for (const term of FORBIDDEN_TERMS) {
    const matcher = matcherFor(term);
    let match = matcher.exec(text);
    while (match !== null) {
      const { line, column } = positionAt(starts, match.index);
      if (!isAllowed(allowRegions, line)) {
        findings.push(finding(path, line, column, term, match[0], lineText(text, starts, line)));
      }
      match = matcher.exec(text);
    }
  }
  return findings.sort(byPosition);
}

function runStrings({ path, text, segments }) {
  const findings = [];
  if (/vocab-lint:allow/.test(text)) {
    findings.push({
      rule: id,
      path,
      line: 1,
      column: 1,
      message:
        'vocab-lint:allow is not honoured on a shipped string surface — a user-facing string has no exemption',
      excerpt: 'vocab-lint:allow',
    });
  }
  for (const segment of segments) {
    for (const term of FORBIDDEN_TERMS) {
      const matcher = matcherFor(term);
      const match = matcher.exec(segment.text);
      if (match !== null) {
        findings.push(
          finding(path, segment.line, segment.column, term, match[0], segment.text.trim()),
        );
      }
    }
  }
  return findings.sort(byPosition);
}

function finding(path, line, column, term, matched, excerpt) {
  return {
    rule: id,
    termId: term.id,
    path,
    line,
    column,
    message: `"${matched}" is not a product word (${term.term}) — say "${term.sayInstead}" instead`,
    excerpt: excerpt.length > 160 ? `${excerpt.slice(0, 157)}…` : excerpt,
  };
}

function byPosition(a, b) {
  return a.line - b.line || a.column - b.column || a.termId.localeCompare(b.termId);
}
