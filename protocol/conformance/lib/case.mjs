// protocol/conformance/lib/case.mjs — what a conformance case is (task 01KZC2P6SQ7E00ENWQ6F4YVXJJ).
//
// A case is a question put to a client across the adapter boundary, and an expectation stated in
// terms of the published material. Four fields carry the weight, and each earns its place by what a
// report would lose without it:
//
//  - `rule` — the normative sentence being tested, quoted or paraphrased tightly. A failure prints
//    it, so the implementer reads the rule rather than the assertion.
//  - `citation` — where that sentence lives. A rule with no address is a rule the implementer has
//    to take on faith, and this suite is for people who have no reason to.
//  - `requires` — the adapter capability the question needs. A client that does not declare it gets
//    `unsupported`, never `fail`: a partial implementation reporting honestly is not a broken one.
//  - `specGap` — set when NO client could pass from the published material alone, because the
//    specification does not say how. The report counts these separately and names them; a suite that
//    let them read as ordinary `unsupported` would be hiding the specification's own defects behind
//    an implementation's.

const REQUIRED = ["id", "family", "title", "rule", "citation", "requires"];

export function makeCase(definition) {
  for (const field of REQUIRED) {
    if (!definition[field]) throw new Error(`a conformance case is missing ${field}: ${definition.id ?? "?"}`);
  }
  if (!Array.isArray(definition.requests) || definition.requests.length === 0) {
    throw new Error(`conformance case ${definition.id} asks nothing`);
  }
  if (typeof definition.expect !== "function") {
    throw new Error(`conformance case ${definition.id} has no expectation`);
  }
  return Object.freeze({ specGap: null, ...definition });
}

/** The common shape: one request, one check over its response. */
export function single({ request, check, ...rest }) {
  return makeCase({ ...rest, requests: [request], expect: (responses) => check(responses[0]) });
}

/**
 * Several requests answered in order, checked together. An entry may be a function of the responses
 * so far, which is how a case asks a follow-up question about bytes the client just produced.
 */
export function several({ requests, check, ...rest }) {
  return makeCase({ ...rest, requests, expect: check });
}

/**
 * A case nothing can answer yet, declared so the report says so out loud. It asks the client one
 * question it is expected not to implement; a client that DOES answer is graded normally.
 */
export function declared({ request, check, ...rest }) {
  return single({ request, check, ...rest });
}
