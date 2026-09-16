import {
  productionRoute,
  productionRouteFocusSelector,
  productionRouteIsAvailable,
  type WorkspacePageId,
} from "./production-route";

export type ProductionShellState = Readonly<{
  activePage: WorkspacePageId;
  workspaceReady: boolean;
  nativeChangeCount: number;
  notice: Readonly<{
    generation: number;
    message: string;
    error: boolean;
    proof: "agent-handoff-rescanned" | null;
  }> | null;
  buildIdentity: Readonly<{ label: string; title: string }>;
  focusRequest: Readonly<{ page: WorkspacePageId; selector: string | null; sequence: number }> | null;
  surfaceFailures: Readonly<Record<string, boolean>>;
}>;

let sequence = 0;
let explicitPage = false;
let workspaceIdentity: number | null = null;
let noticeGeneration = 0;
let noticeReplayActivated = false;
let state: ProductionShellState = Object.freeze({
  activePage: "workspaces",
  workspaceReady: false,
  nativeChangeCount: 0,
  notice: null,
  buildIdentity: Object.freeze({
    label: "Build identity unavailable",
    title: "Mesh refused a malformed or incomplete build identity.",
  }),
  focusRequest: null,
  surfaceFailures: Object.freeze({}),
});
const subscribers = new Set<() => void>();

function publish(next: ProductionShellState) {
  state = Object.freeze(next);
  for (const subscriber of subscribers) subscriber();
}

export function requestProductionPage(page: unknown, selector: unknown = null) {
  const route = productionRoute(page);
  const focusSelector = productionRouteFocusSelector(route?.id, selector);
  if (!route
    || !productionRouteIsAvailable(route.id, state.workspaceReady)
    || focusSelector === undefined) return false;
  explicitPage = true;
  publish({
    ...state,
    activePage: route.id,
    focusRequest: Object.freeze({ page: route.id, selector: focusSelector, sequence: ++sequence }),
  });
  return true;
}

export function subscribeProductionShell(subscriber: () => void) {
  subscribers.add(subscriber);
  return () => subscribers.delete(subscriber);
}

export function productionShellSnapshot() {
  return state;
}

document.addEventListener("mesh:workspace-page-request", (event) => {
  const detail = (event as CustomEvent<Record<string, unknown>>).detail;
  requestProductionPage(detail?.page, detail?.selector);
});

document.addEventListener("mesh:workspace-chrome-projection", (event) => {
  const detail = (event as CustomEvent<Record<string, unknown>>).detail;
  const chrome = detail?.chrome as Record<string, unknown> | undefined;
  if (!Number.isSafeInteger(detail?.workspaceIdentity)
    || (detail.workspaceIdentity as number) < 0
    || typeof chrome?.workspaceReady !== "boolean"
    || !Number.isSafeInteger(chrome.nativeChangeCount)
    || (chrome.nativeChangeCount as number) < 0
    || (chrome.nativeChangeCount as number) > 1_000_000) return;
  const nextWorkspaceIdentity = detail.workspaceIdentity as number;
  const identityChanged = workspaceIdentity !== null && workspaceIdentity !== nextWorkspaceIdentity;
  const openedWorkspace = chrome.workspaceReady && !state.workspaceReady;
  const becameUnavailable = state.workspaceReady && !chrome.workspaceReady;
  if (identityChanged || becameUnavailable) explicitPage = false;
  workspaceIdentity = nextWorkspaceIdentity;
  const activePage = !chrome.workspaceReady
    ? identityChanged || becameUnavailable ? "workspaces" : state.activePage
    : identityChanged || (openedWorkspace && !explicitPage) ? "current" : state.activePage;
  const focusRequest = identityChanged || becameUnavailable
    ? Object.freeze({ page: activePage, selector: null, sequence: ++sequence })
    : state.focusRequest;
  if (!identityChanged
    && state.workspaceReady === chrome.workspaceReady
    && state.nativeChangeCount === chrome.nativeChangeCount
    && state.activePage === activePage
    && state.focusRequest === focusRequest) return;
  publish({
    ...state,
    workspaceReady: chrome.workspaceReady,
    nativeChangeCount: chrome.nativeChangeCount as number,
    activePage,
    focusRequest,
  });
});

document.addEventListener("mesh:notice-projection", (event) => {
  if (!noticeReplayActivated) return;
  const detail = (event as CustomEvent<Record<string, unknown>>).detail;
  if (!detail
    || Object.keys(detail).sort().join(",") !== "error,generation,message,proof,schema"
    || detail.schema !== "mesh.notice/v1"
    || !Number.isSafeInteger(detail.generation)
    || (detail.generation as number) <= noticeGeneration
    || typeof detail.message !== "string"
    || detail.message.length === 0
    || detail.message.length > 4_096
    || typeof detail.error !== "boolean"
    || !(detail.proof === null || detail.proof === "agent-handoff-rescanned")
    || (detail.proof !== null && detail.error)) return;
  noticeGeneration = detail.generation as number;
  publish({
    ...state,
    notice: Object.freeze({
      generation: noticeGeneration,
      message: detail.message,
      error: detail.error,
      proof: detail.proof,
    }),
  });
});

export function activateProductionNoticeReplay() {
  if (noticeReplayActivated) return;
  noticeReplayActivated = true;
  document.dispatchEvent(new Event("mesh:notice-snapshot-request"));
}

document.addEventListener("mesh:build-identity-projection", (event) => {
  const detail = (event as CustomEvent<Record<string, unknown>>).detail;
  if (!detail
    || Object.keys(detail).sort().join(",") !== "label,title"
    || typeof detail.label !== "string"
    || detail.label.length === 0
    || detail.label.length > 80
    || typeof detail.title !== "string"
    || detail.title.length === 0
    || detail.title.length > 256) return;
  publish({ ...state, buildIdentity: Object.freeze({ label: detail.label, title: detail.title }) });
});
document.dispatchEvent(new CustomEvent("mesh:build-identity-available"));

const surfaceLifecycles = Object.freeze([
  ["mesh:workspace-entry-projection", "mesh:workspace-entry-mounted", "mesh:workspace-entry-rejected", ["workspace-entry"]],
  ["mesh:import-workbench-projection", "mesh:import-workbench-mounted", "mesh:import-workbench-rejected", ["import-workbench"]],
  ["mesh:workspace-overview-projection", "mesh:workspace-overview-mounted", "mesh:workspace-overview-rejected", ["workspace-overview"]],
  ["mesh:workspace-current-projection", "mesh:workspace-current-mounted", "mesh:workspace-current-rejected", ["workspace-current"]],
  ["mesh:workspace-files-changes-projection", "mesh:workspace-files-changes-mounted", "mesh:workspace-files-changes-rejected", ["workspace-files", "workspace-changes"]],
  ["mesh:review-workbench-projection", "mesh:review-workbench-mounted", "mesh:review-workbench-rejected", ["review-workbench"]],
  ["mesh:workspace-versions-projection", "mesh:workspace-versions-mounted", "mesh:workspace-versions-rejected", ["workspace-versions"]],
  ["mesh:workspace-destination-projection", "mesh:workspace-destination-mounted", "mesh:workspace-destination-rejected", ["workspace-destination"]],
  ["mesh:workspace-restore-projection", "mesh:workspace-restore-mounted", "mesh:workspace-restore-rejected", ["workspace-restore"]],
] as const);

for (const [projectionEvent, mountedEvent, rejectedEvent, surfaces] of surfaceLifecycles) {
  let generation: number | null = null;
  document.addEventListener(projectionEvent, (event) => {
    const candidate = (event as CustomEvent<Record<string, unknown>>).detail?.generation;
    if (Number.isSafeInteger(candidate) && (candidate as number) >= 0) generation = candidate as number;
  });
  document.addEventListener(mountedEvent, (event) => {
    const candidate = (event as CustomEvent<Record<string, unknown>>).detail?.generation;
    if (candidate !== generation) return;
    const surfaceFailures = { ...state.surfaceFailures };
    for (const surface of surfaces) delete surfaceFailures[surface];
    publish({ ...state, surfaceFailures: Object.freeze(surfaceFailures) });
  });
  document.addEventListener(rejectedEvent, (event) => {
    const candidate = (event as CustomEvent<Record<string, unknown>>).detail?.generation;
    if (candidate !== generation) return;
    const surfaceFailures = { ...state.surfaceFailures };
    for (const surface of surfaces) surfaceFailures[surface] = true;
    publish({ ...state, surfaceFailures: Object.freeze(surfaceFailures) });
  });
}
