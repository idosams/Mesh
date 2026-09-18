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

const live = {
  available: false,
  state: "idle",
  summary: "No agent currently owns this workspace.",
  workspaceRoot: "",
  changes: [],
  workspaces: [],
};

test("review page status envelopes are closed, bounded, and carry no review authority", async () => {
  const { reviewPageEnvelope } = await loadModule("./src/models/review-page.ts");
  const empty = reviewPageEnvelope({
    generation: 7,
    state: "empty",
    controls,
    live,
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
    live,
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
      live,
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
      live,
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

test("live agent review accepts stable snapshots and exposes no save approval or export authority", async () => {
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const { reviewPageEnvelope, liveReviewFileEnvelope } = await loadModule("./src/models/review-page.ts");
  const { LiveAgentReview } = await loadModule("./src/organisms/live-agent-review.tsx");
  const liveInput = {
    available: true,
    state: "ready",
    summary: "Two mutable live changes detected.",
    workspaceRoot: "/private/workspace-a",
    changes: [
      { path: "src/live.ts", kind: "modified-file" },
      { path: "tmp/new.bin", kind: "new-file" },
    ],
    workspaces: [
      { path: "/private/workspace-a", label: "Workspace A", state: "current", canOpen: false },
      { path: "/private/workspace-b", label: "Workspace B", state: "agent-assigned", canOpen: true },
    ],
  };
  const accepted = reviewPageEnvelope({
    generation: 11,
    state: "empty",
    controls,
    live: liveInput,
    status: { title: "No saved review", description: "Live work remains available." },
  }, 10);
  assert.equal(accepted.live.available, true);
  assert.equal(accepted.live.workspaces[1].canOpen, true);
  const snapshot = liveReviewFileEnvelope({
    generation: 11,
    path: "src/live.ts",
    error: null,
    snapshot: {
      path: "src/live.ts",
      kind: "modified-file",
      byteCount: 12,
      contentDigest: "ab".repeat(32),
      executable: false,
      text: "changed\n",
      previewKind: "text",
      imageDataUrl: null,
      previewError: null,
    },
  }, 11, accepted.live);
  assert.equal(snapshot.preview.text, "changed\n");
  assert.throws(() => liveReviewFileEnvelope({
    generation: 10,
    path: "src/live.ts",
    error: "stale",
    snapshot: null,
  }, 11, accepted.live), /stale/);

  const markup = renderToStaticMarkup(React.createElement(LiveAgentReview, {
    model: accepted.live,
    preview: snapshot.preview,
    loading: false,
    error: null,
    onIntent: () => {},
  }));
  assert.match(markup, /Live agent work/);
  assert.match(markup, /Mutable/);
  assert.match(markup, /Unrecorded/);
  assert.match(markup, /Workspace B/);
  assert.doesNotMatch(markup, /Approve exact version|Save privately|Create Git branch/);
});

test("live agent review bounds a 10,000-change projection without hiding the complete match count", async () => {
  const React = await import("react");
  const { renderToStaticMarkup } = await import("react-dom/server");
  const { liveReviewChangeProjection, LIVE_REVIEW_ROW_LIMIT } = await loadModule("./src/models/review-page.ts");
  const { LiveAgentReview } = await loadModule("./src/organisms/live-agent-review.tsx");
  const changes = Array.from({ length: 10_000 }, (_, index) => ({
    path: `src/generated/file-${String(index).padStart(5, "0")}.ts`,
    kind: "modified-file",
  }));
  const projection = liveReviewChangeProjection(changes, "");
  assert.equal(projection.matched, 10_000);
  assert.equal(projection.changes.length, LIVE_REVIEW_ROW_LIMIT);
  assert.equal(projection.truncated, true);
  assert.equal(projection.changes.at(-1).path, "src/generated/file-00499.ts");
  assert.throws(() => liveReviewChangeProjection(changes, "", 0), /bound was invalid/);

  const markup = renderToStaticMarkup(React.createElement(LiveAgentReview, {
    model: {
      available: true,
      state: "ready",
      summary: "10,000 mutable live changes detected.",
      workspaceRoot: "/private/workspace-a",
      changes,
      workspaces: [{ path: "/private/workspace-a", label: "Workspace A", state: "current", canOpen: false }],
    },
    preview: null,
    loading: false,
    error: null,
    errorPath: null,
    onIntent: () => assert.fail("SSR must not emit an intent"),
  }));
  assert.equal((markup.match(/aria-pressed=/g) || []).length, LIVE_REVIEW_ROW_LIMIT);
  assert.match(markup, /500 of 10000 visible changes shown/);
  assert.match(markup, /Showing the first 500 visible live changes/);
  assert.doesNotMatch(markup, /file-09999\.ts/);
});
