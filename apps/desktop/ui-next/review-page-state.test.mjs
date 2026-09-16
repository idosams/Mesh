import assert from "node:assert/strict";
import { createRequire } from "node:module";
import test from "node:test";
import { build } from "esbuild";

async function loadModule(path) {
  const result = await build({
    entryPoints: [new URL(path, import.meta.url).pathname],
    bundle: true,
    format: "cjs",
    platform: "node",
    external: ["react", "react-dom", "react/jsx-runtime"],
    write: false,
  });
  const require = createRequire(import.meta.url);
  const module = { exports: {} };
  Function("require", "module", "exports", result.outputFiles[0].text)(require, module, module.exports);
  return module.exports;
}

const controls = {
  countLabel: "0 recorded reviews",
  overflowLabel: null,
  canSetupApproval: false,
  setupApprovalLabel: "Approval unavailable",
  setupApprovalReason: "Approval is unavailable in this test.",
  canRecordReview: false,
  recordReviewLabel: "Record reviewed version",
  recordReviewReason: "Save work privately first.",
  earlierReviews: [],
};

test("review page status envelopes are closed, bounded, and carry no review authority", async () => {
  const { reviewPageEnvelope } = await loadModule("./src/models/review-page.ts");
  const empty = reviewPageEnvelope({
    generation: 7,
    state: "empty",
    controls,
    status: {
      title: "No review is ready yet",
      description: "Save work privately before reviewing an exact saved version.",
    },
  }, 6);
  assert.equal(empty.state, "empty");
  assert.equal(empty.bundle, null);
  assert.equal(Object.isFrozen(empty), true);
  assert.equal(Object.isFrozen(empty.model), true);
  assert.deepEqual(Object.keys(empty.model).sort(), ["description", "state", "title"]);

  const unavailableInput = {
    generation: 8,
    state: "unavailable",
    controls,
    status: {
      title: "Review details are unavailable",
      description: "Mesh could not verify a complete bounded review for the current saved version.",
    },
  };
  const unavailable = reviewPageEnvelope(unavailableInput, 7);
  assert.equal(unavailable.state, "unavailable");
  assert.equal(unavailable.bundle, null);

  assert.throws(
    () => reviewPageEnvelope({
      generation: 9,
      state: "empty",
      controls,
      status: {
        title: "No review is ready yet",
        description: "Nothing is ready.",
        canApprove: true,
      },
    }, 8),
    /unrecognized or missing fields/,
  );
  assert.throws(
    () => reviewPageEnvelope({
      generation: 9,
      state: "unavailable",
      controls,
      status: { title: "Unavailable", description: "x".repeat(4_097) },
    }, 8),
    /unsafe or unbounded/,
  );
  const missingControlsField = { ...controls };
  delete missingControlsField.earlierReviews;
  assert.throws(
    () => reviewPageEnvelope({ ...unavailableInput, generation: 9, controls: missingControlsField }, 8),
    /unrecognized or missing fields/,
  );
  const repeatedOperation = "ab".repeat(32);
  assert.throws(
    () => reviewPageEnvelope({
      ...unavailableInput,
      generation: 9,
      controls: {
        ...controls,
        earlierReviews: [
          { operation: repeatedOperation, label: "Earlier review", canOpen: true },
          { operation: repeatedOperation, label: "Repeated review", canOpen: false },
        ],
      },
    }, 8),
    /repeated an earlier saved point/,
  );
  assert.throws(() => reviewPageEnvelope(unavailableInput, 8), /stale/);
});

test("empty and unavailable review states render explicit React pages without decision controls", async () => {
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const { ReviewPageStatus } = await loadModule("./src/organisms/review-page-status.tsx");
  for (const model of [
    {
      state: "empty",
      title: "No review is ready yet",
      description: "Save work privately before reviewing an exact saved version.",
    },
    {
      state: "unavailable",
      title: "Review details are unavailable",
      description: "Mesh could not verify a complete bounded review for the current saved version.",
    },
  ]) {
    const html = renderToStaticMarkup(React.createElement(ReviewPageStatus, { model, controls }));
    assert.match(html, new RegExp(`data-mesh-proof="review-${model.state}"`));
    assert.match(html, new RegExp(model.title));
    assert.match(html, new RegExp(model.description));
    assert.doesNotMatch(html, /Approve exact version|Choose export folder/);
    assert.match(html, /<button[^>]*disabled/);
  }
});
