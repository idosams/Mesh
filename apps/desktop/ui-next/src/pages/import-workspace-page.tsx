import type { ProductionRoute } from "../models/production-route";
import type { ProductionShellState } from "../models/production-shell-store";
import { IslandSlot, WorkspaceView } from "../views/workspace-view";

type ImportWorkspaceRoute = Extract<ProductionRoute, { id: "import" }>;

export function ImportWorkspacePage({ active, route, surfaceFailures }: Readonly<{
  active: boolean;
  route: ImportWorkspaceRoute;
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
