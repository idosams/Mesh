// protocol/conformance/lib/report.mjs — the report format, and why it has three results
// (task 01KZC2P6SQ7E00ENWQ6F4YVXJJ).
//
// `pass` and `fail` are the obvious two. The third, `unsupported`, exists because a partial
// implementation reporting honestly is not a broken one, and a suite that cannot express the
// difference pushes implementers towards the one answer that hides it: a stub that returns
// something. `unsupported` is a first-class result, it is never counted as a failure, and it is
// never counted as a pass either.
//
// `unsupported` then splits again, and this split is the part that keeps the suite honest about
// the SPECIFICATION rather than only about the client:
//
//  - `client` — this client does not implement it. Somebody else's might.
//  - `specification` — nothing could implement it from the published material, because the
//    published material does not say how. `ID-*` is there today: `record_id_hex` cannot be
//    recomputed from `protocol/**` at all (01KZCZDTVD0D36W5YRGX8CNE17). So is every `PUB-*` case.
//
// A suite that let a specification gap read as a client gap would report a complete-looking pass
// for a protocol nobody can fully implement. The gap is printed on every run, at the end, whether
// or not anything failed.

export const PASS = "pass";
export const FAIL = "fail";
export const UNSUPPORTED = "unsupported";

export function summarise(results) {
  const counts = { pass: 0, fail: 0, unsupported: 0 };
  for (const result of results) counts[result.result] += 1;

  const gaps = results.filter(
    (result) => result.result === UNSUPPORTED && result.unsupported_because === "specification",
  );
  const answeredGaps = results.filter((result) => result.result === PASS && result.spec_gap);

  const families = {};
  for (const result of results) {
    const family = (families[result.family] ??= { pass: 0, fail: 0, unsupported: 0 });
    family[result.result] += 1;
  }

  return { counts, families, specification_gaps: gaps, answered_specification_gaps: answeredGaps };
}

export function buildReport({ client, hello, results, startedAt, finishedAt }) {
  const summary = summarise(results);
  return {
    report_format: "cwp-conformance-report/0",
    protocol: "CWP v0",
    client: {
      name: client.name,
      command: client.command,
      declared: hello,
    },
    duration_ms: finishedAt - startedAt,
    summary: {
      total: results.length,
      ...summary.counts,
      specification_gaps: summary.specification_gaps.length,
      answered_specification_gaps: summary.answered_specification_gaps.length,
      by_family: summary.families,
    },
    verdict: summary.counts.fail === 0 ? "conformant-so-far" : "non-conformant",
    specification_gaps: summary.specification_gaps.map((result) => ({
      case: result.id,
      rule: result.rule,
      citation: result.citation,
      tracking: result.spec_gap?.tracking ?? null,
      question: result.spec_gap?.question ?? null,
    })),
    cases: results,
  };
}

const symbol = { pass: "pass", fail: "FAIL", unsupported: "unsup" };

export function renderText(report, { verbose = false } = {}) {
  const lines = [];
  lines.push(`CWP conformance — ${report.client.name}`);
  lines.push(`  adapter: ${report.client.command}`);
  if (report.client.declared?.description) {
    lines.push(`  client:  ${report.client.declared.description}`);
  }
  lines.push("");

  for (const result of report.cases) {
    if (result.result === FAIL) {
      lines.push(`FAIL  ${result.id}`);
      lines.push(`      ${result.title}`);
      lines.push(`      rule:  ${result.rule}`);
      lines.push(`      cited: ${result.citation}`);
      for (const line of String(result.detail).split("\n")) lines.push(`      ${line}`);
      lines.push("");
    } else if (verbose) {
      lines.push(`${symbol[result.result]}  ${result.id} — ${result.title}`);
      if (result.result === UNSUPPORTED) lines.push(`      ${result.detail}`);
    }
  }

  const gaps = report.specification_gaps;
  if (gaps.length > 0) {
    lines.push("SPECIFICATION GAPS — cases no client can pass from the published material");
    lines.push("These are not failures of this client. They are unanswered questions in CWP v0.");
    lines.push("");
    const byTracking = new Map();
    for (const gap of gaps) {
      const key = gap.tracking ?? "untracked";
      const bucket = byTracking.get(key) ?? { question: gap.question, cases: [] };
      bucket.cases.push(gap.case);
      byTracking.set(key, bucket);
    }
    for (const [tracking, bucket] of byTracking) {
      lines.push(`  ${tracking === "untracked" ? "(no tracking id)" : tracking} — ${bucket.cases.length} case(s)`);
      for (const line of wrap(bucket.question, 92)) lines.push(`    ${line}`);
      lines.push(`    cases: ${bucket.cases.slice(0, 4).join(", ")}${bucket.cases.length > 4 ? ", …" : ""}`);
      lines.push("");
    }
  }

  if (report.summary.answered_specification_gaps > 0) {
    lines.push(
      `NOTE  ${report.summary.answered_specification_gaps} case(s) marked as specification gaps ` +
        "were answered correctly. That means this client knows something the published material " +
        "does not say — worth publishing, or worth doubting.",
    );
    lines.push("");
  }

  const byFamily = Object.entries(report.summary.by_family)
    .map(([family, counts]) => `${family} ${counts.pass}/${counts.pass + counts.fail}`)
    .join(" · ");
  lines.push(byFamily);
  lines.push(
    `${report.summary.total} cases · ${report.summary.pass} pass · ${report.summary.fail} fail · ` +
      `${report.summary.unsupported} unsupported (${report.summary.specification_gaps} of them ` +
      `specification gaps) · ${report.duration_ms} ms`,
  );
  lines.push(
    report.summary.fail === 0
      ? "VERDICT: no case failed. Unsupported cases are unanswered, not passed."
      : `VERDICT: ${report.summary.fail} case(s) failed.`,
  );
  return lines.join("\n");
}

function wrap(text, width) {
  if (!text) return [];
  const words = String(text).split(/\s+/);
  const lines = [];
  let line = "";
  for (const word of words) {
    if (line.length + word.length + 1 > width) {
      lines.push(line);
      line = word;
    } else {
      line = line ? `${line} ${word}` : word;
    }
  }
  if (line) lines.push(line);
  return lines;
}
