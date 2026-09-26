import { useTranslation } from "../lib/localization";
import type { ProductionRoute } from "../models/production-route";
import type { ProductionShellState } from "../models/production-shell-store";
import { IslandSlot, WorkspaceView } from "../views/workspace-view";

type UpdateDestinationWorkspaceRoute = Extract<ProductionRoute, { id: "update" }>;

export function UpdateDestinationWorkspacePage({ active, route, surfaceFailures }: Readonly<{
  active: boolean;
  route: UpdateDestinationWorkspaceRoute;
  surfaceFailures: ProductionShellState["surfaceFailures"];
}>) {
  const t = useTranslation();
  const slot = route.slots[0];
  return (
    <WorkspaceView active={active} label={t(route.pageLabel)}>
      <IslandSlot
        name={slot.name}
        label={t(slot.loadingLabel)}
        failed={surfaceFailures[slot.name]}
      />
    </WorkspaceView>
  );
}
