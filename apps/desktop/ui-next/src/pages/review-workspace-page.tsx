import type { ProductionRoute } from "../models/production-route";
import type { ProductionShellState } from "../models/production-shell-store";
import { IslandSlot, WorkspaceView } from "../views/workspace-view";

type ReviewWorkspaceRoute = Extract<ProductionRoute, { id: "review" }>;

export function ReviewWorkspacePage({ active, route, surfaceFailures }: Readonly<{
  active: boolean;
  route: ReviewWorkspaceRoute;
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
