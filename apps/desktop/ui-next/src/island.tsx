import React, { useEffect, useLayoutEffect, useState, type ErrorInfo, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import stylesheet from "./styles.css?inline";
import { ProductionWorkspacePage } from "./pages/production-workspace-page";
import { ArtifactReview } from "./organisms/artifact-review";
import { LiveAgentReview } from "./organisms/live-agent-review";
import { SegmentedControl } from "./atoms/segmented-control";
import { ReviewPageStatus } from "./organisms/review-page-status";
import { ImportWorkbench } from "./organisms/import-workbench";
import { WorkspaceOverview } from "./organisms/workspace-overview";
import { WorkspaceHeader } from "./organisms/workspace-chrome";
import { WorkspaceCurrent } from "./organisms/workspace-current";
import { WorkspaceChanges, WorkspaceFiles } from "./organisms/workspace-files-changes";
import { WorkspaceVersionNavigator } from "./organisms/workspace-version-navigator";
import { ConfirmationDialog } from "./organisms/confirmation-dialog";
import { WorkspaceEntryPage } from "./pages/workspace-entry-page";
import { WorkspaceRestore } from "./organisms/workspace-restore";
import { WorkspaceDestination } from "./organisms/workspace-destination";
import {
  liveReviewFileEnvelope,
  reviewPageEnvelope,
  type LiveReviewFilePreview,
  type LiveReviewModel,
  type ReviewPageControls,
  type ReviewPageStatusModel,
} from "./models/review-page";
import { reviewArtifactPreviewEnvelope } from "./models/review-artifact-preview";
import {
  reconcileReviewWorkbenchProjection,
  reduceReviewWorkbench,
  type ReviewArtifactPreview,
  type ReviewWorkbenchIntent,
  type ReviewWorkbenchModel,
} from "./models/review-workbench";
import { workspaceOverviewEnvelope, type WorkspaceOverviewIntent } from "./models/workspace-overview";
import { workspaceChromeEnvelope } from "./models/workspace-chrome";
import { workspaceCurrentEnvelope, type WorkspaceCurrentIntent } from "./models/workspace-current";
import { workspaceFilesChangesEnvelope, type WorkspaceFilesChangesIntent } from "./models/workspace-files-changes";
import { importWorkbenchEnvelope, type ImportWorkbenchIntent } from "./models/import-workbench";
import {
  advanceImportFocusRequest,
  authorizeImportReviewFocus,
  captureImportFocusRequest,
  type ImportFocusRequest,
  type ImportReviewFocusAuthorization,
} from "./models/import-focus";
import { workspaceVersionsEnvelope, type WorkspaceVersionsIntent } from "./models/workspace-versions";
import { confirmationEnvelope, confirmationIntent } from "./models/confirmation";
import { workspaceEntryEnvelope, type WorkspaceEntryIntent } from "./models/workspace-entry";
import { workspaceRestoreEnvelope, type WorkspaceRestoreIntent } from "./models/workspace-restore";
import { workspaceDestinationEnvelope, type WorkspaceDestinationIntent } from "./models/workspace-destination";
import { createIslandLiveness, type IslandLiveness } from "./models/island-liveness";

const PROJECTION_EVENT = "mesh:review-workbench-projection";
const MOUNTED_EVENT = "mesh:review-workbench-mounted";
const REJECTED_EVENT = "mesh:review-workbench-rejected";
const AVAILABLE_EVENT = "mesh:review-workbench-available";
const INTENT_EVENT = "mesh:review-workbench-intent";
const ARTIFACT_PREVIEW_EVENT = "mesh:review-workbench-artifact-preview";
const LIVE_PREVIEW_EVENT = "mesh:review-workbench-live-preview";
const OVERVIEW_PROJECTION_EVENT = "mesh:workspace-overview-projection";
const OVERVIEW_MOUNTED_EVENT = "mesh:workspace-overview-mounted";
const OVERVIEW_REJECTED_EVENT = "mesh:workspace-overview-rejected";
const OVERVIEW_AVAILABLE_EVENT = "mesh:workspace-overview-available";
const OVERVIEW_INTENT_EVENT = "mesh:workspace-overview-intent";
const IMPORT_PROJECTION_EVENT = "mesh:import-workbench-projection";
const IMPORT_MOUNTED_EVENT = "mesh:import-workbench-mounted";
const IMPORT_REJECTED_EVENT = "mesh:import-workbench-rejected";
const IMPORT_AVAILABLE_EVENT = "mesh:import-workbench-available";
const IMPORT_INTENT_EVENT = "mesh:import-workbench-intent";
const IMPORT_EXTERNAL_FOCUS_EVENT = "mesh:import-workbench-external-focus";
const VERSIONS_PROJECTION_EVENT = "mesh:workspace-versions-projection";
const VERSIONS_MOUNTED_EVENT = "mesh:workspace-versions-mounted";
const VERSIONS_REJECTED_EVENT = "mesh:workspace-versions-rejected";
const VERSIONS_AVAILABLE_EVENT = "mesh:workspace-versions-available";
const VERSIONS_INTENT_EVENT = "mesh:workspace-versions-intent";
const CHROME_PROJECTION_EVENT = "mesh:workspace-chrome-projection";
const CHROME_MOUNTED_EVENT = "mesh:workspace-chrome-mounted";
const CHROME_REJECTED_EVENT = "mesh:workspace-chrome-rejected";
const CHROME_AVAILABLE_EVENT = "mesh:workspace-chrome-available";
const ENTRY_PROJECTION_EVENT = "mesh:workspace-entry-projection";
const ENTRY_MOUNTED_EVENT = "mesh:workspace-entry-mounted";
const ENTRY_REJECTED_EVENT = "mesh:workspace-entry-rejected";
const ENTRY_AVAILABLE_EVENT = "mesh:workspace-entry-available";
const ENTRY_INTENT_EVENT = "mesh:workspace-entry-intent";
const RESTORE_PROJECTION_EVENT = "mesh:workspace-restore-projection";
const RESTORE_MOUNTED_EVENT = "mesh:workspace-restore-mounted";
const RESTORE_REJECTED_EVENT = "mesh:workspace-restore-rejected";
const RESTORE_AVAILABLE_EVENT = "mesh:workspace-restore-available";
const RESTORE_INTENT_EVENT = "mesh:workspace-restore-intent";
const DESTINATION_PROJECTION_EVENT = "mesh:workspace-destination-projection";
const DESTINATION_MOUNTED_EVENT = "mesh:workspace-destination-mounted";
const DESTINATION_REJECTED_EVENT = "mesh:workspace-destination-rejected";
const DESTINATION_AVAILABLE_EVENT = "mesh:workspace-destination-available";
const DESTINATION_INTENT_EVENT = "mesh:workspace-destination-intent";
const CURRENT_PROJECTION_EVENT = "mesh:workspace-current-projection";
const CURRENT_MOUNTED_EVENT = "mesh:workspace-current-mounted";
const CURRENT_REJECTED_EVENT = "mesh:workspace-current-rejected";
const CURRENT_AVAILABLE_EVENT = "mesh:workspace-current-available";
const CURRENT_INTENT_EVENT = "mesh:workspace-current-intent";
const WORK_PROJECTION_EVENT = "mesh:workspace-files-changes-projection";
const WORK_MOUNTED_EVENT = "mesh:workspace-files-changes-mounted";
const WORK_REJECTED_EVENT = "mesh:workspace-files-changes-rejected";
const WORK_AVAILABLE_EVENT = "mesh:workspace-files-changes-available";
const WORK_INTENT_EVENT = "mesh:workspace-files-changes-intent";
const CONFIRMATION_PROJECTION_EVENT = "mesh:confirmation-projection";
const CONFIRMATION_MOUNTED_EVENT = "mesh:confirmation-mounted";
const CONFIRMATION_REJECTED_EVENT = "mesh:confirmation-rejected";
const CONFIRMATION_AVAILABLE_EVENT = "mesh:confirmation-available";
const CONFIRMATION_INTENT_EVENT = "mesh:confirmation-intent";
const CONFIRMATION_DISMISSED_EVENT = "mesh:confirmation-dismissed";

const STATIC_ISLAND_HOSTS = Object.freeze([
  ["workspace-chrome-next", "workspace-header"],
  ["workspace-entry-next", "workspace-entry"],
  ["import-workbench-next", "import-workbench"],
  ["workspace-overview-next", "workspace-overview"],
  ["workspace-current-next", "workspace-current"],
  ["workspace-files-next", "workspace-files"],
  ["workspace-changes-next", "workspace-changes"],
  ["review-workbench-next", "review-workbench"],
  ["workspace-versions-next", "workspace-versions"],
  ["workspace-destination-next", "workspace-destination"],
  ["workspace-restore-next", "workspace-restore"],
  ["confirmation-dialog-next", "confirmation-dialog"],
] as const);

function hasExactStaticIslandHosts(productionHost: HTMLElement) {
  const children = Array.from(productionHost.children);
  return children.length === STATIC_ISLAND_HOSTS.length
    && STATIC_ISLAND_HOSTS.every(([id, slot], index) => {
      const surface = children[index];
      return surface.id === id
        && surface.getAttribute("slot") === slot
        && document.querySelectorAll(`#${id}`).length === 1;
    });
}

const productionHost = document.getElementById("mesh-app-next");
if (!productionHost || !hasExactStaticIslandHosts(productionHost)) {
  throw new Error("Mesh React shell host topology is invalid");
}
const shadow = productionHost.shadowRoot ?? productionHost.attachShadow({ mode: "open" });
const style = document.createElement("style");
style.textContent = stylesheet;
const container = document.createElement("div");
shadow.replaceChildren(style, container);
const productionRoot = createRoot(container);
// Commit the one visible shell before advertising it. Packaged WKWebView launches may suspend
// timers while the window is still backgrounded, so a pre-commit readiness marker can strand
// both assistive focus and the renderer proof waiting for controls that are not painted yet.
flushSync(() => productionRoot.render(<ProductionWorkspacePage />));
productionHost.setAttribute("data-mesh-react-shell-active", "true");
document.dispatchEvent(new CustomEvent("mesh:react-shell-committed"));

function Committed({ children, onCommit }: { children: ReactNode; onCommit: () => void }) {
  useLayoutEffect(onCommit, [onCommit]);
  return children;
}

class IslandRenderBoundary extends React.Component<{
  children: ReactNode;
  generation: number;
  liveness: IslandLiveness;
}, { failed: boolean; generation: number }> {
  state = { failed: false, generation: this.props.generation };

  static getDerivedStateFromProps(
    props: { generation: number },
    state: { failed: boolean; generation: number },
  ) {
    return props.generation === state.generation
      ? null
      : { failed: false, generation: props.generation };
  }

  static getDerivedStateFromError() {
    return { failed: true };
  }

  componentDidCatch(error: unknown, _info: ErrorInfo) {
    this.props.liveness.fail(this.props.generation, error);
  }

  render() {
    return this.state.failed ? null : this.props.children;
  }
}

function guardedProjection(
  generation: number,
  liveness: IslandLiveness,
  children: ReactNode,
) {
  return <IslandRenderBoundary generation={generation} liveness={liveness}>
    {children}
  </IslandRenderBoundary>;
}

function projectionLiveness(rejectedEvent: string) {
  return createIslandLiveness({
    reject: (generation, reason) => document.dispatchEvent(new CustomEvent(rejectedEvent, {
      detail: Object.freeze({ generation, reason }),
    })),
  });
}

function ReviewModeChooser({ mode, onChange }: {
  mode: "Saved review" | "Live agent work";
  onChange: (mode: "Saved review" | "Live agent work") => void;
}) {
  return (
    <div className="mb-4 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-border bg-card p-4">
      <div>
        <p className="text-xs font-semibold uppercase tracking-[0.16em] text-primary">Review mode</p>
        <p className="mt-1 text-sm text-muted-foreground">Saved review is immutable. Live agent work is mutable and unrecorded.</p>
      </div>
      <SegmentedControl label="Review mode" options={["Saved review", "Live agent work"]} value={mode} onChange={(value) => onChange(value as typeof mode)} />
    </div>
  );
}

function IslandReview({ source, generation, bundle, controls, live }: {
  source: ReviewWorkbenchModel;
  generation: number;
  bundle: string;
  controls: ReviewPageControls;
  live: LiveReviewModel;
}) {
  const [model, setModel] = useState(source);
  const [reviewMode, setReviewMode] = useState<"Saved review" | "Live agent work">(live.available ? "Live agent work" : "Saved review");
  const [artifactPreview, setArtifactPreview] = useState<ReviewArtifactPreview | null>(null);
  const [artifactPreviewLoading, setArtifactPreviewLoading] = useState(false);
  const [artifactPreviewError, setArtifactPreviewError] = useState<string | null>(null);
  const [livePreview, setLivePreview] = useState<LiveReviewFilePreview | null>(null);
  const [livePreviewLoading, setLivePreviewLoading] = useState(false);
  const [livePreviewError, setLivePreviewError] = useState<string | null>(null);
  const [livePreviewErrorPath, setLivePreviewErrorPath] = useState<string | null>(null);
  useEffect(() => {
    setModel((current) => reconcileReviewWorkbenchProjection(current, source));
    // Native requests are generation-bound. A refreshed authority projection cancels any older
    // in-flight request, so release its spinner while retaining an already verified same-bundle
    // preview and the person's local comparison choices.
    setArtifactPreviewLoading(false);
  }, [source]);
  useEffect(() => {
    const receivePreview = (event: Event) => {
      try {
        const preview = reviewArtifactPreviewEnvelope(
          (event as CustomEvent<unknown>).detail,
          generation,
          bundle,
          source,
        );
        setArtifactPreview(preview);
        setArtifactPreviewError(null);
      } catch (error) {
        setArtifactPreview(null);
        setArtifactPreviewError(error instanceof Error ? error.message : "The visual comparison was refused.");
      } finally {
        setArtifactPreviewLoading(false);
      }
    };
    document.addEventListener(ARTIFACT_PREVIEW_EVENT, receivePreview);
    return () => document.removeEventListener(ARTIFACT_PREVIEW_EVENT, receivePreview);
  }, [bundle, generation, source]);
  useEffect(() => {
    const receivePreview = (event: Event) => {
      try {
        const result = liveReviewFileEnvelope((event as CustomEvent<unknown>).detail, generation, live);
        setLivePreview(result.preview);
        setLivePreviewError(result.error);
        setLivePreviewErrorPath(result.error ? result.path : null);
      } catch (error) {
        setLivePreview(null);
        setLivePreviewError(error instanceof Error ? error.message : "The live snapshot was refused.");
        setLivePreviewErrorPath(null);
      } finally {
        setLivePreviewLoading(false);
      }
    };
    document.addEventListener(LIVE_PREVIEW_EVENT, receivePreview);
    return () => document.removeEventListener(LIVE_PREVIEW_EVENT, receivePreview);
  }, [generation, live]);
  const handleIntent = (intent: ReviewWorkbenchIntent) => {
    if (intent.type === "select-change") {
      setModel((current) => reduceReviewWorkbench(current, intent));
      setArtifactPreview(null);
      setArtifactPreviewLoading(false);
      setArtifactPreviewError(null);
      return;
    }
    if (intent.type === "change-mode"
      || intent.type === "change-diff-layout") {
      setModel((current) => reduceReviewWorkbench(current, intent));
      return;
    }
    if (intent.type === "load-artifact-preview") {
      setArtifactPreviewLoading(true);
      setArtifactPreviewError(null);
    }
    if (intent.type === "load-live-file") {
      setLivePreviewLoading(true);
      setLivePreviewError(null);
      setLivePreviewErrorPath(null);
    }
    document.dispatchEvent(new CustomEvent(INTENT_EVENT, {
      detail: Object.freeze({ generation, bundle, intent: Object.freeze({ ...intent }) }),
    }));
  };
  return <div>
    <ReviewModeChooser mode={reviewMode} onChange={setReviewMode} />
    {reviewMode === "Saved review" ? <ArtifactReview
      model={model}
      controls={controls}
      onIntent={handleIntent}
      artifactPreview={artifactPreview}
      artifactPreviewLoading={artifactPreviewLoading}
      artifactPreviewError={artifactPreviewError}
    /> : <LiveAgentReview
      model={live}
      preview={livePreview}
      loading={livePreviewLoading}
      error={livePreviewError}
      errorPath={livePreviewErrorPath}
      onIntent={handleIntent}
    />}
  </div>;
}

function IslandReviewStatus({ model, controls, live, generation }: {
  model: ReviewPageStatusModel;
  controls: ReviewPageControls;
  live: LiveReviewModel;
  generation: number;
}) {
  const [reviewMode, setReviewMode] = useState<"Saved review" | "Live agent work">(live.available ? "Live agent work" : "Saved review");
  const [preview, setPreview] = useState<LiveReviewFilePreview | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [errorPath, setErrorPath] = useState<string | null>(null);
  useEffect(() => {
    const receivePreview = (event: Event) => {
      try {
        const result = liveReviewFileEnvelope((event as CustomEvent<unknown>).detail, generation, live);
        setPreview(result.preview);
        setError(result.error);
        setErrorPath(result.error ? result.path : null);
      } catch (caught) {
        setPreview(null);
        setError(caught instanceof Error ? caught.message : "The live snapshot was refused.");
        setErrorPath(null);
      } finally {
        setLoading(false);
      }
    };
    document.addEventListener(LIVE_PREVIEW_EVENT, receivePreview);
    return () => document.removeEventListener(LIVE_PREVIEW_EVENT, receivePreview);
  }, [generation, live]);
  const onIntent = (intent: ReviewWorkbenchIntent) => {
    if (intent.type === "load-live-file") {
      setLoading(true);
      setError(null);
      setErrorPath(null);
    }
    document.dispatchEvent(new CustomEvent(INTENT_EVENT, {
      detail: Object.freeze({ generation, bundle: null, intent: Object.freeze({ ...intent }) }),
    }));
  };
  return <div>
    <ReviewModeChooser mode={reviewMode} onChange={setReviewMode} />
    {reviewMode === "Saved review"
      ? <ReviewPageStatus model={model} controls={controls} onIntent={onIntent} />
      : <LiveAgentReview model={live} preview={preview} loading={loading} error={error} errorPath={errorPath} onIntent={onIntent} />}
  </div>;
}

const host = document.getElementById("review-workbench-next");
if (host) {
  const shadow = host.shadowRoot ?? host.attachShadow({ mode: "open" });
  const style = document.createElement("style");
  style.textContent = stylesheet;
  const container = document.createElement("div");
  container.className = "mesh-review-island";
  shadow.replaceChildren(style, container);
  const liveness = projectionLiveness(REJECTED_EVENT);
  const root = createRoot(container);
  let generation = -1;

  document.addEventListener(PROJECTION_EVENT, (event) => {
    let beganGeneration: number | null = null;
    try {
      const candidate = reviewPageEnvelope((event as CustomEvent<unknown>).detail, generation);
      generation = candidate.generation;
      liveness.begin(candidate.generation);
      beganGeneration = candidate.generation;
      root.render(guardedProjection(candidate.generation, liveness,
        <Committed onCommit={() => {
          if (!liveness.commit(candidate.generation)) return;
          document.dispatchEvent(new CustomEvent(MOUNTED_EVENT, {
            detail: Object.freeze({ generation: candidate.generation, bundle: candidate.bundle }),
          }));
        }}>
          {candidate.state === "ready" ? (
            <IslandReview
              key={candidate.bundle}
              source={candidate.model}
              generation={candidate.generation}
              bundle={candidate.bundle}
              controls={candidate.controls}
              live={candidate.live}
            />
          ) : <IslandReviewStatus
            model={candidate.model}
            controls={candidate.controls}
            live={candidate.live}
            generation={candidate.generation}
          />}
        </Committed>,
      ));
    } catch (error) {
      if (beganGeneration !== null) liveness.cancel(beganGeneration);
      document.dispatchEvent(new CustomEvent(REJECTED_EVENT, {
        detail: Object.freeze({
          generation: Number.isSafeInteger((event as CustomEvent<Record<string, unknown>>).detail?.generation)
            ? (event as CustomEvent<Record<string, unknown>>).detail.generation
            : null,
          reason: error instanceof Error ? error.message : "The review workbench refused its input.",
        }),
      }));
    }
  });
  document.dispatchEvent(new CustomEvent(AVAILABLE_EVENT));
}

const overviewHost = document.getElementById("workspace-overview-next");
if (overviewHost) {
  const shadow = overviewHost.shadowRoot ?? overviewHost.attachShadow({ mode: "open" });
  const style = document.createElement("style");
  style.textContent = stylesheet;
  const container = document.createElement("div");
  container.className = "mesh-review-island";
  shadow.replaceChildren(style, container);
  const liveness = projectionLiveness(OVERVIEW_REJECTED_EVENT);
  const root = createRoot(container);
  let generation = -1;
  document.addEventListener(OVERVIEW_PROJECTION_EVENT, (event) => {
    let beganGeneration: number | null = null;
    try {
      const candidate = workspaceOverviewEnvelope((event as CustomEvent<unknown>).detail, generation);
      generation = candidate.generation;
      const exactGeneration = candidate.generation;
      liveness.begin(exactGeneration);
      beganGeneration = exactGeneration;
      const onIntent = (intent: WorkspaceOverviewIntent) => {
        document.dispatchEvent(new CustomEvent(OVERVIEW_INTENT_EVENT, {
          detail: Object.freeze({ generation: exactGeneration, intent: Object.freeze({ ...intent }) }),
        }));
      };
      root.render(guardedProjection(exactGeneration, liveness,
        <Committed onCommit={() => {
          if (!liveness.commit(exactGeneration)) return;
          document.dispatchEvent(new CustomEvent(OVERVIEW_MOUNTED_EVENT, {
            detail: Object.freeze({ generation: exactGeneration }),
          }));
        }}>
          <WorkspaceOverview model={candidate.model} onIntent={onIntent} />
        </Committed>,
      ));
    } catch (error) {
      if (beganGeneration !== null) liveness.cancel(beganGeneration);
      document.dispatchEvent(new CustomEvent(OVERVIEW_REJECTED_EVENT, {
        detail: Object.freeze({
          generation: Number.isSafeInteger((event as CustomEvent<Record<string, unknown>>).detail?.generation)
            ? (event as CustomEvent<Record<string, unknown>>).detail.generation
            : null,
          reason: error instanceof Error ? error.message : "The workspace overview refused its input.",
        }),
      }));
    }
  });
  document.dispatchEvent(new CustomEvent(OVERVIEW_AVAILABLE_EVENT));
}

const importHost = document.getElementById("import-workbench-next");
if (importHost) {
  const shadow = importHost.shadowRoot ?? importHost.attachShadow({ mode: "open" });
  const style = document.createElement("style");
  style.textContent = stylesheet;
  const container = document.createElement("div");
  container.className = "mesh-review-island";
  shadow.replaceChildren(style, container);
  const liveness = projectionLiveness(IMPORT_REJECTED_EVENT);
  const root = createRoot(container);
  let generation = -1;
  let phase: "select" | "review" | null = null;
  let pendingFocus: ImportFocusRequest | null = null;
  let commitFocus: ImportReviewFocusAuthorization | null = null;
  document.addEventListener(IMPORT_EXTERNAL_FOCUS_EVENT, (event) => {
    const detail = (event as CustomEvent<Record<string, unknown>>).detail;
    if (!detail
      || Object.keys(detail).length !== 1
      || detail.generation !== generation
      || phase !== "select") return;
    pendingFocus = captureImportFocusRequest(shadow, generation);
  });
  document.addEventListener(IMPORT_PROJECTION_EVENT, (event) => {
    let beganGeneration: number | null = null;
    try {
      const previousGeneration = generation;
      const candidate = importWorkbenchEnvelope((event as CustomEvent<unknown>).detail, generation);
      const previousPhase = phase;
      commitFocus?.cancel();
      commitFocus = null;
      if (candidate.model.phase === "select") {
        pendingFocus = advanceImportFocusRequest(
          pendingFocus,
          previousGeneration,
          candidate.generation,
        );
      } else {
        commitFocus = previousPhase === "select"
          ? authorizeImportReviewFocus(pendingFocus, previousGeneration, shadow, document)
          : null;
        pendingFocus = null;
      }
      generation = candidate.generation;
      const exactGeneration = candidate.generation;
      liveness.begin(exactGeneration);
      beganGeneration = exactGeneration;
      phase = candidate.model.phase;
      const onIntent = (intent: ImportWorkbenchIntent) => {
        if (intent.type === "choose-folder" || intent.type === "preview-path") {
          pendingFocus = captureImportFocusRequest(shadow, exactGeneration);
        }
        document.dispatchEvent(new CustomEvent(IMPORT_INTENT_EVENT, {
          detail: Object.freeze({ generation: exactGeneration, intent: Object.freeze({ ...intent }) }),
        }));
      };
      root.render(guardedProjection(exactGeneration, liveness,
        <Committed onCommit={() => {
          if (!liveness.commit(exactGeneration)) return;
          document.dispatchEvent(new CustomEvent(IMPORT_MOUNTED_EVENT, {
            detail: Object.freeze({ generation: exactGeneration }),
          }));
        }}>
          <ImportWorkbench
            model={candidate.model}
            onIntent={onIntent}
            reviewFocusAuthorization={commitFocus}
          />
        </Committed>,
      ));
    } catch (error) {
      if (beganGeneration !== null) liveness.cancel(beganGeneration);
      document.dispatchEvent(new CustomEvent(IMPORT_REJECTED_EVENT, {
        detail: Object.freeze({
          generation: Number.isSafeInteger((event as CustomEvent<Record<string, unknown>>).detail?.generation)
            ? (event as CustomEvent<Record<string, unknown>>).detail.generation
            : null,
          reason: error instanceof Error ? error.message : "The import workbench refused its input.",
        }),
      }));
    }
  });
  document.dispatchEvent(new CustomEvent(IMPORT_AVAILABLE_EVENT));
}

const versionsHost = document.getElementById("workspace-versions-next");
if (versionsHost) {
  const shadow = versionsHost.shadowRoot ?? versionsHost.attachShadow({ mode: "open" });
  const style = document.createElement("style");
  style.textContent = stylesheet;
  const container = document.createElement("div");
  container.className = "mesh-review-island";
  shadow.replaceChildren(style, container);
  const liveness = projectionLiveness(VERSIONS_REJECTED_EVENT);
  const root = createRoot(container);
  let generation = -1;
  document.addEventListener(VERSIONS_PROJECTION_EVENT, (event) => {
    let beganGeneration: number | null = null;
    try {
      const candidate = workspaceVersionsEnvelope((event as CustomEvent<unknown>).detail, generation);
      generation = candidate.generation;
      const exactGeneration = candidate.generation;
      liveness.begin(exactGeneration);
      beganGeneration = exactGeneration;
      const onIntent = (intent: WorkspaceVersionsIntent) => {
        document.dispatchEvent(new CustomEvent(VERSIONS_INTENT_EVENT, {
          detail: Object.freeze({ generation: exactGeneration, intent: Object.freeze({ ...intent }) }),
        }));
      };
      root.render(guardedProjection(exactGeneration, liveness,
        <Committed onCommit={() => {
          if (!liveness.commit(exactGeneration)) return;
          document.dispatchEvent(new CustomEvent(VERSIONS_MOUNTED_EVENT, {
            detail: Object.freeze({ generation: exactGeneration }),
          }));
        }}>
          <WorkspaceVersionNavigator model={candidate.model} generation={exactGeneration} onIntent={onIntent} />
        </Committed>,
      ));
    } catch (error) {
      if (beganGeneration !== null) liveness.cancel(beganGeneration);
      document.dispatchEvent(new CustomEvent(VERSIONS_REJECTED_EVENT, {
        detail: Object.freeze({
          generation: Number.isSafeInteger((event as CustomEvent<Record<string, unknown>>).detail?.generation)
            ? (event as CustomEvent<Record<string, unknown>>).detail.generation
            : null,
          reason: error instanceof Error ? error.message : "The workspace version navigator refused its input.",
        }),
      }));
    }
  });
  document.dispatchEvent(new CustomEvent(VERSIONS_AVAILABLE_EVENT));
}

const chromeHost = document.getElementById("workspace-chrome-next");
if (chromeHost) {
  const installContainer = (host: HTMLElement) => {
    const shadow = host.shadowRoot ?? host.attachShadow({ mode: "open" });
    const style = document.createElement("style");
    style.textContent = stylesheet;
    const container = document.createElement("div");
    container.className = "mesh-review-island";
    shadow.replaceChildren(style, container);
    return container;
  };
  const liveness = projectionLiveness(CHROME_REJECTED_EVENT);
  const headerRoot = createRoot(installContainer(chromeHost));
  let generation = -1;
  document.addEventListener(CHROME_PROJECTION_EVENT, (event) => {
    let beganGeneration: number | null = null;
    try {
      const candidate = workspaceChromeEnvelope((event as CustomEvent<unknown>).detail, generation);
      generation = candidate.generation;
      liveness.begin(candidate.generation);
      beganGeneration = candidate.generation;
      const onCommit = () => {
        if (!liveness.commit(candidate.generation)) return;
        document.dispatchEvent(new CustomEvent(CHROME_MOUNTED_EVENT, {
          detail: Object.freeze({
            generation: candidate.generation,
            workspaceReady: candidate.model.workspaceReady,
          }),
        }));
      };
      headerRoot.render(guardedProjection(candidate.generation, liveness,
        <Committed onCommit={onCommit}>
          <WorkspaceHeader model={candidate.model} />
        </Committed>,
      ));
    } catch (error) {
      if (beganGeneration !== null) liveness.cancel(beganGeneration);
      document.dispatchEvent(new CustomEvent(CHROME_REJECTED_EVENT, {
        detail: Object.freeze({
          generation: Number.isSafeInteger((event as CustomEvent<Record<string, unknown>>).detail?.generation)
            ? (event as CustomEvent<Record<string, unknown>>).detail.generation
            : null,
          reason: error instanceof Error ? error.message : "The workspace chrome refused its input.",
        }),
      }));
    }
  });
  document.dispatchEvent(new CustomEvent(CHROME_AVAILABLE_EVENT));
}

const entryHost = document.getElementById("workspace-entry-next");
if (entryHost) {
  const shadow = entryHost.shadowRoot ?? entryHost.attachShadow({ mode: "open" });
  const style = document.createElement("style");
  style.textContent = stylesheet;
  const container = document.createElement("div");
  container.className = "mesh-review-island";
  shadow.replaceChildren(style, container);
  const liveness = projectionLiveness(ENTRY_REJECTED_EVENT);
  const root = createRoot(container);
  let generation = -1;
  document.addEventListener(ENTRY_PROJECTION_EVENT, (event) => {
    let beganGeneration: number | null = null;
    try {
      const candidate = workspaceEntryEnvelope((event as CustomEvent<unknown>).detail, generation);
      generation = candidate.generation;
      const exactGeneration = candidate.generation;
      liveness.begin(exactGeneration);
      beganGeneration = exactGeneration;
      const onIntent = (intent: WorkspaceEntryIntent) => {
        document.dispatchEvent(new CustomEvent(ENTRY_INTENT_EVENT, {
          detail: Object.freeze({ generation: exactGeneration, intent: Object.freeze({ ...intent }) }),
        }));
      };
      root.render(guardedProjection(exactGeneration, liveness,
        <Committed onCommit={() => {
          if (!liveness.commit(exactGeneration)) return;
          document.dispatchEvent(new CustomEvent(ENTRY_MOUNTED_EVENT, {
            detail: Object.freeze({ generation: exactGeneration }),
          }));
        }}>
          <WorkspaceEntryPage model={candidate.model} onIntent={onIntent} />
        </Committed>,
      ));
    } catch (error) {
      if (beganGeneration !== null) liveness.cancel(beganGeneration);
      document.dispatchEvent(new CustomEvent(ENTRY_REJECTED_EVENT, {
        detail: Object.freeze({
          generation: Number.isSafeInteger((event as CustomEvent<Record<string, unknown>>).detail?.generation)
            ? (event as CustomEvent<Record<string, unknown>>).detail.generation
            : null,
          reason: error instanceof Error ? error.message : "The workspace entry page refused its input.",
        }),
      }));
    }
  });
  document.dispatchEvent(new CustomEvent(ENTRY_AVAILABLE_EVENT));
}

const restoreHost = document.getElementById("workspace-restore-next");
if (restoreHost) {
  const shadow = restoreHost.shadowRoot ?? restoreHost.attachShadow({ mode: "open" });
  const style = document.createElement("style");
  style.textContent = stylesheet;
  const container = document.createElement("div");
  container.className = "mesh-review-island";
  shadow.replaceChildren(style, container);
  const liveness = projectionLiveness(RESTORE_REJECTED_EVENT);
  const root = createRoot(container);
  let generation = -1;
  document.addEventListener(RESTORE_PROJECTION_EVENT, (event) => {
    let beganGeneration: number | null = null;
    try {
      const candidate = workspaceRestoreEnvelope((event as CustomEvent<unknown>).detail, generation);
      generation = candidate.generation;
      const exactGeneration = candidate.generation;
      liveness.begin(exactGeneration);
      beganGeneration = exactGeneration;
      const onIntent = (intent: WorkspaceRestoreIntent) => {
        document.dispatchEvent(new CustomEvent(RESTORE_INTENT_EVENT, {
          detail: Object.freeze({ generation: exactGeneration, intent: Object.freeze({ ...intent }) }),
        }));
      };
      root.render(guardedProjection(exactGeneration, liveness,
        <Committed onCommit={() => {
          if (!liveness.commit(exactGeneration)) return;
          document.dispatchEvent(new CustomEvent(RESTORE_MOUNTED_EVENT, {
            detail: Object.freeze({ generation: exactGeneration }),
          }));
        }}>
          <WorkspaceRestore model={candidate.model} onIntent={onIntent} />
        </Committed>,
      ));
    } catch (error) {
      if (beganGeneration !== null) liveness.cancel(beganGeneration);
      document.dispatchEvent(new CustomEvent(RESTORE_REJECTED_EVENT, {
        detail: Object.freeze({
          generation: Number.isSafeInteger((event as CustomEvent<Record<string, unknown>>).detail?.generation)
            ? (event as CustomEvent<Record<string, unknown>>).detail.generation
            : null,
          reason: error instanceof Error ? error.message : "The workspace restore panel refused its input.",
        }),
      }));
    }
  });
  document.dispatchEvent(new CustomEvent(RESTORE_AVAILABLE_EVENT));
}

const destinationHost = document.getElementById("workspace-destination-next");
if (destinationHost) {
  const shadow = destinationHost.shadowRoot ?? destinationHost.attachShadow({ mode: "open" });
  const style = document.createElement("style");
  style.textContent = stylesheet;
  const container = document.createElement("div");
  container.className = "mesh-review-island";
  shadow.replaceChildren(style, container);
  let generation = -1;
  let liveGeneration = -1;
  let liveDestination = "";
  let livenessCheckQueued = false;
  let destinationTransitioning = false;
  const setDestinationTransitioning = (transitioning: boolean) => {
    destinationTransitioning = transitioning;
    destinationHost.setAttribute("aria-busy", String(transitioning));
  };
  const blockDestinationTransitionInteraction = (event: Event) => {
    if (!destinationTransitioning) return;
    if (event.type === "keydown" && (event as KeyboardEvent).key === "Tab") return;
    const transitionAction = event.target instanceof Element
      ? event.target.closest('[data-mesh-transition-action="read-only-preview"]')
      : null;
    if (transitionAction && container.contains(transitionAction)
      && ["click", "keydown", "mousedown", "pointerdown"].includes(event.type)) return;
    event.preventDefault();
    event.stopImmediatePropagation();
  };
  for (const eventName of ["beforeinput", "change", "click", "input", "keydown", "mousedown", "pointerdown", "submit"]) {
    container.addEventListener(eventName, blockDestinationTransitionInteraction, true);
  }
  const destinationGenerationIsLive = (candidate: number, expectedDestination: string) => {
    const mounted = container.querySelector('[data-mesh-proof="workspace-destination"]');
    const selected = container.querySelector('[data-mesh-proof="destination-selected"]');
    return mounted?.getAttribute("data-mesh-generation") === String(candidate)
      && selected?.tagName === "OUTPUT"
      && selected.textContent === expectedDestination
      && container.querySelector('[data-mesh-proof="destination-draft"]') !== null
      && container.querySelector('[data-mesh-proof="destination-choose"]') !== null
      && container.querySelector('[data-mesh-proof="destination-preview-all"]') !== null
      && container.querySelector('[data-mesh-proof="destination-confirm-all"]') !== null
      && container.querySelector('[data-mesh-proof="destination-hint"]') !== null;
  };
  const rejectDestinationGeneration = (candidate: number) => {
    if (!Number.isSafeInteger(candidate) || candidate < 0) return;
    if (liveGeneration === candidate) {
      liveGeneration = -1;
      liveDestination = "";
    }
    document.dispatchEvent(new CustomEvent(DESTINATION_REJECTED_EVENT, {
      detail: Object.freeze({
        generation: candidate,
        reason: "The destination update panel did not remain available. The current controls are restored.",
      }),
    }));
    if (generation === candidate) setDestinationTransitioning(false);
  };
  const root = createRoot(container, {
    onUncaughtError: () => rejectDestinationGeneration(generation),
  });
  const observer = new MutationObserver(() => {
    if (livenessCheckQueued || liveGeneration < 0) return;
    livenessCheckQueued = true;
    queueMicrotask(() => {
      livenessCheckQueued = false;
      const expectedGeneration = liveGeneration;
      const expectedDestination = liveDestination;
      if (expectedGeneration >= 0 && !destinationGenerationIsLive(expectedGeneration, expectedDestination)) {
        rejectDestinationGeneration(expectedGeneration);
      }
    });
  });
  observer.observe(container, { childList: true, subtree: true });
  document.addEventListener(DESTINATION_PROJECTION_EVENT, (event) => {
    try {
      const candidate = workspaceDestinationEnvelope((event as CustomEvent<unknown>).detail, generation);
      generation = candidate.generation;
      const exactGeneration = candidate.generation;
      // Destination verification intentionally waits for WebKit to settle an exact projected
      // value. Keep the already-mounted surface painted during that bounded interval, but make
      // both its previous and newly committed controls non-actionable until the coordinator
      // synchronously accepts this generation. The capture gate preserves keyboard focus and Tab
      // movement while preventing an edit or activation whose intent would still be stale.
      setDestinationTransitioning(true);
      const onIntent = (intent: WorkspaceDestinationIntent) => {
        document.dispatchEvent(new CustomEvent(DESTINATION_INTENT_EVENT, {
          detail: Object.freeze({ generation: exactGeneration, intent: Object.freeze({ ...intent }) }),
        }));
      };
      root.render(
        <Committed onCommit={() => {
          let verificationComplete = false;
          const verifyDestinationGeneration = () => {
            if (verificationComplete) return;
            verificationComplete = true;
            if (generation !== exactGeneration) return;
            if (!destinationGenerationIsLive(exactGeneration, candidate.model.destination)) {
              rejectDestinationGeneration(exactGeneration);
              return;
            }
            liveGeneration = exactGeneration;
            liveDestination = candidate.model.destination;
            document.dispatchEvent(new CustomEvent(DESTINATION_MOUNTED_EVENT, {
              detail: Object.freeze({ generation: exactGeneration }),
            }));
            if (generation === exactGeneration && liveGeneration === exactGeneration) {
              setDestinationTransitioning(false);
            }
          };
          const boundedVerification = setTimeout(verifyDestinationGeneration, 250);
          requestAnimationFrame(() => requestAnimationFrame(() => {
            clearTimeout(boundedVerification);
            verifyDestinationGeneration();
          }));
        }}>
          <WorkspaceDestination model={candidate.model} generation={exactGeneration} onIntent={onIntent} />
        </Committed>,
      );
    } catch (error) {
      document.dispatchEvent(new CustomEvent(DESTINATION_REJECTED_EVENT, {
        detail: Object.freeze({
          generation: Number.isSafeInteger((event as CustomEvent<Record<string, unknown>>).detail?.generation)
            ? (event as CustomEvent<Record<string, unknown>>).detail.generation
            : null,
          reason: error instanceof Error ? error.message : "The destination update panel refused its input.",
        }),
      }));
    }
  });
  document.dispatchEvent(new CustomEvent(DESTINATION_AVAILABLE_EVENT));
}

const currentHost = document.getElementById("workspace-current-next");
if (currentHost) {
  const shadow = currentHost.shadowRoot ?? currentHost.attachShadow({ mode: "open" });
  const style = document.createElement("style");
  style.textContent = stylesheet;
  const container = document.createElement("div");
  container.className = "mesh-review-island";
  shadow.replaceChildren(style, container);
  const liveness = projectionLiveness(CURRENT_REJECTED_EVENT);
  const root = createRoot(container);
  let generation = -1;
  document.addEventListener(CURRENT_PROJECTION_EVENT, (event) => {
    let beganGeneration: number | null = null;
    try {
      const candidate = workspaceCurrentEnvelope((event as CustomEvent<unknown>).detail, generation);
      generation = candidate.generation;
      const exactGeneration = candidate.generation;
      liveness.begin(exactGeneration);
      beganGeneration = exactGeneration;
      const onIntent = (intent: WorkspaceCurrentIntent) => {
        document.dispatchEvent(new CustomEvent(CURRENT_INTENT_EVENT, {
          detail: Object.freeze({ generation: exactGeneration, intent: Object.freeze({ ...intent }) }),
        }));
      };
      root.render(guardedProjection(exactGeneration, liveness,
        <Committed onCommit={() => {
          if (!liveness.commit(exactGeneration)) return;
          document.dispatchEvent(new CustomEvent(CURRENT_MOUNTED_EVENT, {
            detail: Object.freeze({ generation: exactGeneration }),
          }));
        }}>
          <WorkspaceCurrent model={candidate.model} generation={exactGeneration} onIntent={onIntent} />
        </Committed>,
      ));
    } catch (error) {
      if (beganGeneration !== null) liveness.cancel(beganGeneration);
      document.dispatchEvent(new CustomEvent(CURRENT_REJECTED_EVENT, {
        detail: Object.freeze({
          generation: Number.isSafeInteger((event as CustomEvent<Record<string, unknown>>).detail?.generation)
            ? (event as CustomEvent<Record<string, unknown>>).detail.generation
            : null,
          reason: error instanceof Error ? error.message : "The current workspace view refused its input.",
        }),
      }));
    }
  });
  document.dispatchEvent(new CustomEvent(CURRENT_AVAILABLE_EVENT));
}

const filesHost = document.getElementById("workspace-files-next");
const changesHost = document.getElementById("workspace-changes-next");
if (filesHost && changesHost) {
  const installContainer = (host: HTMLElement) => {
    const shadow = host.shadowRoot ?? host.attachShadow({ mode: "open" });
    const style = document.createElement("style");
    style.textContent = stylesheet;
    const container = document.createElement("div");
    container.className = "mesh-review-island";
    shadow.replaceChildren(style, container);
    return container;
  };
  const liveness = projectionLiveness(WORK_REJECTED_EVENT);
  const filesRoot = createRoot(installContainer(filesHost));
  const changesRoot = createRoot(installContainer(changesHost));
  let generation = -1;
  document.addEventListener(WORK_PROJECTION_EVENT, (event) => {
    let beganGeneration: number | null = null;
    try {
      const candidate = workspaceFilesChangesEnvelope((event as CustomEvent<unknown>).detail, generation);
      generation = candidate.generation;
      const exactGeneration = candidate.generation;
      liveness.begin(exactGeneration, ["files", "changes"]);
      beganGeneration = exactGeneration;
      const onCommit = (surface: "files" | "changes") => {
        if (!liveness.commit(exactGeneration, surface)) return;
        document.dispatchEvent(new CustomEvent(WORK_MOUNTED_EVENT, {
          detail: Object.freeze({ generation: exactGeneration }),
        }));
      };
      const onIntent = (intent: WorkspaceFilesChangesIntent) => {
        document.dispatchEvent(new CustomEvent(WORK_INTENT_EVENT, {
          detail: Object.freeze({ generation: exactGeneration, intent: Object.freeze({ ...intent }) }),
        }));
      };
      filesRoot.render(guardedProjection(exactGeneration, liveness,
        <Committed onCommit={() => onCommit("files")}>
          <WorkspaceFiles model={candidate.model} onIntent={onIntent} />
        </Committed>,
      ));
      changesRoot.render(guardedProjection(exactGeneration, liveness,
        <Committed onCommit={() => onCommit("changes")}>
          <WorkspaceChanges model={candidate.model} onIntent={onIntent} />
        </Committed>,
      ));
    } catch (error) {
      if (beganGeneration !== null) liveness.cancel(beganGeneration);
      document.dispatchEvent(new CustomEvent(WORK_REJECTED_EVENT, {
        detail: Object.freeze({
          generation: Number.isSafeInteger((event as CustomEvent<Record<string, unknown>>).detail?.generation)
            ? (event as CustomEvent<Record<string, unknown>>).detail.generation
            : null,
          reason: error instanceof Error ? error.message : "The files and changes workbench refused its input.",
        }),
      }));
    }
  });
  document.dispatchEvent(new CustomEvent(WORK_AVAILABLE_EVENT));
}

const confirmationHost = document.getElementById("confirmation-dialog-next");
if (confirmationHost) {
  const shadow = confirmationHost.shadowRoot ?? confirmationHost.attachShadow({ mode: "open" });
  const style = document.createElement("style");
  style.textContent = stylesheet;
  const container = document.createElement("div");
  shadow.replaceChildren(style, container);
  const root = createRoot(container);
  let generation = -1;
  let mountedGeneration: number | null = null;
  document.addEventListener(CONFIRMATION_PROJECTION_EVENT, (event) => {
    try {
      const candidate = confirmationEnvelope((event as CustomEvent<unknown>).detail, generation);
      generation = candidate.generation;
      mountedGeneration = candidate.generation;
      const exactGeneration = candidate.generation;
      root.render(
        <Committed onCommit={() => document.dispatchEvent(new CustomEvent(CONFIRMATION_MOUNTED_EVENT, {
          detail: Object.freeze({ generation: exactGeneration }),
        }))}>
          <ConfirmationDialog
            key={exactGeneration}
            model={candidate.model}
            onIntent={(value) => {
              const intent = confirmationIntent(value);
              document.dispatchEvent(new CustomEvent(CONFIRMATION_INTENT_EVENT, {
                detail: Object.freeze({ generation: exactGeneration, intent }),
              }));
            }}
          />
        </Committed>,
      );
    } catch (error) {
      document.dispatchEvent(new CustomEvent(CONFIRMATION_REJECTED_EVENT, {
        detail: Object.freeze({
          generation: Number.isSafeInteger((event as CustomEvent<Record<string, unknown>>).detail?.generation)
            ? (event as CustomEvent<Record<string, unknown>>).detail.generation
            : null,
          reason: error instanceof Error ? error.message : "The confirmation dialog refused its input.",
        }),
      }));
    }
  });
  document.addEventListener(CONFIRMATION_DISMISSED_EVENT, (event) => {
    if ((event as CustomEvent<Record<string, unknown>>).detail?.generation !== mountedGeneration) return;
    mountedGeneration = null;
    root.render(null);
  });
  confirmationHost.setAttribute("data-mesh-confirmation-ready", "true");
  document.dispatchEvent(new CustomEvent(CONFIRMATION_AVAILABLE_EVENT));
}
