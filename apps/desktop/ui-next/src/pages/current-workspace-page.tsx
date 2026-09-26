import { useTranslation } from "../lib/localization";
import type { ProductionRoute } from "../models/production-route";
import type { ProductionShellState } from "../models/production-shell-store";
import { IslandSlot, WorkspaceView } from "../views/workspace-view";

type CurrentWorkspaceRoute = Extract<ProductionRoute, { id: "current" }>;

export function CurrentWorkspacePage({ active, route, surfaceFailures }: Readonly<{
  active: boolean;
  route: CurrentWorkspaceRoute;
  surfaceFailures: ProductionShellState["surfaceFailures"];
}>) {
  const t = useTranslation();
  return (
    <WorkspaceView active={active} label={t(route.pageLabel)}>
      <div className="grid gap-4">
        {route.slots.map((slot) => (
          <IslandSlot
            key={slot.name}
            name={slot.name}
            label={t(slot.loadingLabel)}
            failed={surfaceFailures[slot.name]}
          />
        ))}
      </div>
    </WorkspaceView>
  );
}
