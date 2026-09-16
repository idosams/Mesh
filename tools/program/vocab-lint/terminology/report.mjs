/** Rendering for the terminology lint. Human output first; `--json` for CI. */

import { CHECKS } from './checks.mjs';

export function renderHuman(findings, model) {
  const lines = [];
  if (findings.length === 0) {
    lines.push('terminology: clean');
    lines.push(
      `  ${model.register.length} terms · ${model.aliases.length} aliases · ` +
        `${model.members.length} member values · ${model.crates.length} crates · ` +
        `${CHECKS.length} checks`,
    );
    return lines.join('\n');
  }
  for (const item of findings) {
    const at = item.line > 0 ? `${item.doc}:${item.line}` : item.doc;
    lines.push(`${at}: ${item.id} ${item.message}`);
  }
  lines.push('');
  lines.push(`terminology: ${findings.length} finding${findings.length === 1 ? '' : 's'}`);
  return lines.join('\n');
}

export function renderJson(findings, model) {
  return JSON.stringify(
    {
      ok: findings.length === 0,
      counts: {
        terms: model.register.length,
        aliases: model.aliases.length,
        memberValues: model.members.length,
        crates: model.crates.length,
        publicItems: model.publicItems.length,
        checks: CHECKS.length,
      },
      findings,
    },
    null,
    2,
  );
}
