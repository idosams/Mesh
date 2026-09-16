export const productionRoutes = Object.freeze([
  Object.freeze({
    id: "workspaces",
    label: "Workspaces",
    pageLabel: "Workspaces",
    workspaceRequired: false,
    focusHostId: "workspace-entry-next",
    focusSelectorPolicy: "host-only",
    slots: Object.freeze([
      Object.freeze({ name: "workspace-entry", loadingLabel: "Preparing workspace controls…" }),
    ]),
  }),
  Object.freeze({
    id: "import",
    label: "Import",
    pageLabel: "Import folder",
    workspaceRequired: false,
    focusHostId: "import-workbench-next",
    focusSelectorPolicy: "import-chooser",
    slots: Object.freeze([
      Object.freeze({ name: "import-workbench", loadingLabel: "Preparing folder import…" }),
    ]),
  }),
  Object.freeze({
    id: "current",
    label: "Current",
    pageLabel: "Current workspace",
    workspaceRequired: true,
    focusHostId: "workspace-current-next",
    focusSelectorPolicy: "host-only",
    slots: Object.freeze([
      Object.freeze({ name: "workspace-overview", loadingLabel: "Preparing workspace summary…" }),
      Object.freeze({ name: "workspace-current", loadingLabel: "Preparing current workspace…" }),
    ]),
  }),
  Object.freeze({
    id: "files",
    label: "Files",
    pageLabel: "Files",
    workspaceRequired: true,
    focusHostId: "workspace-files-next",
    focusSelectorPolicy: "host-only",
    slots: Object.freeze([
      Object.freeze({ name: "workspace-files", loadingLabel: "Preparing files…" }),
    ]),
  }),
  Object.freeze({
    id: "changes",
    label: "Changes",
    pageLabel: "Changes",
    workspaceRequired: true,
    focusHostId: "workspace-changes-next",
    focusSelectorPolicy: "changes-workflow",
    slots: Object.freeze([
      Object.freeze({ name: "workspace-changes", loadingLabel: "Preparing changes…" }),
    ]),
  }),
  Object.freeze({
    id: "review",
    label: "Review",
    pageLabel: "Review",
    workspaceRequired: true,
    focusHostId: "review-workbench-next",
    focusSelectorPolicy: "host-only",
    slots: Object.freeze([
      Object.freeze({ name: "review-workbench", loadingLabel: "Preparing review…" }),
    ]),
  }),
  Object.freeze({
    id: "versions",
    label: "Versions",
    pageLabel: "Versions",
    workspaceRequired: true,
    focusHostId: "workspace-versions-next",
    focusSelectorPolicy: "saved-version",
    slots: Object.freeze([
      Object.freeze({ name: "workspace-versions", loadingLabel: "Preparing versions…" }),
    ]),
  }),
  Object.freeze({
    id: "update",
    label: "Update destination",
    pageLabel: "Update destination",
    workspaceRequired: true,
    focusHostId: "workspace-destination-next",
    focusSelectorPolicy: "update-destination",
    slots: Object.freeze([
      Object.freeze({ name: "workspace-destination", loadingLabel: "Preparing destination…" }),
    ]),
  }),
  Object.freeze({
    id: "restore",
    label: "Restore",
    pageLabel: "Restore",
    workspaceRequired: true,
    focusHostId: "workspace-restore-next",
    focusSelectorPolicy: "host-only",
    slots: Object.freeze([
      Object.freeze({ name: "workspace-restore", loadingLabel: "Preparing restore…" }),
    ]),
  }),
] as const);

export type ProductionRoute = (typeof productionRoutes)[number];
export type WorkspacePageId = ProductionRoute["id"];

const routesById = new Map<string, ProductionRoute>(
  productionRoutes.map((route) => [route.id, route]),
);

export function productionRoute(value: unknown): ProductionRoute | null {
  return typeof value === "string" ? routesById.get(value) ?? null : null;
}

export function productionRouteIsAvailable(value: unknown, workspaceReady: boolean): boolean {
  const route = productionRoute(value);
  return Boolean(route && (workspaceReady === true || !route.workspaceRequired));
}

const activeSavedVersionSelector = '[role="radio"][tabindex="0"]:not(:disabled)';
const exactSavedVersionSelector = /^\[data-mesh-version-operation="[0-9a-f]{64}"\]:not\(:disabled\)$/u;
const updateDestinationSelectors = new Set([
  '[data-mesh-proof="destination-draft"]',
  '[data-mesh-proof="destination-choose"]',
  '[data-mesh-proof="destination-preview-all"]',
]);
const importChooserSelector = '[data-mesh-import-choose]';
const changesWorkflowSelectors = new Set([
  'textarea',
  '[data-mesh-work-action="scan-files"]',
  '[data-mesh-work-action="save-all-private"]',
  '[data-mesh-work-field="missingSource"]',
  '[data-mesh-native-queue]',
]);

export function productionRouteFocusSelector(
  routeValue: unknown,
  selectorValue: unknown,
): string | null | undefined {
  const route = productionRoute(routeValue);
  if (!route) return undefined;
  if (selectorValue === null) return null;
  if (typeof selectorValue !== "string") return undefined;
  if (route.focusSelectorPolicy === "saved-version") {
    return selectorValue === activeSavedVersionSelector || exactSavedVersionSelector.test(selectorValue)
      ? selectorValue
      : undefined;
  }
  if (route.focusSelectorPolicy === "import-chooser") {
    return selectorValue === importChooserSelector ? selectorValue : undefined;
  }
  if (route.focusSelectorPolicy === "changes-workflow") {
    return changesWorkflowSelectors.has(selectorValue) ? selectorValue : undefined;
  }
  if (route.focusSelectorPolicy === "update-destination") {
    return updateDestinationSelectors.has(selectorValue) ? selectorValue : undefined;
  }
  return undefined;
}
