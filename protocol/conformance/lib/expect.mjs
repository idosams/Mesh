// protocol/conformance/lib/expect.mjs — comparison helpers whose failure text is the finding
// (task 01KZC2P6SQ7E00ENWQ6F4YVXJJ).
//
// A conformance failure that says "assertion failed" costs the implementer the afternoon the suite
// was supposed to save. Every helper here returns either `null` — meaning the case holds — or a
// sentence naming what differed and where, so the report can print the rule, the citation and the
// difference with nothing further to look up.

const shorten = (value, keep = 80) => {
  const text = typeof value === "string" ? value : JSON.stringify(value);
  return text.length <= keep ? text : `${text.slice(0, keep)}… (${text.length} chars)`;
};

/**
 * A window of sixteen bytes either side of `at`, so the printed bytes actually contain the
 * difference. Truncating from the front is what makes a byte-comparison failure useless.
 */
function window(hex, at) {
  const from = Math.max(0, (at - 16) * 2);
  const to = Math.min(hex.length, (at + 17) * 2);
  return `${from > 0 ? "…" : ""}${hex.slice(from, to)}${to < hex.length ? "…" : ""}`;
}

/** The first differing byte of two hex strings, described in a sentence. */
export function hexEquals(actual, expected, what) {
  if (typeof actual !== "string") return `${what} is ${typeof actual}, expected a hex string`;
  if (actual === expected) return null;
  const at = firstDifference(actual, expected) ?? 0;
  const lengths =
    actual.length === expected.length
      ? `${what} differs at byte ${at}: expected 0x${expected.slice(at * 2, at * 2 + 2)}, ` +
        `got 0x${actual.slice(at * 2, at * 2 + 2)}`
      : `${what} is ${actual.length / 2} bytes, expected ${expected.length / 2}; ` +
        `they first differ at byte ${at}`;
  return (
    `${lengths}\n        expected ${window(expected, at)}\n        got      ${window(actual, at)}`
  );
}

function firstDifference(left, right) {
  const shared = Math.min(left.length, right.length);
  for (let index = 0; index < shared; index += 2) {
    if (left.slice(index, index + 2) !== right.slice(index, index + 2)) return index / 2;
  }
  return shared === left.length && shared === right.length ? null : shared / 2;
}

/** Structural equality with the path to the first disagreement. */
export function deepEquals(actual, expected, what) {
  const path = walk(actual, expected, "");
  if (path === null) return null;
  const [where, got, wanted] = path;
  return (
    `${what}${where} is ${shorten(got)}, expected ${shorten(wanted)}` +
    `\n        expected ${shorten(JSON.stringify(expected), 160)}` +
    `\n        got      ${shorten(JSON.stringify(actual), 160)}`
  );
}

function walk(actual, expected, where) {
  if (Array.isArray(expected)) {
    if (!Array.isArray(actual)) return [where, actual, expected];
    if (actual.length !== expected.length) {
      return [`${where}.length`, actual.length, expected.length];
    }
    for (let index = 0; index < expected.length; index += 1) {
      const found = walk(actual[index], expected[index], `${where}[${index}]`);
      if (found) return found;
    }
    return null;
  }
  if (expected !== null && typeof expected === "object") {
    if (actual === null || typeof actual !== "object" || Array.isArray(actual)) {
      return [where, actual, expected];
    }
    const expectedKeys = Object.keys(expected).sort();
    const actualKeys = Object.keys(actual).sort();
    if (expectedKeys.join(",") !== actualKeys.join(",")) {
      return [`${where} keys`, actualKeys.join(","), expectedKeys.join(",")];
    }
    for (const key of expectedKeys) {
      const found = walk(actual[key], expected[key], `${where}.${key}`);
      if (found) return found;
    }
    return null;
  }
  return actual === expected ? null : [where, actual, expected];
}

export function equals(actual, expected, what) {
  return actual === expected ? null : `${what} is ${shorten(actual)}, expected ${shorten(expected)}`;
}

/** The response to a `bytes.reject` request must be a refusal, and the reason is printed. */
export function rejected(response, what) {
  if (response.rejected === true) return null;
  return `${what} was accepted; a conformant decoder refuses it. The client said: ${response.reason}`;
}

/** The response to a `message.admit` request must be a refusal, optionally carrying a named code. */
export function refused(response, what, errorCode) {
  if (response.admitted !== false) {
    return `${what} was admitted; the published preconditions refuse it`;
  }
  if (errorCode !== undefined && response.error_code !== errorCode) {
    return (
      `${what} was refused with error code ${JSON.stringify(response.error_code)}, ` +
      `the published table names ${JSON.stringify(errorCode)}`
    );
  }
  return null;
}

export function admitted(response, what) {
  if (response.admitted === true) return null;
  return `${what} was refused (${response.reason}); the published preconditions admit it`;
}
