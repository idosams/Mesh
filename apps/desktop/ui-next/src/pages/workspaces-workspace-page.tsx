import type { ProductionRoute } from "../models/production-route";
import type { ProductionShellState } from "../models/production-shell-store";
import { IslandSlot, WorkspaceView } from "../views/workspace-view";

type WorkspacesWorkspaceRoute = Extract<ProductionRoute, { id: "workspaces" }>;

export function WorkspacesWorkspacePage({ active, route, surfaceFailures }: Readonly<{
  active: boolean;
  route: WorkspacesWorkspaceRoute;
  surfaceFailures: ProductionShellState["surfaceFailures"];
}>) {
  const slot = route.slots[0];
  return (
    <WorkspaceView active={active} label={route.pageLabel}>
      <IslandSlot
        name={slot.name}
        label={slot.loadingLabel}
        failed={surfaceFailures[slot.name]}
      />
    </WorkspaceView>
  );
}
