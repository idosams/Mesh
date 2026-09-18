import { useEffect, useLayoutEffect, useSyncExternalStore } from "react";
import { flushSync } from "react-dom";
import { ProductionWorkspaceLayout } from "../layouts/production-workspace-layout";
import { ProductionNavigation } from "../organisms/production-navigation";
import { focusProductionRoute } from "../lib/production-focus";
import { ChangesWorkspacePage } from "./changes-workspace-page";
import { CurrentWorkspacePage } from "./current-workspace-page";
import { FilesWorkspacePage } from "./files-workspace-page";
import { ImportWorkspacePage } from "./import-workspace-page";
import { ReviewWorkspacePage } from "./review-workspace-page";
import { RestoreWorkspacePage } from "./restore-workspace-page";
import { UpdateDestinationWorkspacePage } from "./update-destination-workspace-page";
import { VersionsWorkspacePage } from "./versions-workspace-page";
import { WorkspacesWorkspacePage } from "./workspaces-workspace-page";
import {
  productionRoutes,
  type ProductionRoute,
  type WorkspacePageId,
} from "../models/production-route";
import {
  activateProductionNoticeReplay,
  productionShellSnapshot,
  requestProductionPage,
  subscribeProductionShell,
  type ProductionShellState,
} from "../models/production-shell-store";

type FocusRequest = Readonly<{ page: WorkspacePageId; selector: string | null; sequence: number }>;

function focusPage(request: FocusRequest) {
  focusProductionRoute(document, request);
}

function ProductionRoutePage({ route, activePage, surfaceFailures }: Readonly<{
  route: ProductionRoute;
  activePage: WorkspacePageId;
  surfaceFailures: ProductionShellState["surfaceFailures"];
}>) {
  const active = activePage === route.id;
  switch (route.id) {
    case "workspaces":
      return <WorkspacesWorkspacePage active={active} route={route} surfaceFailures={surfaceFailures} />;
    case "import":
      return <ImportWorkspacePage active={active} route={route} surfaceFailures={surfaceFailures} />;
    case "current":
      return <CurrentWorkspacePage active={active} route={route} surfaceFailures={surfaceFailures} />;
    case "files":
      return <FilesWorkspacePage active={active} route={route} surfaceFailures={surfaceFailures} />;
    case "changes":
      return <ChangesWorkspacePage active={active} route={route} surfaceFailures={surfaceFailures} />;
    case "review":
      return <ReviewWorkspacePage active={active} route={route} surfaceFailures={surfaceFailures} />;
    case "versions":
      return <VersionsWorkspacePage active={active} route={route} surfaceFailures={surfaceFailures} />;
    case "update":
      return <UpdateDestinationWorkspacePage active={active} route={route} surfaceFailures={surfaceFailures} />;
    case "restore":
      return <RestoreWorkspacePage active={active} route={route} surfaceFailures={surfaceFailures} />;
  }
  route satisfies never;
  return null;
}

export function ProductionWorkspacePage() {
  const { activePage, workspaceReady, nativeChangeCount, notice, buildIdentity, focusRequest, surfaceFailures } = useSyncExternalStore(
    subscribeProductionShell,
    productionShellSnapshot,
    productionShellSnapshot,
  );

  useLayoutEffect(() => {
    if (focusRequest) focusPage(focusRequest);
  }, [focusRequest]);

  useLayoutEffect(() => {
    document.dispatchEvent(new CustomEvent("mesh:react-shell-committed"));
  });

  useEffect(() => {
    activateProductionNoticeReplay();
  }, []);

  return (
    <>
      <ProductionWorkspaceLayout
      header={<slot name="workspace-header" />}
      navigation={(
        <div className="flex gap-1 overflow-x-auto border-b border-border bg-background/80 px-1">
          <ProductionNavigation
            activePage={activePage}
            workspaceReady={workspaceReady}
            nativeChangeCount={nativeChangeCount}
            onNavigate={(page) => flushSync(() => requestProductionPage(page))}
          />
        </div>
      )}
      notice={(
        <div data-mesh-proof="production-notice" data-mesh-notice-generation={notice?.generation} data-mesh-agent-proof={notice?.proof ?? undefined} className={notice ? `mt-4 whitespace-pre-line rounded-xl border p-3 text-sm leading-6 ${notice.error ? "border-destructive text-red-200" : "border-primary/40 text-foreground"}` : undefined}>
          <div role="status" aria-live="polite" aria-atomic="true">
            {notice && !notice.error ? notice.message : null}
          </div>
          <div role="alert" aria-live="assertive" aria-atomic="true">
            {notice?.error ? notice.message : null}
          </div>
        </div>
      )}
      buildIdentity={buildIdentity}
    >
      {productionRoutes.map((route) => (
        <ProductionRoutePage
          key={route.id}
          route={route}
          activePage={activePage}
          surfaceFailures={surfaceFailures}
        />
      ))}
      </ProductionWorkspaceLayout>
      <slot name="confirmation-dialog" />
    </>
  );
}
