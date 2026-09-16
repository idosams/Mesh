import { productionRoute, type WorkspacePageId } from "../models/production-route";

type ProductionDocumentRoot = Pick<Document, "getElementById">;
type ProductionFocusRequest = Readonly<{ page: WorkspacePageId; selector: string | null }>;

export function focusProductionMain(root: ProductionDocumentRoot): boolean {
  const host = root.getElementById("mesh-app-next");
  const main = host?.shadowRoot?.getElementById("mesh-react-main");
  if (!main) return false;
  main.focus({ preventScroll: true });
  return true;
}

export function focusProductionRoute(
  root: ProductionDocumentRoot,
  request: ProductionFocusRequest,
): boolean {
  const route = productionRoute(request.page);
  const host = route ? root.getElementById(route.focusHostId) : null;
  const target = request.selector ? host?.shadowRoot?.querySelector<HTMLElement>(request.selector) : null;
  const focusTarget = target ?? host;
  if (focusTarget && !host?.classList.contains("hidden")) {
    if (focusTarget === host && host.getAttribute("tabindex") === null) host.setAttribute("tabindex", "-1");
    focusTarget.focus({ preventScroll: true });
    return true;
  }
  return focusProductionMain(root);
}
