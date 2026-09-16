#!/usr/bin/env node
// Folds the spike's JSON Lines into the tables the ADR quotes.
//
//   node benchmarks/workloads/chunk-policy/summarize.mjs results.jsonl
//
// Deliberately dumb: it reads the rows, it does not recompute a byte. Every number it prints is
// present in the input, so a reader can grep for it. Throwaway spike code (task
// 01KZC2E6N03KVPK93EESJ15Z4V); the JSON Lines are the artifact, this is a viewer.

import { readFileSync } from "node:fs";

const rows = readFileSync(process.argv[2] ?? "results.jsonl", "utf8")
  .trim()
  .split("\n")
  .map((line) => JSON.parse(line));

const measured = rows.filter((r) => r.workload !== "push-sweep");
const policies = [...new Set(measured.map((r) => r.policy))];
const segments = [...new Set(measured.map((r) => `${r.workload}|${r.scale}|${r.segment}`))];

const pad = (v, n) => String(v).padStart(n);
const cell = (v, n) => String(v).padEnd(n);

console.log(`segments ${segments.length} · policies ${policies.length} · rows ${measured.length}`);
console.log(
  `round-trip failures ${measured.reduce((s, r) => s + r.roundtrip_failures, 0)} / ` +
    `${measured.reduce((s, r) => s + r.roundtrip_checked, 0)} files checked · ` +
    `byte figures repeatable on all three passes: ${measured.every((r) => r.byte_figures_repeatable)}`
);

// --------------------------------------------------- per segment, every policy
for (const key of segments) {
  const [workload, scale, segment] = key.split("|");
  const here = measured.filter(
    (r) => r.workload === workload && r.scale === scale && r.segment === segment
  );
  const best = Math.min(...here.map((r) => r.transfer_bytes_with_journal));
  const bestStore = Math.min(...here.map((r) => r.store_total_bytes));
  console.log(
    `\n## ${workload} [${scale}] ${segment} — ${here[0].files} files, ${here[0].content_bytes} B, ${here[0].byte_profile}`
  );
  console.log(
    `${cell("policy", 30)} ${pad("store B", 12)} ${pad("amp", 7)} ${pad("edit B", 12)} ${pad("vs best", 9)} ${pad("chunks", 8)} ${pad("MiB/s p50", 10)} ${pad("p99/p50", 8)}`
  );
  for (const r of here) {
    console.log(
      `${cell(r.policy, 30)} ${pad(r.store_total_bytes, 12)} ${pad((r.store_amplification_per_mille / 1000).toFixed(3), 7)} ` +
        `${pad(r.transfer_bytes_with_journal, 12)} ${pad((r.transfer_bytes_with_journal / best).toFixed(2) + "x", 9)} ` +
        `${pad(r.chunk_refs, 8)} ${pad(r.mib_per_s_at_p50 ?? "none", 10)} ${pad((r.ns_p99 / r.ns_p50).toFixed(2), 8)}` +
        (r.store_total_bytes === bestStore ? "  <- smallest store" : "")
    );
  }
}

// ------------------------------------------------------------- policy ranking
console.log("\n\n## Ranking — how often each policy is the cheapest edit, and its worst loss");
const table = policies.map((policy) => {
  let wins = 0;
  let worst = { ratio: 1, segment: "" };
  let transferTotal = 0;
  let storeTotal = 0;
  for (const key of segments) {
    const [workload, scale, segment] = key.split("|");
    const here = measured.filter(
      (r) => r.workload === workload && r.scale === scale && r.segment === segment
    );
    const mine = here.find((r) => r.policy === policy);
    const best = Math.min(...here.map((r) => r.transfer_bytes_with_journal));
    transferTotal += mine.transfer_bytes_with_journal;
    storeTotal += mine.store_total_bytes;
    if (mine.transfer_bytes_with_journal === best) wins += 1;
    const ratio = mine.transfer_bytes_with_journal / Math.max(best, 1);
    if (ratio > worst.ratio) worst = { ratio, segment: key };
  }
  return { policy, wins, worst, transferTotal, storeTotal };
});
table.sort((a, b) => a.transferTotal - b.transferTotal);
console.log(
  `${cell("policy", 30)} ${pad("seg wins", 9)} ${pad("total edit B", 14)} ${pad("total store B", 14)} worst loss`
);
for (const r of table) {
  console.log(
    `${cell(r.policy, 30)} ${pad(r.wins + "/" + segments.length, 9)} ${pad(r.transferTotal, 14)} ${pad(r.storeTotal, 14)} ` +
      `${r.worst.ratio.toFixed(1)}x on ${r.worst.segment}`
  );
}

// --------------------------------------------------------------- push sweep
const sweep = rows.filter((r) => r.workload === "push-sweep");
if (sweep.length > 0) {
  console.log("\n\n## Push-size sweep — a property of ChunkStream, not of the policy");
  console.log(`${cell("policy", 18)} ${pad("push block B", 14)} ${pad("MiB/s p50", 10)}`);
  for (const r of sweep) {
    console.log(
      `${cell(r.policy, 18)} ${pad(r.push_block_bytes, 14)} ${pad(r.mib_per_s_at_p50, 10)}`
    );
  }
}
