/** Human and machine renderings of a lint run. */

/**
 * @param {{ files: object[], selfTest: object|null }} run
 * @returns {string}
 */
export function renderHuman(run) {
  const lines = [];
  const findings = run.files.flatMap((file) => file.findings);
  const suppressions = run.files.flatMap((file) =>
    file.allowRegions.map((region) => ({ path: file.path, ...region })),
  );

  if (run.selfTest) {
    lines.push(renderSelfTest(run.selfTest), '');
  }

  for (const file of run.files) {
    if (file.findings.length === 0) continue;
    lines.push(file.path);
    for (const finding of file.findings) {
      lines.push(`  ${file.path}:${finding.line}:${finding.column}  [${finding.rule}] ${finding.message}`);
      if (finding.excerpt) lines.push(`      ${finding.excerpt}`);
    }
    lines.push('');
  }

  if (suppressions.length > 0) {
    lines.push('Suppressions in force (every one is reviewed, none is silent):');
    for (const region of suppressions) {
      lines.push(`  ${region.path}:${region.startLine}-${region.endLine}  ${region.reason}`);
    }
    lines.push('');
  }

  const scanned = run.files.length;
  lines.push(
    findings.length === 0
      ? `vocab-lint: clean — ${scanned} user-facing ${scanned === 1 ? 'surface' : 'surfaces'} scanned, ${suppressions.length} reviewed suppression${suppressions.length === 1 ? '' : 's'}`
      : `vocab-lint: ${findings.length} finding${findings.length === 1 ? '' : 's'} across ${scanned} scanned ${scanned === 1 ? 'surface' : 'surfaces'}`,
  );
  return lines.join('\n');
}

function renderSelfTest(selfTest) {
  const head = selfTest.ok
    ? `vocab-lint self-test: ${selfTest.passed}/${selfTest.total} fixtures pass; all ${selfTest.termsCovered} forbidden terms fire`
    : `vocab-lint self-test: FAILED (${selfTest.failures.length} of ${selfTest.total})`;
  if (selfTest.ok) return head;
  return [head, ...selfTest.failures.map((failure) => `  ${failure.fixture}: ${failure.message}`)].join('\n');
}

/** @returns {string} */
export function renderJson(run) {
  return JSON.stringify(
    {
      ok: run.files.every((file) => file.findings.length === 0) && (run.selfTest?.ok ?? true),
      scanned: run.files.map((file) => file.path),
      findings: run.files.flatMap((file) => file.findings),
      suppressions: run.files.flatMap((file) =>
        file.allowRegions.map((region) => ({ path: file.path, ...region })),
      ),
      selfTest: run.selfTest,
    },
    null,
    2,
  );
}
