/**
 * The lint engine: read a file, extract its user-facing strings, run the rules
 * it opted into, and account for every suppression.
 */

import fs from 'node:fs';
import path from 'node:path';
import { extractSegments, findAllowRegions } from './extract.mjs';
import { ruleById } from './rules/index.mjs';

export const ALLOW_REGION_RULE = 'allow-region';

/**
 * @returns {{ findings: object[], allowRegions: object[], path: string }}
 */
export function lintFile({ root, toolRoot, relPath, mode, rules, allowRegionBudget, statusPath }) {
  const absolute = path.join(root, relPath);
  let text;
  try {
    text = fs.readFileSync(absolute, 'utf8');
  } catch (error) {
    throw new Error(`cannot read ${relPath}: ${error.message}`);
  }
  const extension = path.extname(relPath);
  const findings = [];
  const { regions, errors } = mode === 'prose' ? findAllowRegions(text) : { regions: [], errors: [] };

  for (const error of errors) {
    findings.push({
      rule: ALLOW_REGION_RULE,
      path: relPath,
      line: error.line,
      column: 1,
      message: error.message,
      excerpt: '',
    });
  }
  if (regions.length > allowRegionBudget) {
    findings.push({
      rule: ALLOW_REGION_RULE,
      path: relPath,
      line: regions[allowRegionBudget].startLine,
      column: 1,
      message: `${regions.length} allow regions, budget is ${allowRegionBudget} — raise the budget in a reviewed change or remove the suppression`,
      excerpt: regions[allowRegionBudget].reason,
    });
  }

  const segments = extractSegments(text, mode, extension);
  for (const ruleId of rules) {
    const rule = ruleById(ruleId);
    if (!rule) throw new Error(`unknown rule "${ruleId}"`);
    if (!rule.modes.includes(mode)) continue;
    findings.push(
      ...rule.run({ path: relPath, text, mode, segments, allowRegions: regions, toolRoot, statusPath }),
    );
  }
  return { path: relPath, findings, allowRegions: regions };
}

/** Infer a mode from a file extension, for ad-hoc `lint.mjs <file>` runs. */
export function inferMode(relPath) {
  const extension = path.extname(relPath);
  if (extension === '.md' || extension === '.txt') return 'prose';
  if (extension === '.json') return 'json-strings';
  return 'source-strings';
}
