import { useTranslation } from "../lib/localization";
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
  const t = useTranslation();
  return (
    <nav aria-label={t("Primary pages")} className="flex gap-1.5">
      {productionRoutes.filter((page) => productionRouteIsAvailable(page.id, workspaceReady)).map((page) => {
        const active = activePage === page.id;
        return (
          <button
            key={page.id}
            type="button"
            className={`relative min-h-11 shrink-0 border-b-2 px-3 text-xs font-semibold hover:bg-secondary/50 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset ${active ? "border-primary text-foreground" : "border-transparent text-muted-foreground"}`}
            aria-current={active ? "page" : undefined}
            data-state={active ? "active" : "inactive"}
            onClick={() => onNavigate(page.id)}
          >
            {t(page.label)}
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
