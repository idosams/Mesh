import type { ProductionRoute } from "../models/production-route";
import type { ProductionShellState } from "../models/production-shell-store";
import { IslandSlot, WorkspaceView } from "../views/workspace-view";

type FilesWorkspaceRoute = Extract<ProductionRoute, { id: "files" }>;

export function FilesWorkspacePage({ active, route, surfaceFailures }: Readonly<{
  active: boolean;
  route: FilesWorkspaceRoute;
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
