import type { ProductionRoute } from "../models/production-route";
import type { ProductionShellState } from "../models/production-shell-store";
import { IslandSlot, WorkspaceView } from "../views/workspace-view";

type ChangesWorkspaceRoute = Extract<ProductionRoute, { id: "changes" }>;

export function ChangesWorkspacePage({ active, route, surfaceFailures }: Readonly<{
  active: boolean;
  route: ChangesWorkspaceRoute;
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
