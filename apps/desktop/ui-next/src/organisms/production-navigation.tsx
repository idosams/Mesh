import {
  productionRouteIsAvailable,
  productionRoutes,
  type WorkspacePageId,
} from "../models/production-route";

export function ProductionNavigation({ activePage, workspaceReady, nativeChangeCount, onNavigate }: Readonly<{
  activePage: WorkspacePageId;
  workspaceReady: boolean;
  nativeChangeCount: number;
  onNavigate: (page: WorkspacePageId) => void;
}>) {
  return (
    <nav aria-label="Primary pages" className="flex gap-1.5">
      {productionRoutes.filter((page) => productionRouteIsAvailable(page.id, workspaceReady)).map((page) => {
        const active = activePage === page.id;
        return (
          <button
            key={page.id}
            type="button"
            className={`min-h-11 shrink-0 rounded-lg px-3 text-xs font-semibold hover:bg-secondary hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${active ? "bg-secondary text-foreground" : "text-muted-foreground"}`}
            aria-current={active ? "page" : undefined}
            data-state={active ? "active" : "inactive"}
            onClick={() => onNavigate(page.id)}
          >
            {page.label}
            {page.id === "changes" && nativeChangeCount > 0 ? (
              <span className="ml-2 min-w-5 rounded-full bg-primary px-1.5 py-0.5 text-center text-[10px] text-primary-foreground" aria-label={`${nativeChangeCount} folder changes`}>
                {nativeChangeCount}
              </span>
            ) : null}
          </button>
        );
      })}
    </nav>
  );
}
