// protocol/conformance/lib/run-cases.mjs — putting the catalogue to a client
// (task 01KZC2P6SQ7E00ENWQ6F4YVXJJ).
//
// The grading rule, stated once so it is not re-derived per case:
//
//  - a capability the client did not declare, or a request the client answers `unsupported` →
//    UNSUPPORTED, never FAIL. The reason is recorded verbatim.
//  - a request the client answers with an error, or an adapter that crashes, times out or writes
//    something that is not JSON → FAIL, naming the adapter rule it broke. An adapter that dies is
//    not an honest `unsupported`.
//  - a request answered, expectation met → PASS. Not met → FAIL, with the expectation's own
//    sentence as the detail.

import { AdapterError } from "./adapter.mjs";
import { FAIL, PASS, UNSUPPORTED } from "./report.mjs";

const capabilitiesOf = (item) => (Array.isArray(item.requires) ? item.requires : [item.requires]);

function base(item, result, extra) {
  return {
    id: item.id,
    family: item.family,
    title: item.title,
    rule: item.rule,
    citation: item.citation,
    requires: capabilitiesOf(item),
    spec_gap: item.specGap ?? null,
    result,
    ...extra,
  };
}

export async function runCases(adapter, hello, cases, { only } = {}) {
  const declared = new Set(hello.capabilities);
  const results = [];

  for (const item of cases) {
    if (only && !item.id.includes(only) && item.family !== only) continue;

    const missing = capabilitiesOf(item).filter((capability) => !declared.has(capability));
    if (missing.length > 0) {
      results.push(
        base(item, UNSUPPORTED, {
          unsupported_because: item.specGap ? "specification" : "client",
          detail: `the client did not declare the capability ${missing.join(", ")}`,
        }),
      );
      continue;
    }

    let responses;
    try {
      responses = [];
      for (const request of item.requests) {
        const resolved = typeof request === "function" ? request(responses) : request;
        responses.push(await adapter.ask(resolved));
      }
    } catch (error) {
      results.push(
        base(item, FAIL, {
          detail:
            error instanceof AdapterError
              ? `the adapter broke cwp-conformance-adapter/0: ${error.message}`
              : `the harness could not put the case: ${error.message}`,
        }),
      );
      continue;
    }

    const unsupported = responses.find(
      (response) => response.ok === false && response.unsupported === true,
    );
    if (unsupported) {
      results.push(
        base(item, UNSUPPORTED, {
          unsupported_because: item.specGap ? "specification" : "client",
          detail: unsupported.reason ?? "the client answered `unsupported` without a reason",
          tracking: unsupported.tracking ?? item.specGap?.tracking ?? null,
        }),
      );
      continue;
    }

    const refused = responses.find((response) => response.ok !== true);
    if (refused) {
      results.push(
        base(item, FAIL, {
          detail: `the client answered with an error: ${refused.reason ?? JSON.stringify(refused)}`,
        }),
      );
      continue;
    }

    let detail;
    try {
      detail = item.expect(responses);
    } catch (error) {
      detail = `the expectation could not be evaluated: ${error.message}`;
    }

    results.push(detail === null || detail === undefined ? base(item, PASS, {}) : base(item, FAIL, { detail }));
  }

  return results;
}
